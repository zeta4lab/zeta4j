// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # 트랜잭션 메트릭 수집
//!
//! 트랜잭션 시스템의 성능 및 상태 메트릭을 수집하고 분석합니다.
//!
//! ## 주요 기능
//!
//! - **처리량 메트릭**: TPS (초당 트랜잭션), 커밋/롤백 비율
//! - **지연 시간 메트릭**: 트랜잭션 평균/최대 지속 시간
//! - **락 메트릭**: 락 대기 시간, 데드락 발생 빈도
//! - **WAL 메트릭**: 쓰기 처리량, 동기화 지연
//! - **MVCC 메트릭**: 스냅샷 수, GC 효율성
//!
//! ## 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::transaction::metrics::{TransactionMetrics, MetricsCollector};
//!
//! let collector = MetricsCollector::new();
//!
//! // 트랜잭션 시작/종료 기록
//! collector.record_begin();
//! // ... 작업 수행 ...
//! collector.record_commit(elapsed_ms);
//!
//! // 메트릭 조회
//! let metrics = collector.snapshot();
//! println!("TPS: {:.2}", metrics.transactions_per_second());
//! ```

use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// 트랜잭션 메트릭 스냅샷
///
/// 특정 시점의 트랜잭션 시스템 상태를 나타냅니다.
#[derive(Debug, Clone)]
pub struct TransactionMetrics {
    /// 스냅샷 생성 시간
    pub timestamp: Instant,
    /// 수집 시작 이후 경과 시간 (초)
    pub uptime_secs: f64,

    // ===== 처리량 메트릭 =====
    /// 총 시작된 트랜잭션 수
    pub total_begun: u64,
    /// 총 커밋된 트랜잭션 수
    pub total_committed: u64,
    /// 총 롤백된 트랜잭션 수
    pub total_aborted: u64,
    /// 총 타임아웃된 트랜잭션 수
    pub total_timed_out: u64,

    // ===== 지연 시간 메트릭 (밀리초) =====
    /// 평균 트랜잭션 지속 시간
    pub avg_duration_ms: f64,
    /// 최대 트랜잭션 지속 시간
    pub max_duration_ms: u64,
    /// 최소 트랜잭션 지속 시간
    pub min_duration_ms: u64,
    /// 총 트랜잭션 지속 시간 합계
    pub total_duration_ms: u64,

    // ===== 락 메트릭 =====
    /// 총 락 획득 시도 횟수
    pub total_lock_acquisitions: u64,
    /// 락 획득 실패 (타임아웃) 횟수
    pub total_lock_timeouts: u64,
    /// 데드락 발생 횟수
    pub total_deadlocks: u64,
    /// 평균 락 대기 시간 (밀리초)
    pub avg_lock_wait_ms: f64,
    /// 최대 락 대기 시간 (밀리초)
    pub max_lock_wait_ms: u64,

    // ===== WAL 메트릭 =====
    /// 총 WAL 레코드 수
    pub total_wal_records: u64,
    /// 총 WAL 바이트 수
    pub total_wal_bytes: u64,
    /// WAL 동기화 횟수
    pub total_wal_syncs: u64,
    /// 평균 WAL 동기화 시간 (밀리초)
    pub avg_wal_sync_ms: f64,

    // ===== MVCC 메트릭 =====
    /// 현재 활성 스냅샷 수
    pub active_snapshots: u64,
    /// 총 생성된 스냅샷 수
    pub total_snapshots_created: u64,
    /// Write-Write 충돌 횟수
    pub total_write_conflicts: u64,
    /// SSI 직렬화 실패 횟수
    pub total_serialization_failures: u64,
}

impl TransactionMetrics {
    /// 초당 트랜잭션 처리량 (TPS)
    pub fn transactions_per_second(&self) -> f64 {
        if self.uptime_secs > 0.0 {
            self.total_committed as f64 / self.uptime_secs
        } else {
            0.0
        }
    }

    /// 커밋 성공률 (%)
    pub fn commit_rate(&self) -> f64 {
        let total = self.total_committed + self.total_aborted;
        if total > 0 {
            (self.total_committed as f64 / total as f64) * 100.0
        } else {
            100.0
        }
    }

