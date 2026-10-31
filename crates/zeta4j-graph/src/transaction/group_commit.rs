// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # Group Commit (WAL 배치 쓰기 최적화)
//!
//! 여러 트랜잭션의 WAL 레코드를 모아서 한 번에 fsync하여 I/O 성능을 향상시킵니다.
//!
//! ## 동작 원리
//!
//! ```text
//! 트랜잭션 1 ─┐
//! 트랜잭션 2 ─┼─► GroupCommitManager ─► 배치 WAL 쓰기 ─► 단일 fsync
//! 트랜잭션 3 ─┘
//! ```
//!
//! ## 플러시 조건
//!
//! 다음 조건 중 하나라도 충족되면 배치 플러시:
//! - `max_batch_size` 도달 (트랜잭션 수)
//! - `max_wait_ms` 경과 (최대 대기 시간)
//! - `max_batch_bytes` 도달 (배치 크기)
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::transaction::group_commit::{GroupCommitManager, GroupCommitConfig};
//!
//! let config = GroupCommitConfig::default()
//!     .with_max_batch_size(100)
//!     .with_max_wait_ms(10);
//!
//! let manager = GroupCommitManager::new(config, wal_writer);
//!
//! // 커밋 요청 (비동기)
//! manager.enqueue_commit(tx_id, records).await?;
//! ```

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::wal::LogRecord;
use super::{Result, TransactionError, TransactionId};

/// LogRecord 직렬화 헬퍼
fn serialize_record(record: &LogRecord) -> std::result::Result<Vec<u8>, TransactionError> {
    zeta4j_codec::serialize(record).map_err(|e| TransactionError::SerializationError(e.to_string()))
}

/// LogRecord 직렬화 크기 계산
fn record_serialized_size(record: &LogRecord) -> usize {
    zeta4j_codec::serialized_size(record).unwrap_or(0) as usize
}

/// Group Commit 설정
#[derive(Debug, Clone)]
pub struct GroupCommitConfig {
    /// 최대 배치 크기 (트랜잭션 수)
    pub max_batch_size: usize,
    /// 최대 대기 시간 (밀리초)
    pub max_wait_ms: u64,
    /// 최대 배치 바이트
    pub max_batch_bytes: usize,
    /// 활성화 여부
    pub enabled: bool,
}

impl Default for GroupCommitConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 100,
            max_wait_ms: 10,
            max_batch_bytes: 1024 * 1024, // 1MB
            enabled: true,
        }
    }
}

impl GroupCommitConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 최대 배치 크기 설정
    pub fn with_max_batch_size(mut self, size: usize) -> Self {
        self.max_batch_size = size;
        self
    }

    /// 최대 대기 시간 설정 (밀리초)
    pub fn with_max_wait_ms(mut self, ms: u64) -> Self {
        self.max_wait_ms = ms;
        self
    }

    /// 최대 배치 바이트 설정
    pub fn with_max_batch_bytes(mut self, bytes: usize) -> Self {
        self.max_batch_bytes = bytes;
        self
    }

    /// 활성화 설정
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// 대기 중인 커밋 정보
struct PendingCommit {
    #[expect(dead_code)] // 디버깅/로깅용 보존
    tx_id: TransactionId,
    records: Vec<LogRecord>,
    #[expect(dead_code)] // 통계용 보존
    byte_size: usize,
    result_sender: oneshot::Sender<Result<()>>,
    enqueue_time: Instant,
}

/// Group Commit 통계
#[derive(Debug, Clone, Default)]
pub struct GroupCommitStats {
    /// 총 커밋 수
    pub total_commits: u64,
    /// 총 배치 수
    pub total_batches: u64,
    /// 총 fsync 수
    pub total_fsyncs: u64,
    /// 평균 배치 크기
    pub avg_batch_size: f64,
    /// 평균 대기 시간 (마이크로초)
    pub avg_wait_time_us: u64,
    /// 총 바이트 쓰기
    pub total_bytes_written: u64,
}

/// WAL Writer trait
pub trait WalWriter: Send + Sync {
    /// 레코드들을 WAL에 쓰기 (fsync 없이)
    fn write_records(&self, records: &[LogRecord]) -> Result<usize>;

    /// fsync 수행
    fn sync(&self) -> Result<()>;
}

/// File 기반 WAL Writer 구현
pub struct FileWalWriter {
    writer: Mutex<BufWriter<File>>,
}

impl FileWalWriter {
    /// 새 FileWalWriter 생성
    pub fn new(file: File) -> Self {
        Self {
            writer: Mutex::new(BufWriter::new(file)),
        }
    }
}

impl WalWriter for FileWalWriter {
    fn write_records(&self, records: &[LogRecord]) -> Result<usize> {
        let mut writer = self.writer.lock();
        let mut total_bytes = 0;

        for record in records {
            let serialized = serialize_record(record)?;
            writer
                .write_all(&serialized)
                .map_err(|e| TransactionError::WalWriteFailed(e.to_string()))?;
            total_bytes += serialized.len();
        }

        Ok(total_bytes)
    }

