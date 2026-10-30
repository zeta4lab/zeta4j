#![allow(dead_code)]
//! 백그라운드 Flusher
//!
//! 비동기적으로 dirty 페이지를 디스크에 기록하여
//! 동기식 flush_all_pages() 블로킹 문제를 해결합니다.
//!
//! ## 문제점
//!
//! 기존 `flush_all_pages()`는 동기적으로 모든 dirty 페이지를
//! 디스크에 쓰는 동안 새 페이지 할당이 블로킹됩니다.
//! 대량 쓰기 시 버퍼 풀 병목을 유발합니다.
//!
//! ## 해결책
//!
//! `BackgroundFlusher`는 별도 스레드에서 주기적으로:
//! - dirty 페이지 비율 모니터링
//! - 임계값 초과 시 배치로 flush
//! - 메인 스레드와 비동기 동작

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Flusher 설정
#[derive(Debug, Clone)]
pub struct FlushConfig {
    /// dirty 페이지 비율 임계값 (0.0 ~ 1.0)
    /// 이 비율을 초과하면 flush 시작
    pub dirty_threshold: f64,
    /// 한 번에 flush할 최대 페이지 수
    pub batch_size: usize,
    /// flush 체크 주기 (밀리초)
    pub interval_ms: u64,
    /// 강제 flush 주기 (밀리초, 0이면 비활성화)
    /// dirty 비율에 관계없이 주기적으로 flush
    pub force_flush_interval_ms: u64,
}

impl Default for FlushConfig {
    fn default() -> Self {
        Self {
            dirty_threshold: 0.7, // 70%
            batch_size: 100,
            interval_ms: 100,               // 100ms
            force_flush_interval_ms: 30000, // 30초
        }
    }
}

/// Flusher 명령
enum FlushCommand {
    /// 즉시 flush 실행
    FlushNow,
    /// Flusher 종료
    Shutdown,
}

/// Flush 통계
#[derive(Debug, Clone, Default)]
pub struct FlushStats {
    /// 총 flush 횟수
    pub total_flushes: u64,
    /// 총 flush된 페이지 수
    pub total_pages_flushed: u64,
    /// 마지막 flush 시간 (시작 이후 경과 ms)
    pub last_flush_elapsed_ms: u64,
    /// 평균 flush 소요 시간 (ms)
    pub avg_flush_duration_ms: f64,
}

/// Dirty 페이지 정보 제공 트레이트
///
/// BufferPoolManager가 구현해야 합니다.
pub trait DirtyPageProvider: Send + Sync {
    /// 현재 dirty 페이지 비율 (0.0 ~ 1.0)
    fn dirty_ratio(&self) -> f64;

    /// dirty 페이지 ID 목록 (최대 limit개)
    fn get_dirty_pages(&self, limit: usize) -> Vec<u32>;

    /// 페이지를 디스크에 flush
    fn flush_page(&self, page_id: u32) -> Result<(), String>;
}

/// 백그라운드 Flusher
pub struct BackgroundFlusher {
    config: FlushConfig,
    command_tx: SyncSender<FlushCommand>,
    handle: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    stats: Arc<FlushStatsInner>,
    start_time: Instant,
}

struct FlushStatsInner {
    total_flushes: AtomicU64,
    total_pages_flushed: AtomicU64,
    last_flush_elapsed_ms: AtomicU64,
    total_flush_duration_ms: AtomicU64,
}

impl BackgroundFlusher {
    /// 새 BackgroundFlusher 생성 및 시작
    pub fn new<P>(provider: Arc<P>, config: FlushConfig) -> std::io::Result<Self>
    where
        P: DirtyPageProvider + 'static,
    {
        let (command_tx, command_rx) = mpsc::sync_channel::<FlushCommand>(16);
        let running = Arc::new(AtomicBool::new(true));
        let stats = Arc::new(FlushStatsInner {
            total_flushes: AtomicU64::new(0),
            total_pages_flushed: AtomicU64::new(0),
            last_flush_elapsed_ms: AtomicU64::new(0),
            total_flush_duration_ms: AtomicU64::new(0),
        });
        let start_time = Instant::now();

        let handle = {
            let running = Arc::clone(&running);
            let stats = Arc::clone(&stats);
            let config = config.clone();

            thread::Builder::new()
                .name("background-flusher".to_string())
                .spawn(move || {
                    Self::flush_loop(provider, config, command_rx, running, stats, start_time);
                })?
        };

        Ok(Self {
            config,
            command_tx,
            handle: Some(handle),
            running,
            stats,
            start_time,
        })
    }