    /// 락 성공률 (%)
    pub fn lock_success_rate(&self) -> f64 {
        let total = self.total_lock_acquisitions;
        if total > 0 {
            let failed = self.total_lock_timeouts + self.total_deadlocks;
            ((total - failed) as f64 / total as f64) * 100.0
        } else {
            100.0
        }
    }

    /// WAL 쓰기 처리량 (bytes/sec)
    pub fn wal_throughput(&self) -> f64 {
        if self.uptime_secs > 0.0 {
            self.total_wal_bytes as f64 / self.uptime_secs
        } else {
            0.0
        }
    }
}

impl Default for TransactionMetrics {
    fn default() -> Self {
        Self {
            timestamp: Instant::now(),
            uptime_secs: 0.0,
            total_begun: 0,
            total_committed: 0,
            total_aborted: 0,
            total_timed_out: 0,
            avg_duration_ms: 0.0,
            max_duration_ms: 0,
            min_duration_ms: u64::MAX,
            total_duration_ms: 0,
            total_lock_acquisitions: 0,
            total_lock_timeouts: 0,
            total_deadlocks: 0,
            avg_lock_wait_ms: 0.0,
            max_lock_wait_ms: 0,
            total_wal_records: 0,
            total_wal_bytes: 0,
            total_wal_syncs: 0,
            avg_wal_sync_ms: 0.0,
            active_snapshots: 0,
            total_snapshots_created: 0,
            total_write_conflicts: 0,
            total_serialization_failures: 0,
        }
    }
}

/// 메트릭 수집기
///
/// 트랜잭션 시스템의 모든 메트릭을 수집합니다.
///
/// # 스레드 안전성
///
/// 모든 카운터는 원자적으로 업데이트되며, 여러 스레드에서
/// 안전하게 사용할 수 있습니다.
pub struct MetricsCollector {
    /// 수집 시작 시간
    start_time: Instant,

    // ===== 처리량 카운터 =====
    total_begun: AtomicU64,
    total_committed: AtomicU64,
    total_aborted: AtomicU64,
    total_timed_out: AtomicU64,

    // ===== 지연 시간 카운터 =====
    total_duration_ms: AtomicU64,
    max_duration_ms: AtomicU64,
    min_duration_ms: AtomicU64,
    duration_count: AtomicU64,

    // ===== 락 카운터 =====
    total_lock_acquisitions: AtomicU64,
    total_lock_timeouts: AtomicU64,
    total_deadlocks: AtomicU64,
    total_lock_wait_ms: AtomicU64,
    max_lock_wait_ms: AtomicU64,
    lock_wait_count: AtomicU64,

    // ===== WAL 카운터 =====
    total_wal_records: AtomicU64,
    total_wal_bytes: AtomicU64,
    total_wal_syncs: AtomicU64,
    total_wal_sync_ms: AtomicU64,

    // ===== MVCC 카운터 =====
    active_snapshots: AtomicU64,
    total_snapshots_created: AtomicU64,
    total_write_conflicts: AtomicU64,
    total_serialization_failures: AtomicU64,

    /// 히스토리 (최근 N개 스냅샷 유지)
    history: RwLock<Vec<TransactionMetrics>>,
    history_max_size: usize,
}

impl MetricsCollector {
    /// 새 메트릭 수집기 생성
    pub fn new() -> Self {
        Self::with_history_size(60) // 기본 60개 (1분간 1초 간격)
    }