    fn sync(&self) -> Result<()> {
        let mut writer = self.writer.lock();
        writer
            .flush()
            .map_err(|e| TransactionError::WalSyncFailed(e.to_string()))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|e| TransactionError::WalSyncFailed(e.to_string()))?;
        Ok(())
    }
}

/// Group Commit Manager
///
/// 여러 트랜잭션의 커밋을 모아서 배치 처리합니다.
pub struct GroupCommitManager {
    config: GroupCommitConfig,
    pending: Arc<Mutex<VecDeque<PendingCommit>>>,
    pending_bytes: AtomicU64,
    notify: Arc<tokio::sync::Notify>,
    shutdown: Arc<AtomicBool>,
    flush_task: Mutex<Option<tokio::task::JoinHandle<()>>>,

    // 통계
    total_commits: AtomicU64,
    total_batches: AtomicU64,
    total_fsyncs: AtomicU64,
    total_wait_time_us: AtomicU64,
    total_bytes_written: AtomicU64,
}

impl GroupCommitManager {
    /// 새 GroupCommitManager 생성
    ///
    /// tokio 런타임 컨텍스트에서 호출해야 합니다 (tokio::spawn 사용).
    pub fn new<W: WalWriter + 'static>(config: GroupCommitConfig, wal_writer: Arc<W>) -> Arc<Self> {
        let manager = Arc::new(Self {
            config: config.clone(),
            pending: Arc::new(Mutex::new(VecDeque::new())),
            pending_bytes: AtomicU64::new(0),
            notify: Arc::new(tokio::sync::Notify::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            flush_task: Mutex::new(None),
            total_commits: AtomicU64::new(0),
            total_batches: AtomicU64::new(0),
            total_fsyncs: AtomicU64::new(0),
            total_wait_time_us: AtomicU64::new(0),
            total_bytes_written: AtomicU64::new(0),
        });

        if config.enabled {
            let mgr = Arc::clone(&manager);
            let handle = tokio::spawn(async move {
                Self::flush_loop_async(mgr, wal_writer).await;
            });

            *manager.flush_task.lock() = Some(handle);
        }

        manager
    }

    /// 커밋 요청 등록 (비동기)
    pub async fn enqueue_commit(
        &self,
        tx_id: TransactionId,
        records: Vec<LogRecord>,
    ) -> Result<()> {
        if !self.config.enabled {
            return Err(TransactionError::WalWriteFailed(
                "Group commit disabled".to_string(),
            ));
        }

        let byte_size: usize = records.iter().map(record_serialized_size).sum();
        let (sender, receiver) = oneshot::channel();

        {
            let mut pending = self.pending.lock();
            pending.push_back(PendingCommit {
                tx_id,
                records,
                byte_size,
                result_sender: sender,
                enqueue_time: Instant::now(),
            });
            self.pending_bytes
                .fetch_add(byte_size as u64, Ordering::Relaxed);
        }

        // 플러시 스레드에 알림
        self.notify.notify_one();

        // 결과 대기
        receiver
            .await
            .map_err(|_| TransactionError::WalWriteFailed("Commit cancelled".to_string()))?
    }

    /// 동기식 커밋 등록 (블로킹)
    ///
    /// Note: 이 함수는 내부적으로 tokio 런타임을 사용합니다.
    /// 이미 tokio 런타임 컨텍스트에서 실행 중이라면 `enqueue_commit`을 사용하세요.
    pub fn enqueue_commit_sync(&self, tx_id: TransactionId, records: Vec<LogRecord>) -> Result<()> {
        if !self.config.enabled {
            return Err(TransactionError::WalWriteFailed(
                "Group commit disabled".to_string(),
            ));
        }

        let byte_size: usize = records.iter().map(record_serialized_size).sum();
        let (sender, receiver) = oneshot::channel();

        {
            let mut pending = self.pending.lock();
            pending.push_back(PendingCommit {
                tx_id,
                records,
                byte_size,
                result_sender: sender,
                enqueue_time: Instant::now(),
            });
            self.pending_bytes
                .fetch_add(byte_size as u64, Ordering::Relaxed);
        }

        self.notify.notify_one();

        // 플러시 결과 대기
        match receiver.blocking_recv() {
            Ok(result) => result,
            Err(_) => {
                // 타임아웃 또는 채널 닫힘
                Err(TransactionError::WalWriteFailed(
                    "Commit timeout or cancelled".to_string(),
                ))
            }
        }
    }

    /// 즉시 플러시 강제
    pub fn force_flush(&self) {
        self.notify.notify_waiters();
    }

    /// 통계 조회
    pub fn stats(&self) -> GroupCommitStats {
        let total_commits = self.total_commits.load(Ordering::Relaxed);
        let total_batches = self.total_batches.load(Ordering::Relaxed);

        GroupCommitStats {
            total_commits,
            total_batches,
            total_fsyncs: self.total_fsyncs.load(Ordering::Relaxed),
            avg_batch_size: if total_batches > 0 {
                total_commits as f64 / total_batches as f64
            } else {
                0.0
            },
            avg_wait_time_us: self
                .total_wait_time_us
                .load(Ordering::Relaxed)
                .checked_div(total_commits)
                .unwrap_or(0),
            total_bytes_written: self.total_bytes_written.load(Ordering::Relaxed),
        }
    }

