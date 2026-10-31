// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 그래프 엔진 - 통합 그래프 연산 관리자
//!
//! ## 모듈 구조
//!
//! - `config`: 엔진 설정 및 통계 타입
//! - `poolable`: 메모리 풀 Poolable 트레이트 구현
//! - `core`: GraphEngine 핵심 구조체
//! - `transaction`: 트랜잭션 관리 메서드
//! - `crud`: CRUD 연산
//! - `index_ops`: Secondary Index 연산

mod canonical;
pub mod config;
mod core;
mod crud;
pub mod graph_view;
mod index_ops;
pub mod page_recovery;
mod poolable;
mod transaction;

// Public exports
pub use canonical::{
    CanonicalGraphStore, GraphIndexDefinition, GraphIndexKind, InMemoryCanonicalGraphStore,
};
pub use config::{EngineStats, GraphEngineConfig, StorageType};
pub use core::{EntityType, GraphEngine, VersionKey};
pub use graph_view::GraphView;
pub use page_recovery::{
    DirtyPageEntry, PageRecoveryManager, RecoveryStats, TxStatus, TxTableEntry,
};
pub use transaction::{GraphTransactionDelta, PreparedGraphCommit};
