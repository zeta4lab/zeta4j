// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Multi-Tenancy Module (M6.1 ~ M6.4)
//!
//! 다중 데이터베이스 및 테넌트 격리 시스템
//!
//! ## Features
//! - M6.1: 다중 데이터베이스 관리
//! - M6.2: 리소스 격리
//! - M6.3: 네임스페이스
//! - M6.4: 데이터베이스 수명주기

mod database;
mod lifecycle;
mod namespace;
mod resource;

pub use database::{
    AccessMode, ActiveVectorGeneration, DatabaseContext, DatabaseContextError,
    DatabaseFulltextRepository, DatabaseGraphRepository, DatabaseId, DatabaseMetadata,
    DatabaseSnapshot, DatabaseState, DatabaseStatistics, DatabaseStatisticsSnapshot,
    DatabaseVectorRepository, GraphRepositoryError, GraphWriteSet, HnswGenerationNode,
    StagedVectorGeneration, TenantDatabase, TenantDatabaseConfig, TenantDatabaseConfigBuilder,
    TenantDatabaseManager, TenantDatabaseManagerConfig, TenantDatabaseSnapshot, TenantDbError,
    TransactionCoordinator, UnifiedWriteSet, VectorDistance, VectorEntityBinding, VectorEntityRef,
    VectorGcReport, VectorGenerationWriteSet, VectorRecord, VectorRepositoryError,
};

pub use resource::{
    ResourceAction, ResourceEnforcer, ResourceLimits, ResourceLimitsBuilder, ResourceMonitor,
    ResourceType, ResourceUsage,
};

pub use namespace::{
    IsolationLevel, Namespace, NamespaceManager, Tenant, TenantConfig, TenantConfigBuilder,
    TenantError, TenantId, TenantStatus,
};

pub use lifecycle::{
    DatabaseLifecycle, LifecycleError, MigrationError, SchemaMigration, SchemaVersion,
};
