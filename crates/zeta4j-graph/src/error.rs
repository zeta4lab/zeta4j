//! 에러 타입 정의
//!
//! `GraphError`는 Zeta4j 그래프 데이터베이스의 모든 에러를 포괄합니다.
//!
//! ## 에러 카테고리
//!
//! | 카테고리 | 에러 타입 | 설명 |
//! |----------|-----------|------|
//! | 엔티티 | `NodeNotFound`, `EdgeNotFound` | 노드/엣지 조회 실패 |
//! | 직렬화 | `Serialization` | Postcard, JSON 등 직렬화 실패 |
//! | 트랜잭션 | `Transaction` | 트랜잭션 처리 에러 (소스 체이닝) |
//! | 제약조건 | `Constraint` | 스키마 제약조건 위반 |
//! | 동시성 | `Lock`, `Concurrency` | 락/동시성 제어 관련 |
//! | 스토리지 | `Storage`, `Index` | 저장소/인덱스 에러 |
//! | 파싱/검증 | `Parse`, `Validation` | 쿼리 파싱/데이터 검증 |
//! | 시스템 | `Io`, `Json`, `System` | 시스템/I/O 에러 |
//!
//! ## 유틸리티 메서드
//!
//! - [`is_retryable`](GraphError::is_retryable): 재시도 가능 여부
//! - [`requires_rollback`](GraphError::requires_rollback): 롤백 필요 여부
//! - [`is_client_error`](GraphError::is_client_error): 클라이언트 에러 여부 (4xx류)
//!
//! ## 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::error::{GraphError, Result};
//!
//! fn process() -> Result<()> {
//!     // 트랜잭션 에러는 자동 변환
//!     let tx_result = transaction_op()?;
//!
//!     // 재시도 가능한 에러 처리
//!     match some_operation() {
//!         Err(e) if e.is_retryable() => retry(),
//!         Err(e) => return Err(e),
//!         Ok(v) => v,
//!     }
//! }
//! ```

use thiserror::Error;
use zeta4j_common::{ZetaError, codes};

/// Zeta4j 그래프 데이터베이스 에러 타입
///
/// 모든 그래프 작업에서 발생할 수 있는 에러를 포괄합니다.
#[derive(Error, Debug)]
pub enum GraphError {
    // ==================== 엔티티 관련 ====================
    /// 노드를 찾을 수 없음
    #[error("Node not found: {id}")]
    NodeNotFound { id: u64 },

    /// 엣지를 찾을 수 없음
    #[error("Edge not found: {id}")]
    EdgeNotFound { id: u64 },

