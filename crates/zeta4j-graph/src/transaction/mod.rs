//! # 트랜잭션 관리 모듈
//!
//! ACID 트랜잭션을 지원하는 트랜잭션 관리 시스템입니다.
//!
//! ## 주요 구성요소
//!
//! - [`TransactionManager`]: 트랜잭션 생명주기 관리
//! - [`LockManager`]: 2PL 기반 락 관리 및 데드락 탐지
//! - [`PageWAL`]: ARIES 기반 Page-level WAL (파일 I/O, LSN 관리 통합)
//! - [`MvccEngine`]: 다중 버전 동시성 제어
//! - [`TransactionOrchestrator`]: 고수준 트랜잭션 조율
//! - [`MetricsCollector`]: 트랜잭션 메트릭 수집
//! - [`GroupCommitManager`]: WAL 배치 쓰기 최적화 (Group Commit)
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::transaction::{TransactionManager, IsolationLevel, TransactionConfig};
//!
//! // 트랜잭션 매니저 생성
//! let manager = TransactionManager::new();
//!
//! // 트랜잭션 시작
//! let tx_id = manager.begin_transaction()?;
//!
//! // 작업 수행...
//!
//! // 커밋
//! manager.commit_transaction(tx_id)?;
//! ```
//!
//! ## 격리 수준
//!
//! | 수준 | Dirty Read | Non-Repeatable Read | Phantom Read |
//! |------|------------|---------------------|--------------|
//! | ReadCommitted | ❌ | ⭕ | ⭕ |
//! | RepeatableRead | ❌ | ❌ | ⭕ |
//! | Serializable | ❌ | ❌ | ❌ |
//!
//! ## 설정
//!
//! [`crate::config::TransactionSettings`]를 통해 다음을 설정할 수 있습니다:
//!
//! - `timeout_ms`: 트랜잭션 타임아웃 (기본: 5000ms)
//! - `isolation_level`: 격리 수준 (기본: ReadCommitted)
//! - `lock_wait_ms`: 락 대기 시간 (기본: 1000ms)
//! - `deadlock_check_ms`: 데드락 탐지 주기 (기본: 100ms)

pub mod batch;
pub mod diagnostics;
pub mod error;
pub mod group_commit;
pub mod lock;
pub mod lsn;
pub mod manager;
pub mod metrics;
pub mod mvcc;
pub mod orchestrator;
pub mod page_wal;
pub mod wal;

pub use batch::{
    BatchConfig, BatchStats, BatchTransaction, BatchWrite, BatchWriteFn, TransactionProvider,
};
pub use diagnostics::{
    DeadlockInfo, DiagnosticsDumper, FullDiagnosis, HealthStatus, LockChainAnalysis,
    LockChainAnalyzer, LockStats, LockWaitInfo, TransactionDiagnostics, TransactionDump,
    TransactionInfo, TransactionStats, WaiterInfo,
};
pub use error::{Result, TransactionError};
pub use group_commit::{
    FileWalWriter, GroupCommitConfig, GroupCommitManager, GroupCommitStats, WalWriter,
};
pub use lock::LockManager;
pub use lsn::{FlushedLSNTracker, INVALID_LSN, LSN, LSNGenerator};
pub use manager::{
    TransactionContext, TransactionManager, TransactionManagerStats, TransactionState,
};
pub use metrics::{MetricsCollector, TransactionMetrics};
pub use mvcc::{
    ConflictError, MvccConfig, MvccEngine, MvccStats, ReadSetItem, Snapshot, VersionInfo,
};
pub use orchestrator::{Transaction, TransactionOrchestrator};
pub use page_wal::{PageWAL, PageWALBuilder, WalRecord};
pub use wal::{LogRecord, PageStorageType, SlotUndoOp, WalConfig};

// use crate::core::NodeId; // unused warning resolved

/// 트랜잭션 ID 타입
pub type TransactionId = u64;

/// 트랜잭션 격리 수준
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IsolationLevel {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

/// 트랜잭션 설정
#[derive(Debug, Clone)]
pub struct TransactionConfig {
    pub isolation_level: IsolationLevel,
    pub timeout_ms: u64,
}

impl Default for TransactionConfig {
    fn default() -> Self {
        Self {
            isolation_level: IsolationLevel::ReadCommitted,
            timeout_ms: 5000,
        }
    }
}