    /// 히스토리 크기를 지정하여 생성
    pub fn with_history_size(max_size: usize) -> Self {
        Self {
            start_time: Instant::now(),
            total_begun: AtomicU64::new(0),
            total_committed: AtomicU64::new(0),
            total_aborted: AtomicU64::new(0),
            total_timed_out: AtomicU64::new(0),
            total_duration_ms: AtomicU64::new(0),
            max_duration_ms: AtomicU64::new(0),
            min_duration_ms: AtomicU64::new(u64::MAX),
            duration_count: AtomicU64::new(0),
            total_lock_acquisitions: AtomicU64::new(0),
            total_lock_timeouts: AtomicU64::new(0),
            total_deadlocks: AtomicU64::new(0),
            total_lock_wait_ms: AtomicU64::new(0),
            max_lock_wait_ms: AtomicU64::new(0),
            lock_wait_count: AtomicU64::new(0),
            total_wal_records: AtomicU64::new(0),
            total_wal_bytes: AtomicU64::new(0),
            total_wal_syncs: AtomicU64::new(0),
            total_wal_sync_ms: AtomicU64::new(0),
            active_snapshots: AtomicU64::new(0),
            total_snapshots_created: AtomicU64::new(0),
            total_write_conflicts: AtomicU64::new(0),
            total_serialization_failures: AtomicU64::new(0),
            history: RwLock::new(Vec::with_capacity(max_size)),
            history_max_size: max_size,
        }
    }

    // ===== 트랜잭션 이벤트 기록 =====

    /// 트랜잭션 시작 기록
    pub fn record_begin(&self) {
        self.total_begun.fetch_add(1, Ordering::Relaxed);
    }

    /// 트랜잭션 커밋 기록
    pub fn record_commit(&self, duration_ms: u64) {
        self.total_committed.fetch_add(1, Ordering::Relaxed);
        self.record_duration(duration_ms);
    }

    /// 트랜잭션 롤백 기록
    pub fn record_abort(&self, duration_ms: u64) {
        self.total_aborted.fetch_add(1, Ordering::Relaxed);
        self.record_duration(duration_ms);
    }

    /// 트랜잭션 타임아웃 기록
    pub fn record_timeout(&self) {
        self.total_timed_out.fetch_add(1, Ordering::Relaxed);
    }