    /// 기본 설정으로 생성
    pub fn with_default<P>(provider: Arc<P>) -> std::io::Result<Self>
    where
        P: DirtyPageProvider + 'static,
    {
        Self::new(provider, FlushConfig::default())
    }

    /// 즉시 flush 요청
    pub fn flush_now(&self) {
        let _ = self.command_tx.try_send(FlushCommand::FlushNow);
    }

    /// Flusher 종료
    pub fn shutdown(&mut self) {
        if self.running.swap(false, Ordering::SeqCst) {
            let _ = self.command_tx.send(FlushCommand::Shutdown);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    /// 실행 중인지 확인
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// 통계 조회
    pub fn stats(&self) -> FlushStats {
        let total_flushes = self.stats.total_flushes.load(Ordering::Relaxed);
        let total_pages = self.stats.total_pages_flushed.load(Ordering::Relaxed);
        let total_duration = self.stats.total_flush_duration_ms.load(Ordering::Relaxed);

        FlushStats {
            total_flushes,
            total_pages_flushed: total_pages,
            last_flush_elapsed_ms: self.stats.last_flush_elapsed_ms.load(Ordering::Relaxed),
            avg_flush_duration_ms: if total_flushes > 0 {
                total_duration as f64 / total_flushes as f64
            } else {
                0.0
            },
        }
    }

    /// Flush 루프 (별도 스레드에서 실행)
    fn flush_loop<P>(
        provider: Arc<P>,
        config: FlushConfig,
        command_rx: Receiver<FlushCommand>,
        running: Arc<AtomicBool>,
        stats: Arc<FlushStatsInner>,
        start_time: Instant,
    ) where
        P: DirtyPageProvider + 'static,
    {
        let interval = Duration::from_millis(config.interval_ms);
        let mut last_force_flush = Instant::now();

        while running.load(Ordering::Relaxed) {
            // 명령 체크 (논블로킹)
            match command_rx.recv_timeout(interval) {
                Ok(FlushCommand::FlushNow) => {
                    Self::do_flush(&provider, &config, &stats, start_time);
                    last_force_flush = Instant::now();
                }
                Ok(FlushCommand::Shutdown) => {
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {
                    // 타임아웃 - 조건 체크
                }
                Err(RecvTimeoutError::Disconnected) => {
                    break;
                }
            }

            // dirty 비율 체크
            let dirty_ratio = provider.dirty_ratio();
            let should_flush = dirty_ratio >= config.dirty_threshold;

            // 강제 flush 주기 체크
            let force_flush_due = config.force_flush_interval_ms > 0
                && last_force_flush.elapsed()
                    >= Duration::from_millis(config.force_flush_interval_ms);

            if should_flush || force_flush_due {
                Self::do_flush(&provider, &config, &stats, start_time);
                last_force_flush = Instant::now();
            }
        }

        // 종료 전 마지막 flush
        Self::do_flush(&provider, &config, &stats, start_time);
    }

    /// 실제 flush 수행
    fn do_flush<P>(
        provider: &Arc<P>,
        config: &FlushConfig,
        stats: &Arc<FlushStatsInner>,
        start_time: Instant,
    ) where
        P: DirtyPageProvider,
    {
        let flush_start = Instant::now();

        // dirty 페이지 목록 가져오기
        let dirty_pages = provider.get_dirty_pages(config.batch_size);
        if dirty_pages.is_empty() {
            return;
        }

        let mut flushed_count = 0u64;

        for page_id in dirty_pages {
            match provider.flush_page(page_id) {
                Ok(()) => {
                    flushed_count += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to flush page {}: {}", page_id, e);
                }
            }
        }

        // 통계 업데이트
        let duration_ms = flush_start.elapsed().as_millis() as u64;
        stats.total_flushes.fetch_add(1, Ordering::Relaxed);
        stats
            .total_pages_flushed
            .fetch_add(flushed_count, Ordering::Relaxed);
        stats
            .last_flush_elapsed_ms
            .store(start_time.elapsed().as_millis() as u64, Ordering::Relaxed);
        stats
            .total_flush_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);

        tracing::debug!(
            "Background flush: {} pages in {}ms",
            flushed_count,
            duration_ms
        );
    }
}

impl Drop for BackgroundFlusher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// BufferPoolManager용 DirtyPageProvider 어댑터
///
/// BufferPoolManager를 DirtyPageProvider 트레이트로 래핑합니다.
pub struct BufferPoolAdapter<B> {
    buffer_pool: Arc<B>,
}

impl<B> BufferPoolAdapter<B> {
    pub fn new(buffer_pool: Arc<B>) -> Self {
        Self { buffer_pool }
    }
}