    // ==================== 직렬화 관련 ====================
    /// 직렬화/역직렬화 에러 (Postcard, serde 등)
    #[error("Serialization error: {message}")]
    Serialization {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // ==================== 트랜잭션 관련 ====================
    /// 트랜잭션 에러 (TransactionError에서 자동 변환)
    #[error("Transaction error: {0}")]
    Transaction(#[from] crate::transaction::TransactionError),

    // ==================== 제약조건 관련 ====================
    /// 제약조건 위반
    #[error("Constraint violation: {constraint_name} - {message}")]
    Constraint {
        constraint_name: String,
        message: String,
    },

    // ==================== 동시성 관련 ====================
    /// 락 에러
    #[error("Lock error: {resource} - {message}")]
    Lock { resource: String, message: String },

    /// 동시성 에러
    #[error("Concurrency error: {message}")]
    Concurrency { message: String },

    // ==================== 스토리지/인덱스 ====================
    /// 스토리지 에러
    #[error("Storage error: {message}")]
    Storage { message: String },

    /// 인덱스 에러
    #[error("Index error: {message}")]
    Index { message: String },

    // ==================== 파싱/검증 ====================
    /// 파서 에러
    #[error("Parse error: {message}")]
    Parse { message: String },

    /// 유효성 검사 실패
    #[error("Validation error: {message}")]
    Validation { message: String },

    // ==================== 스키마/온톨로지 ====================
    /// 스키마 에러
    #[error("Schema error: {0}")]
    Schema(String),

    /// 온톨로지 에러

    // ==================== 보안/인증 ====================
    /// 접근 거부 (인증 실패, 권한 부족)
    #[error("Access denied: {message}")]
    AccessDenied { message: String },

    // ==================== 시스템 ====================
    /// I/O 에러 (자동 변환)
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    /// JSON 에러 (자동 변환)
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// 버퍼 풀 가득 참
    #[error("Buffer pool exhausted")]
    BufferPoolExhausted,

    /// Database-scoped memory admission was denied.
    #[error("{0}")]
    Memory(#[from] crate::storage::memory::MemoryLimitError),

    /// 미구현 기능
    #[error("Not implemented: {feature}")]
    NotImplemented { feature: String },

    /// 시스템 에러 (일반)
    #[error("System error: {message}")]
    System { message: String },

    /// 통합 에러 (ZetaError 래핑)
    ///
    /// 다른 크레이트에서 이미 ZetaError로 변환된 에러를 GraphError 컨텍스트에서
    /// 사용해야 할 때 이 변형을 사용합니다. From<GraphError> for ZetaError에서
    /// 언래핑되어 원본 에러 코드가 보존됩니다.
    #[error("{0}")]
    Zeta(ZetaError),
}

// ==================== 유틸리티 메서드 ====================

impl GraphError {
    // === 카테고리 판별 ===

    /// 재시도 가능한 에러인지 판별
    ///
    /// 락 타임아웃, 데드락, 쓰기 충돌 등은 재시도로 해결 가능
    pub fn is_retryable(&self) -> bool {
        match self {
            GraphError::Transaction(tx_err) => tx_err.is_retryable(),
            GraphError::Lock { .. } => true,
            GraphError::Concurrency { .. } => true,
            GraphError::BufferPoolExhausted => true,
            GraphError::Memory(crate::storage::memory::MemoryLimitError::AllocationDenied {
                ..
            }) => true,
            _ => false,
        }
    }

    /// 트랜잭션 롤백이 필요한지 판별
    pub fn requires_rollback(&self) -> bool {
        match self {
            GraphError::Transaction(tx_err) => tx_err.requires_abort(),
            GraphError::Constraint { .. } => true,
            GraphError::Validation { .. } => true,
            GraphError::Memory(_) => true,
            _ => false,
        }
    }

    /// 클라이언트 에러인지 (HTTP 4xx류)
    ///
    /// 클라이언트가 잘못된 요청을 보낸 경우
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            GraphError::NodeNotFound { .. }
                | GraphError::EdgeNotFound { .. }
                | GraphError::Parse { .. }
                | GraphError::Validation { .. }
                | GraphError::Constraint { .. }
                | GraphError::AccessDenied { .. }
                | GraphError::NotImplemented { .. }
        )
    }

    /// 서버 에러인지 (HTTP 5xx류)
    pub fn is_server_error(&self) -> bool {
        !self.is_client_error()
    }

    // === 생성 헬퍼 ===

    /// 직렬화 에러 생성 (소스 포함)
    pub fn serialization<E: std::error::Error + Send + Sync + 'static>(
        message: impl Into<String>,
        source: E,
    ) -> Self {
        GraphError::Serialization {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// 직렬화 에러 생성 (메시지만)
    pub fn serialization_msg(message: impl Into<String>) -> Self {
        GraphError::Serialization {
            message: message.into(),
            source: None,
        }
    }

    /// 스토리지 에러 생성
    pub fn storage(message: impl Into<String>) -> Self {
        GraphError::Storage {
            message: message.into(),
        }
    }

    /// 인덱스 에러 생성
    pub fn index(message: impl Into<String>) -> Self {
        GraphError::Index {
            message: message.into(),
        }
    }

    /// 락 에러 생성
    pub fn lock(resource: impl Into<String>, message: impl Into<String>) -> Self {
        GraphError::Lock {
            resource: resource.into(),
            message: message.into(),
        }
    }

    /// 제약조건 위반 에러 생성
    pub fn constraint(name: impl Into<String>, message: impl Into<String>) -> Self {
        GraphError::Constraint {
            constraint_name: name.into(),
            message: message.into(),
        }
    }

    /// 파싱 에러 생성
    pub fn parse(message: impl Into<String>) -> Self {
        GraphError::Parse {
            message: message.into(),
        }
    }

    /// 유효성 검사 에러 생성
    pub fn validation(message: impl Into<String>) -> Self {
        GraphError::Validation {
            message: message.into(),
        }
    }
}

/// Result 타입 별칭
pub type Result<T> = std::result::Result<T, GraphError>;

// =============================================================================
// From<GraphError> for ZetaError
// =============================================================================

impl From<GraphError> for ZetaError {
    fn from(err: GraphError) -> Self {
        match err {
            // 단순 필드 매핑
            GraphError::NodeNotFound { id } => ZetaError::node_not_found(id),
            GraphError::EdgeNotFound { id } => ZetaError::edge_not_found(id),
            GraphError::Serialization { message, .. } => ZetaError::serialization(message),
            GraphError::Storage { message } => ZetaError::storage_io(message),
            GraphError::Index { message } => ZetaError::index(message),
            GraphError::Parse { message } => ZetaError::query_parse(message),
            GraphError::Validation { message } => ZetaError::invalid_argument(message),
            GraphError::Schema(msg) => ZetaError::invalid_argument(msg),
            GraphError::AccessDenied { message } => ZetaError::permission_denied(message),
            GraphError::NotImplemented { feature } => ZetaError::not_implemented(feature),
            GraphError::System { message } => ZetaError::internal(message),
            GraphError::BufferPoolExhausted => ZetaError::storage_capacity("Buffer pool exhausted"),
            GraphError::Memory(
                error @ crate::storage::memory::MemoryLimitError::AllocationDenied { .. },
            ) => ZetaError::memory_admission(error.to_string()),
            GraphError::Memory(error) => ZetaError::internal(error.to_string()),
            GraphError::Io { source } => ZetaError::storage_io(source.to_string()),
            GraphError::Json(err) => ZetaError::serialization(err.to_string()),

            // 제약조건 (두 필드)
            GraphError::Constraint {
                constraint_name,
                message,
            } => ZetaError::constraint(constraint_name, message),

            // 락/동시성 (코드 지정 필요)
            GraphError::Lock { resource, message } => {
                ZetaError::lock_failed(format!("{}: {}", resource, message))
            }
            GraphError::Concurrency { message } => {
                ZetaError::transaction_with_code(codes::transaction::MVCC_CONFLICT, message)
            }

            // 내포된 에러 변환
            GraphError::Transaction(tx_err) => tx_err.into(),
            GraphError::Zeta(zeta_err) => zeta_err,
        }
    }
}

#[cfg(test)]
mod memory_error_tests {
    use super::*;
    use crate::storage::memory::{MemoryCategory, MemoryLimitError};

    fn denied(category: MemoryCategory) -> GraphError {
        GraphError::Memory(MemoryLimitError::AllocationDenied {
            category,
            requested: 2,
            available: 1,
            limit: 1,
        })
    }

    #[test]
    fn admission_and_internal_reservation_errors_have_distinct_contracts() {
        let overlay = denied(MemoryCategory::TransactionOverlay);
        assert!(overlay.is_retryable());
        assert!(overlay.requires_rollback());
        let overlay: ZetaError = overlay.into();
        assert_eq!(overlay.code(), &codes::graph::MEMORY_ADMISSION_DENIED);

        let query = denied(MemoryCategory::Query);
        assert!(query.is_retryable());
        assert!(query.requires_rollback());

        let invariant = GraphError::Memory(MemoryLimitError::IncompatibleReservation);
        assert!(!invariant.is_retryable());
        assert!(invariant.requires_rollback());
        let invariant: ZetaError = invariant.into();
        assert_eq!(invariant.code(), &codes::graph::SYSTEM_ERROR);
    }
}
