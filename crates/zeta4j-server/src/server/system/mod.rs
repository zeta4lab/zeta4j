// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! System Database Module
//!
//! 시스템 메타데이터 영속화를 위한 모듈
//!
//! ## 구성요소
//!
//! - **SystemStorage**: RocksDB 기반 영속 스토리지
//! - **CatalogManager**: 데이터베이스 카탈로그 관리
//! - **SecurityManager**: 인증/권한 관리
//! - **SchemaManager**: 인덱스/제약조건 레지스트리
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::server::system::SystemDatabase;
//!
//! // 시스템 DB 열기
//! let system_db = SystemDatabase::open("~/.zeta4j/system").await?;
//!
//! // 데이터베이스 카탈로그 조회
//! let databases = system_db.catalog().list().await?;
//!
//! // 사용자 인증
//! let user = system_db.security().authenticate("admin", "password").await?;
//! ```

pub mod catalog;
pub mod credential;
pub mod database;
pub mod error;
pub mod schema;
pub mod security;
pub mod storage;

#[cfg(test)]
pub(crate) fn owner_only_tempdir() -> std::io::Result<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(directory)
}

// Re-exports
pub use catalog::{
    CatalogDatabaseState, CatalogManager, CatalogStorageType, DatabaseEntry, DatabaseLimits,
    DatabaseOptions,
};
pub use credential::{
    ApiKeyDescriptor, AuthenticatedApiKey, AuthenticatedPasswordSession, CredentialClock,
    CredentialRandom, CredentialRuntimePolicy, CredentialSigner, CredentialStore,
    CredentialThrottleState, IssuedApiKey, IssuedTokenPair, LocalCredentialStore,
    LocalSecurityService, SecurityMutation, SecurityMutationOutcome, ServicePrincipal,
    SessionDescriptor, SystemCredentialClock, SystemCredentialRandom,
    initialize_local_credential_runtime, maintain_local_signing_keys,
};
pub use database::{SharedSystemDatabaseService, SystemDatabaseService, SystemSummary};
pub use error::{SystemError, SystemResult};
pub use schema::{
    ConstraintDef, ConstraintType, DatabaseSchema, EntityType, IndexConfig, IndexDef, IndexState,
    IndexType, SchemaManager, Similarity, ValueType,
};
pub use security::{SecurityManager, User};
pub use storage::{
    SharedSystemStorage, SystemStorage, SystemWriteBatch, SystemWriteTransaction, cf,
};
