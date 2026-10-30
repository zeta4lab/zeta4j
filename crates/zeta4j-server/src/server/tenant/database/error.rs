//! 데이터베이스 관리 에러 타입.

use thiserror::Error;

/// 데이터베이스 관리 에러
#[derive(Debug, Error)]
pub enum TenantDbError {
    #[error("Database not found: {0}")]
    DatabaseNotFound(String),

    #[error("Database already exists: {0}")]
    DatabaseAlreadyExists(String),

    #[error("Cannot delete system database")]
    CannotDeleteSystem,

    #[error("Cannot delete default database")]
    CannotDeleteDefault,

    #[error("Database is in invalid state: {0}")]
    InvalidState(String),

    #[error("Invalid database name: {0}")]
    InvalidName(String),

    #[error("Maximum number of databases reached: {0}")]
    MaxDatabasesReached(usize),

    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),

    #[error("Tenant not found: {0}")]
    TenantNotFound(String),

    #[error("Operation not permitted: {0}")]
    NotPermitted(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Engine error: {0}")]
    EngineError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}
