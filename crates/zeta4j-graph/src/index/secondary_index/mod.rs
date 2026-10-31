// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Secondary Index - 레이블+프로퍼티 기반 보조 인덱스
//!
//! Neo4j 스타일의 프로퍼티 인덱스를 제공합니다.
//!
//! ## 단일 프로퍼티 인덱스
//! ```cypher
//! CREATE INDEX FOR (n:Label) ON (n.property)
//! ```
//!
//! ## 복합 프로퍼티 인덱스 (v1.79.0)
//! ```cypher
//! CREATE INDEX FOR (n:Person) ON (n.firstName, n.lastName)
//! ```

mod composite_index;
mod manager;
mod property_index;
mod value;

// Re-exports
pub use composite_index::{MultiPropertyIndex, MultiPropertyKey};
pub use manager::{MultiPropertyIndexEntry, SecondaryIndexEntry, SecondaryIndexManager};
pub use property_index::PropertyIndex;
pub use value::{IndexValue, normalize_array, normalize_map, normalize_value};
