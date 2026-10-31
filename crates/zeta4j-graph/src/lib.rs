// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # Zeta4j Graph
//!
//! 그래프 엔진 라이브러리
//!
//! 이 크레이트는 다음 기능을 포함합니다:
//! - 그래프 데이터 구조 (Node, Edge, Property)
//! - 스토리지 엔진 (메모리, 디스크, RocksDB)
//! - 인덱스 관리 (B+Tree)
//! - 그래프 알고리즘 (경로 탐색, 중심성 등)
//! - 트랜잭션 관리 (MVCC, WAL)
//! - 스키마 관리
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use zeta4j_graph::prelude::*;
//!
//! // 메모리 기반 그래프 엔진 생성
//! let engine = GraphEngine::with_memory().unwrap();
//!
//! // 노드 생성
//! let node_id = engine.create_node(vec!["Person".to_string()]).unwrap();
//! engine.update_node(node_id, |n| {
//!     n.set_property("name", Value::String("Alice".to_string()));
//! }).unwrap();
//! ```
//!
//! ## API Tiers
//!
//! | Tier | Access | Description |
//! |------|--------|-------------|
//! | **prelude** | `use zeta4j_graph::prelude::*` | Core types for everyday use |
//! | **root** | `use zeta4j_graph::*` | Commonly used types |
//! | **module** | `use zeta4j_graph::algorithm::*` | Advanced features |
//!
//! ## Module Structure
//!
//! ### Core Layer
//! - [`core`] - 그래프 기본 타입 (Node, Edge, Value)
//! - [`engine`] - 그래프 엔진 (CRUD, 쿼리)
//! - [`error`] - 에러 타입
//!
//! ### Storage Layer
//! - [`storage`] - 스토리지 백엔드 (메모리, 디스크)
//! - [`index`] - 인덱스 (B+Tree)
//! - [`transaction`] - 트랜잭션 관리 (MVCC, WAL)
//!
//! ### Advanced Features
//! - [`algorithm`] - 그래프 알고리즘 (경로, 중심성, MST 등)
//! - [`schema`] - 스키마 관리
//! - [`ml`] - 머신러닝 관련 기능

// =============================================================================
// Module Structure
// =============================================================================

pub mod algorithm;
pub mod config;
pub mod core;
pub mod engine;
pub mod error;
pub mod index;
pub mod ml;
pub mod prelude;
pub mod schema;
pub mod storage;
pub mod transaction;

// =============================================================================
// External Crate Re-export
// =============================================================================

/// Re-export zeta4j-common for convenience
pub use zeta4j_common as common;

// =============================================================================
// Public API - Error Types
// =============================================================================

pub use error::{GraphError, Result};

// =============================================================================
// Public API - Core Types
// =============================================================================

pub use core::{Edge, EdgeId, Node, NodeId, Property, Value, VectorValue};

// =============================================================================
// Public API - Engine Types
// =============================================================================

pub use engine::{EngineStats, GraphEngine, GraphEngineConfig, StorageType};

// =============================================================================
// Public API - Config Types
// =============================================================================

pub use config::{ConfigError, Zeta4jConfig};

// =============================================================================
// Public API - Transaction Types
// =============================================================================

pub use transaction::{IsolationLevel, TransactionConfig, TransactionId, TransactionManager};

// =============================================================================
// Public API - Schema Types
// =============================================================================

pub use schema::{
    NodeLabelSchema, PropertyDefinition, PropertyType, RelationshipTypeSchema, SchemaManager,
};

// =============================================================================
// Advanced API - Access via module paths
// =============================================================================
//
// For advanced features, use module paths:
//
// Storage internals:
//   use zeta4j_graph::storage::cache::BufferPoolManager;
//   use zeta4j_graph::storage::disk::{DiskManager, SlottedPage, PAGE_SIZE};
//   use zeta4j_graph::storage::util::StorageConfig;
//
// Transaction internals:
//   use zeta4j_graph::transaction::{PageWAL, PageStorageType};
//
// Algorithms:
//   use zeta4j_graph::algorithm::path::{Dijkstra, BellmanFord};
//   use zeta4j_graph::algorithm::centrality::{PageRank, BetweennessCentrality};
//   use zeta4j_graph::algorithm::mst::Kruskal;
//   use zeta4j_graph::algorithm::scc::tarjan_scc;
//   use zeta4j_graph::algorithm::walk::RandomWalk;