    /// 대기 중인 커밋 수
    pub fn pending_count(&self) -> usize {
        self.pending.lock().len()
    }

    /// 대기 중인 바이트 수
    pub fn pending_bytes(&self) -> u64 {
        self.pending_bytes.load(Ordering::Relaxed)
    }

    /// 비동기 플러시 루프 (tokio::spawn으로 실행)
    async fn flush_loop_async<W: WalWriter + 'static>(mgr: Arc<Self>, wal_writer: Arc<W>) {
        let max_wait = Duration::from_millis(mgr.config.max_wait_ms);

        while !mgr.shutdown.load(Ordering::Relaxed) {
            // 1. 배치 수집 시도 (parking_lot Mutex는 빠르게 lock/unlock)
            let batch = {
                let mut pending = mgr.pending.lock();

                if mgr.should_flush(&pending) {
                    let batch: Vec<PendingCommit> = pending.drain(..).collect();
                    mgr.pending_bytes.store(0, Ordering::Relaxed);
                    batch
                } else if !pending.is_empty() {
                    let oldest_time = pending
                        .front()
                        .map(|p| p.enqueue_time)
                        .unwrap_or_else(Instant::now);

                    if oldest_time.elapsed() >= max_wait {
                        let batch: Vec<PendingCommit> = pending.drain(..).collect();
                        mgr.pending_bytes.store(0, Ordering::Relaxed);
                        batch
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            }; // lock dropped

            if !batch.is_empty() {
                // WAL 쓰기는 blocking I/O → spawn_blocking에서 수행
                let mgr_clone = Arc::clone(&mgr);
                let writer_clone = Arc::clone(&wal_writer);
                let _ = tokio::task::spawn_blocking(move || {
                    mgr_clone.flush_batch(&writer_clone, batch);
                })
                .await;
                continue;
            }

            // 2. 대기: Notify 알림 또는 타임아웃
            let wait_duration = {
                let pending = mgr.pending.lock();
                if pending.is_empty() {
                    None // 무한 대기
                } else {
                    let oldest = pending
                        .front()
                        .map(|p| p.enqueue_time)
                        .unwrap_or_else(Instant::now);
                    Some(max_wait.saturating_sub(oldest.elapsed()))
                }
            };

            match wait_duration {
                Some(d) if d.is_zero() => continue,
                Some(d) => {
                    let _ = tokio::time::timeout(d, mgr.notify.notified()).await;
                }
                None => {
                    mgr.notify.notified().await;
                }
            }
        }
    }

    /// 플러시 조건 확인
    fn should_flush(&self, pending: &VecDeque<PendingCommit>) -> bool {
        if pending.is_empty() {
            return false;
        }

        // 배치 크기 조건
        if pending.len() >= self.config.max_batch_size {
            return true;
        }

        // 바이트 조건
        if self.pending_bytes.load(Ordering::Relaxed) as usize >= self.config.max_batch_bytes {
            return true;
        }

        // 시간 조건
        if let Some(oldest) = pending.front()
            && oldest.enqueue_time.elapsed() >= Duration::from_millis(self.config.max_wait_ms)
        {
            return true;
        }

        false
    }

    /// 배치 플러시 수행
    fn flush_batch<W: WalWriter>(&self, wal_writer: &Arc<W>, batch: Vec<PendingCommit>) {
        let batch_size = batch.len();

        // 모든 레코드 수집
        let all_records: Vec<LogRecord> = batch
            .iter()
            .flat_map(|p| p.records.iter().cloned())
            .collect();

        // WAL 쓰기 및 fsync
        let (success, bytes_written, error_msg) = match wal_writer.write_records(&all_records) {
            Ok(bytes) => match wal_writer.sync() {
                Ok(()) => (true, bytes, None),
                Err(e) => (false, 0, Some(format!("{}", e))),
            },
            Err(e) => (false, 0, Some(format!("{}", e))),
        };

        // 통계 업데이트
        self.total_commits
            .fetch_add(batch_size as u64, Ordering::Relaxed);
        self.total_batches.fetch_add(1, Ordering::Relaxed);
        if success {
            self.total_fsyncs.fetch_add(1, Ordering::Relaxed);
            self.total_bytes_written
                .fetch_add(bytes_written as u64, Ordering::Relaxed);
        }

        // 대기 시간 통계
        let total_wait: u64 = batch
            .iter()
            .map(|p| p.enqueue_time.elapsed().as_micros() as u64)
            .sum();
        self.total_wait_time_us
            .fetch_add(total_wait, Ordering::Relaxed);

        // 결과 전송
        for pending in batch {
            let result = if success {
                Ok(())
            } else {
                Err(TransactionError::WalWriteFailed(
                    error_msg
                        .clone()
                        .unwrap_or_else(|| "Unknown error".to_string()),
                ))
            };
            let _ = pending.result_sender.send(result);
        }
    }

    /// 종료
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.notify.notify_waiters();

        if let Some(handle) = self.flush_task.lock().take() {
            handle.abort();
        }
    }
}

impl Drop for GroupCommitManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