    /// 트랜잭션 지속 시간 기록
    fn record_duration(&self, duration_ms: u64) {
        self.total_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        self.duration_count.fetch_add(1, Ordering::Relaxed);

        // 최대값 업데이트 (CAS 루프)
        loop {
            let current = self.max_duration_ms.load(Ordering::Relaxed);
            if duration_ms <= current {
                break;
            }
            if self
                .max_duration_ms
                .compare_exchange_weak(current, duration_ms, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }

        // 최소값 업데이트 (CAS 루프)
        loop {
            let current = self.min_duration_ms.load(Ordering::Relaxed);
            if duration_ms >= current {
                break;
            }
            if self
                .min_duration_ms
                .compare_exchange_weak(current, duration_ms, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    // ===== 락 이벤트 기록 =====

    /// 락 획득 성공 기록
    pub fn record_lock_acquired(&self, wait_ms: u64) {
        self.total_lock_acquisitions.fetch_add(1, Ordering::Relaxed);
        self.total_lock_wait_ms
            .fetch_add(wait_ms, Ordering::Relaxed);
        self.lock_wait_count.fetch_add(1, Ordering::Relaxed);

        // 최대 대기 시간 업데이트
        loop {
            let current = self.max_lock_wait_ms.load(Ordering::Relaxed);
            if wait_ms <= current {
                break;
            }
            if self
                .max_lock_wait_ms
                .compare_exchange_weak(current, wait_ms, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    /// 락 타임아웃 기록
    pub fn record_lock_timeout(&self) {
        self.total_lock_acquisitions.fetch_add(1, Ordering::Relaxed);
        self.total_lock_timeouts.fetch_add(1, Ordering::Relaxed);
    }

    /// 데드락 기록
    pub fn record_deadlock(&self) {
        self.total_deadlocks.fetch_add(1, Ordering::Relaxed);
    }

    // ===== WAL 이벤트 기록 =====

    /// WAL 레코드 쓰기 기록
    pub fn record_wal_write(&self, bytes: u64) {
        self.total_wal_records.fetch_add(1, Ordering::Relaxed);
        self.total_wal_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// WAL 동기화 기록
    pub fn record_wal_sync(&self, duration_ms: u64) {
        self.total_wal_syncs.fetch_add(1, Ordering::Relaxed);
        self.total_wal_sync_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
    }

    // ===== MVCC 이벤트 기록 =====

    /// 스냅샷 생성 기록
    pub fn record_snapshot_created(&self) {
        self.active_snapshots.fetch_add(1, Ordering::Relaxed);
        self.total_snapshots_created.fetch_add(1, Ordering::Relaxed);
    }

    /// 스냅샷 해제 기록
    pub fn record_snapshot_released(&self) {
        self.active_snapshots.fetch_sub(1, Ordering::Relaxed);
    }

    /// Write-Write 충돌 기록
    pub fn record_write_conflict(&self) {
        self.total_write_conflicts.fetch_add(1, Ordering::Relaxed);
    }

    /// SSI 직렬화 실패 기록
    pub fn record_serialization_failure(&self) {
        self.total_serialization_failures
            .fetch_add(1, Ordering::Relaxed);
    }

    // ===== 스냅샷 조회 =====

    /// 현재 메트릭 스냅샷 생성
    pub fn snapshot(&self) -> TransactionMetrics {
        let uptime = self.start_time.elapsed();
        let duration_count = self.duration_count.load(Ordering::Relaxed);
        let lock_wait_count = self.lock_wait_count.load(Ordering::Relaxed);
        let wal_syncs = self.total_wal_syncs.load(Ordering::Relaxed);

        let min_duration = self.min_duration_ms.load(Ordering::Relaxed);

        TransactionMetrics {
            timestamp: Instant::now(),
            uptime_secs: uptime.as_secs_f64(),
            total_begun: self.total_begun.load(Ordering::Relaxed),
            total_committed: self.total_committed.load(Ordering::Relaxed),
            total_aborted: self.total_aborted.load(Ordering::Relaxed),
            total_timed_out: self.total_timed_out.load(Ordering::Relaxed),
            avg_duration_ms: if duration_count > 0 {
                self.total_duration_ms.load(Ordering::Relaxed) as f64 / duration_count as f64
            } else {
                0.0
            },
            max_duration_ms: self.max_duration_ms.load(Ordering::Relaxed),
            min_duration_ms: if min_duration == u64::MAX {
                0
            } else {
                min_duration
            },
            total_duration_ms: self.total_duration_ms.load(Ordering::Relaxed),
            total_lock_acquisitions: self.total_lock_acquisitions.load(Ordering::Relaxed),
            total_lock_timeouts: self.total_lock_timeouts.load(Ordering::Relaxed),
            total_deadlocks: self.total_deadlocks.load(Ordering::Relaxed),
            avg_lock_wait_ms: if lock_wait_count > 0 {
                self.total_lock_wait_ms.load(Ordering::Relaxed) as f64 / lock_wait_count as f64
            } else {
                0.0
            },
            max_lock_wait_ms: self.max_lock_wait_ms.load(Ordering::Relaxed),
            total_wal_records: self.total_wal_records.load(Ordering::Relaxed),
            total_wal_bytes: self.total_wal_bytes.load(Ordering::Relaxed),
            total_wal_syncs: wal_syncs,
            avg_wal_sync_ms: if wal_syncs > 0 {
                self.total_wal_sync_ms.load(Ordering::Relaxed) as f64 / wal_syncs as f64
            } else {
                0.0
            },
            active_snapshots: self.active_snapshots.load(Ordering::Relaxed),
            total_snapshots_created: self.total_snapshots_created.load(Ordering::Relaxed),
            total_write_conflicts: self.total_write_conflicts.load(Ordering::Relaxed),
            total_serialization_failures: self.total_serialization_failures.load(Ordering::Relaxed),
        }
    }

    /// 스냅샷을 히스토리에 저장
    pub fn record_snapshot(&self) {
        let snapshot = self.snapshot();
        let mut history = self.history.write();

        if history.len() >= self.history_max_size {
            history.remove(0);
        }
        history.push(snapshot);
    }

    /// 히스토리 조회
    pub fn history(&self) -> Vec<TransactionMetrics> {
        self.history.read().clone()
    }

    /// 최근 N개 히스토리 조회
    pub fn recent_history(&self, count: usize) -> Vec<TransactionMetrics> {
        let history = self.history.read();
        let start = history.len().saturating_sub(count);
        history[start..].to_vec()
    }

    /// 메트릭 리셋
    pub fn reset(&self) {
        self.total_begun.store(0, Ordering::Relaxed);
        self.total_committed.store(0, Ordering::Relaxed);
        self.total_aborted.store(0, Ordering::Relaxed);
        self.total_timed_out.store(0, Ordering::Relaxed);
        self.total_duration_ms.store(0, Ordering::Relaxed);
        self.max_duration_ms.store(0, Ordering::Relaxed);
        self.min_duration_ms.store(u64::MAX, Ordering::Relaxed);
        self.duration_count.store(0, Ordering::Relaxed);
        self.total_lock_acquisitions.store(0, Ordering::Relaxed);
        self.total_lock_timeouts.store(0, Ordering::Relaxed);
        self.total_deadlocks.store(0, Ordering::Relaxed);
        self.total_lock_wait_ms.store(0, Ordering::Relaxed);
        self.max_lock_wait_ms.store(0, Ordering::Relaxed);
        self.lock_wait_count.store(0, Ordering::Relaxed);
        self.total_wal_records.store(0, Ordering::Relaxed);
        self.total_wal_bytes.store(0, Ordering::Relaxed);
        self.total_wal_syncs.store(0, Ordering::Relaxed);
        self.total_wal_sync_ms.store(0, Ordering::Relaxed);
        // active_snapshots는 리셋하지 않음 (현재 상태 유지)
        self.total_snapshots_created.store(0, Ordering::Relaxed);
        self.total_write_conflicts.store(0, Ordering::Relaxed);
        self.total_serialization_failures
            .store(0, Ordering::Relaxed);
        self.history.write().clear();
    }

    /// 경과 시간 (초)
    pub fn uptime_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// 메트릭 요약 포맷터
impl std::fmt::Display for TransactionMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== Transaction Metrics ===")?;
        writeln!(f, "Uptime: {:.1}s", self.uptime_secs)?;
        writeln!(f)?;

        writeln!(f, "[Throughput]")?;
        writeln!(f, "  TPS: {:.2}", self.transactions_per_second())?;
        writeln!(f, "  Begun: {}", self.total_begun)?;
        writeln!(
            f,
            "  Committed: {} ({:.1}%)",
            self.total_committed,
            self.commit_rate()
        )?;
        writeln!(f, "  Aborted: {}", self.total_aborted)?;
        writeln!(f, "  Timed out: {}", self.total_timed_out)?;
        writeln!(f)?;

        writeln!(f, "[Latency]")?;
        writeln!(f, "  Avg: {:.2}ms", self.avg_duration_ms)?;
        writeln!(f, "  Max: {}ms", self.max_duration_ms)?;
        writeln!(f, "  Min: {}ms", self.min_duration_ms)?;
        writeln!(f)?;

        writeln!(f, "[Locking]")?;
        writeln!(
            f,
            "  Acquisitions: {} ({:.1}% success)",
            self.total_lock_acquisitions,
            self.lock_success_rate()
        )?;
        writeln!(f, "  Timeouts: {}", self.total_lock_timeouts)?;
        writeln!(f, "  Deadlocks: {}", self.total_deadlocks)?;
        writeln!(f, "  Avg wait: {:.2}ms", self.avg_lock_wait_ms)?;
        writeln!(f)?;

        writeln!(f, "[WAL]")?;
        writeln!(f, "  Records: {}", self.total_wal_records)?;
        writeln!(
            f,
            "  Bytes: {} ({:.1} KB/s)",
            self.total_wal_bytes,
            self.wal_throughput() / 1024.0
        )?;
        writeln!(
            f,
            "  Syncs: {} (avg {:.2}ms)",
            self.total_wal_syncs, self.avg_wal_sync_ms
        )?;
        writeln!(f)?;

        writeln!(f, "[MVCC]")?;
        writeln!(f, "  Active snapshots: {}", self.active_snapshots)?;
        writeln!(f, "  Total snapshots: {}", self.total_snapshots_created)?;
        writeln!(f, "  Write conflicts: {}", self.total_write_conflicts)?;
        writeln!(
            f,
            "  Serialization failures: {}",
            self.total_serialization_failures
        )?;

        Ok(())
    }
}
