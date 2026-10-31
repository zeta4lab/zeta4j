// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # 트랜잭션 에러 타입
//!
//! 트랜잭션 처리 중 발생할 수 있는 모든 에러를 세분화하여 정의합니다.
//!
//! ## 에러 카테고리
//!
//! | 카테고리 | 에러 타입 | 설명 |
//! |----------|-----------|------|
//! | 상태 | `NotFound`, `AlreadyCommitted`, `AlreadyAborted` | 트랜잭션 상태 관련 |
//! | 락 | `LockFailed`, `LockTimeout`, `DeadlockDetected` | 동시성 제어 관련 |
//! | WAL | `WalCorrupted`, `WalWriteFailed`, `WalSyncFailed` | 내구성 관련 |
//! | MVCC | `WriteConflict`, `VersionNotFound`, `MvccConflict` | 버전 관리 관련 |
//! | 시스템 | `IoError`, `SystemError` | 시스템/I/O 관련 |
//!
//! ## 재시도 가능 여부
//!
//! [`TransactionError::is_retryable`]로 재시도 가능한 에러인지 확인할 수 있습니다:
//!
//! - ✅ 재시도 가능: `LockTimeout`, `DeadlockDetected`, `WriteConflict`, `MvccConflict`
//! - ❌ 재시도 불가: `NotFound`, `AlreadyCommitted`, `WalCorrupted` 등
//!
//! ## 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::transaction::{Result, TransactionError};
//!
//! fn perform_operation() -> Result<()> {
//!     // ... 트랜잭션 작업 ...
//!     Err(TransactionError::LockTimeout(1000))
//! }
//!
//! match perform_operation() {
//!     Ok(_) => println!("성공"),
//!     Err(e) if e.is_retryable() => println!("재시도 가능: {}", e),
//!     Err(e) => println!("실패: {}", e),
//! }
//! ```

use thiserror::Error;
use zeta4j_common::{ZetaError, codes};

/// 트랜잭션 처리 중 발생하는 에러
///
/// 모든 트랜잭션 관련 에러를 포괄하는 열거형입니다.
///
/// # 유틸리티 메서드
///
/// - [`is_retryable`](Self::is_retryable): 재시도 가능 여부
/// - [`requires_abort`](Self::requires_abort): 트랜잭션 중단 필요 여부
///
/// # 에러 생성 헬퍼
///
/// ```rust,ignore
/// // 데드락 에러
/// let err = TransactionError::deadlock(vec![1, 2, 3]);
///
/// // 락 타임아웃 에러
/// let err = TransactionError::lock_timeout_with_duration(5000);
///
/// // WAL 손상 에러
/// let err = TransactionError::wal_corrupted(1024, "checksum mismatch");
/// ```
#[derive(Error, Debug)]
pub enum TransactionError {
    // ==================== 트랜잭션 상태 에러 ====================
    /// 트랜잭션을 찾을 수 없음
    #[error("Transaction not found: {0}")]
    NotFound(u64),

    /// 이미 커밋된 트랜잭션
    #[error("Transaction already committed: {0}")]
    AlreadyCommitted(u64),

    /// 이미 중단된 트랜잭션
    #[error("Transaction already aborted: {0}")]
    AlreadyAborted(u64),

    /// 잘못된 트랜잭션 상태
    #[error("Invalid transaction state: {tx_id} is in {state} state, expected {expected}")]
    InvalidState {
        tx_id: u64,
        state: String,
        expected: String,
    },

    /// 잘못된 인자
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    // ==================== 락 관련 에러 ====================
    /// 일반적인 락 획득 실패
    #[error("Lock acquisition failed: {0}")]
    LockFailed(String),

    /// 락 타임아웃
    #[error("Lock timeout after {0}ms")]
    LockTimeout(u64),

    /// 락 업그레이드 실패 (공유 → 배타적)
    #[error("Lock upgrade failed: {reason}")]
    LockUpgradeFailed { reason: String },

    /// Mutex/RwLock poisoned
    #[error("Lock poisoned: {lock_name}")]
    LockPoisoned { lock_name: String },

    /// 데드락 감지
    #[error("Deadlock detected involving transactions: {0:?}")]
    DeadlockDetected(Vec<u64>),

    // ==================== WAL 관련 에러 ====================
    /// WAL 로그 손상
    #[error("WAL corrupted at position {position}: {details}")]
    WalCorrupted { position: u64, details: String },

    /// WAL 쓰기 실패
    #[error("WAL write failed: {0}")]
    WalWriteFailed(String),

    /// WAL 동기화 실패
    #[error("WAL sync failed: {0}")]
    WalSyncFailed(String),

    // ==================== MVCC 관련 에러 ====================
    /// 쓰기 충돌 (다른 트랜잭션이 먼저 수정)
    #[error("Write conflict on key {key}: modified by transaction {conflicting_tx}")]
    WriteConflict { key: String, conflicting_tx: u64 },

    /// 버전을 찾을 수 없음
    #[error("Version not found for key {key} at timestamp {timestamp}")]
    VersionNotFound { key: String, timestamp: u64 },

    /// MVCC 충돌 (Serializable 직렬화 실패 포함)
    #[error("MVCC conflict: {0}")]
    MvccConflict(#[from] super::mvcc::ConflictError),

    // ==================== 직렬화 에러 ====================
    /// 직렬화 실패
    #[error("Serialization failed: {0}")]
    SerializationError(String),

    /// 역직렬화 실패
    #[error("Deserialization failed: {0}")]
    DeserializationError(String),

    // ==================== I/O 및 시스템 에러 ====================
    /// I/O 에러
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// 내부 시스템 에러 (catch-all)
    #[error("Internal system error: {0}")]
    SystemError(String),
}

impl TransactionError {
    /// 데드락 에러 생성 헬퍼
    pub fn deadlock(cycle: Vec<u64>) -> Self {
        TransactionError::DeadlockDetected(cycle)
    }

