//! Tenant Database Module (M6.1)
//!
//! 다중 데이터베이스 관리
//!
//! 동시성 안전:
//! - 상태 전환 락을 사용하여 TOCTOU 취약점 방지
//! - CAS(Compare-And-Swap) 패턴으로 상태 전환 직렬화
//!
//! # 모듈 구조
//!
//! - `id`: 데이터베이스 ID 타입
//! - `state`: 데이터베이스 상태 타입
//! - `statistics`: 데이터베이스 통계 타입
//! - `metadata`: 데이터베이스 메타데이터 타입
//! - `config`: 데이터베이스 설정 타입
//! - `database`: TenantDatabase 구현
//! - `error`: 에러 타입
//! - `manager`: TenantDatabaseManager 구현

mod config;
mod context;
#[allow(clippy::module_inception)]
mod database;
mod error;
mod fulltext;
mod graph;
mod id;
mod manager;
mod metadata;
mod state;
mod statistics;
mod vector;

// Re-exports
pub use config::{AccessMode, TenantDatabaseConfig, TenantDatabaseConfigBuilder};
pub use context::{
    DatabaseContext, DatabaseContextError, DatabaseSnapshot, TransactionCoordinator,
    UnifiedWriteSet,
};
pub use database::TenantDatabase;
pub use error::TenantDbError;
pub use fulltext::DatabaseFulltextRepository;
pub use graph::{DatabaseGraphRepository, GraphRepositoryError, GraphWriteSet};
pub use id::DatabaseId;
pub use manager::TenantDatabaseSnapshot;
pub use manager::{TenantDatabaseManager, TenantDatabaseManagerConfig};
pub use metadata::DatabaseMetadata;
pub use state::DatabaseState;
pub use statistics::{DatabaseStatistics, DatabaseStatisticsSnapshot};
pub use vector::{
    ActiveVectorGeneration, DatabaseVectorRepository, HnswGenerationNode, StagedVectorGeneration,
    VectorDistance, VectorEntityBinding, VectorEntityRef, VectorGcReport, VectorGenerationWriteSet,
    VectorRecord, VectorRepositoryError,
};