    /// 락 타임아웃 에러 생성 (기본 0ms)
    pub fn lock_timeout() -> Self {
        TransactionError::LockTimeout(0)
    }

    /// 락 타임아웃 에러 생성 (시간 지정)
    pub fn lock_timeout_with_duration(ms: u64) -> Self {
        TransactionError::LockTimeout(ms)
    }

    /// 락 poisoned 에러 생성 헬퍼
    pub fn lock_poisoned(name: &str) -> Self {
        TransactionError::LockPoisoned {
            lock_name: name.to_string(),
        }
    }

    /// WAL 손상 에러 생성 헬퍼
    pub fn wal_corrupted(position: u64, details: &str) -> Self {
        TransactionError::WalCorrupted {
            position,
            details: details.to_string(),
        }
    }

    /// 잘못된 상태 에러 생성 헬퍼
    pub fn invalid_state(tx_id: u64, state: &str, expected: &str) -> Self {
        TransactionError::InvalidState {
            tx_id,
            state: state.to_string(),
            expected: expected.to_string(),
        }
    }

    /// 에러가 재시도 가능한지 여부
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            TransactionError::LockTimeout(_)
                | TransactionError::DeadlockDetected(_)
                | TransactionError::WriteConflict { .. }
                | TransactionError::MvccConflict(_)
        )
    }

    /// 에러가 트랜잭션 중단이 필요한지 여부
    pub fn requires_abort(&self) -> bool {
        matches!(
            self,
            TransactionError::DeadlockDetected(_)
                | TransactionError::WriteConflict { .. }
                | TransactionError::WalCorrupted { .. }
                | TransactionError::MvccConflict(_)
        )
    }
}

pub type Result<T> = std::result::Result<T, TransactionError>;

// =============================================================================
// From<TransactionError> for ZetaError
// =============================================================================

impl From<TransactionError> for ZetaError {
    fn from(err: TransactionError) -> Self {
        match err {
            // 단순 빌더 메서드 사용
            TransactionError::NotFound(tx_id) => ZetaError::transaction_not_found(tx_id),
            TransactionError::InvalidArgument(msg) => ZetaError::invalid_argument(msg),
            TransactionError::LockFailed(msg) => ZetaError::lock_failed(msg),
            TransactionError::LockTimeout(ms) => ZetaError::lock_timeout("unknown", ms),
            TransactionError::DeadlockDetected(cycle) => ZetaError::deadlock(cycle),
            TransactionError::WriteConflict {
                key,
                conflicting_tx,
            } => ZetaError::write_conflict(key, conflicting_tx),
            TransactionError::WalCorrupted { position, details } => {
                ZetaError::wal_corrupted(position, details)
            }
            TransactionError::SerializationError(msg) => ZetaError::serialization(msg),
            TransactionError::IoError(err) => ZetaError::storage_io(err.to_string()),
            TransactionError::SystemError(msg) => ZetaError::internal(msg),

            // 트랜잭션 상태 에러 (코드 지정)
            TransactionError::AlreadyCommitted(tx_id) => ZetaError::transaction_with_code(
                codes::transaction::TX_ALREADY_COMMITTED,
                format!("Transaction {} already committed", tx_id),
            ),
            TransactionError::AlreadyAborted(tx_id) => ZetaError::transaction_with_code(
                codes::transaction::TX_ALREADY_ABORTED,
                format!("Transaction {} already aborted", tx_id),
            ),
            TransactionError::InvalidState {
                tx_id,
                state,
                expected,
            } => ZetaError::transaction_with_code(
                codes::transaction::TX_INVALID_STATE,
                format!(
                    "Transaction {} is in {} state, expected {}",
                    tx_id, state, expected
                ),
            ),
            TransactionError::MvccConflict(conflict_err) => ZetaError::transaction_with_code(
                codes::transaction::MVCC_CONFLICT,
                conflict_err.to_string(),
            ),

            // 락 에러 (코드 지정)
            TransactionError::LockUpgradeFailed { reason } => ZetaError::lock_failed_with_code(
                codes::transaction::LOCK_UPGRADE_FAILED,
                format!("Lock upgrade failed: {}", reason),
            ),
            TransactionError::LockPoisoned { lock_name } => ZetaError::lock_failed_with_code(
                codes::transaction::LOCK_POISONED,
                format!("Lock poisoned: {}", lock_name),
            ),

            // WAL 에러 (코드 지정)
            TransactionError::WalWriteFailed(msg) => ZetaError::storage_io_with_code(
                codes::transaction::WAL_WRITE_FAILED,
                format!("WAL write failed: {}", msg),
            ),
            TransactionError::WalSyncFailed(msg) => ZetaError::storage_io_with_code(
                codes::transaction::WAL_SYNC_FAILED,
                format!("WAL sync failed: {}", msg),
            ),

            // 기타
            TransactionError::VersionNotFound { key, timestamp } => ZetaError::not_found(format!(
                "Version for key {} at timestamp {}",
                key, timestamp
            )),
            TransactionError::DeserializationError(msg) => {
                ZetaError::serialization_with_code(codes::graph::DESERIALIZATION_FAILED, msg)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_and_deadlock_retry_and_abort_contracts_differ() {
        let timeout = TransactionError::LockTimeout(250);
        assert!(timeout.is_retryable());
        assert!(!timeout.requires_abort());

        let deadlock = TransactionError::DeadlockDetected(vec![1, 2, 1]);
        assert!(deadlock.is_retryable());
        assert!(deadlock.requires_abort());
    }
}
