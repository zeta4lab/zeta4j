//! Zeta4j Core Prelude
//!
//! 자주 사용되는 타입들을 한 번에 import할 수 있습니다.
//!
//! # 사용 예제
//!
//! ```rust,ignore
//! use zeta4j_graph::prelude::*;
//!
//! // 이제 Value, Node, Edge 등을 바로 사용 가능
//! let value = Value::Integer(42);
//! ```

// =========================================================================
// Core Types
// =========================================================================

pub use crate::core::{
    Edge, EdgeId, Node, NodeId, Property, PropertyContainer, Value, VectorValue,
};

// =========================================================================
// Engine Types
// =========================================================================

pub use crate::engine::{EngineStats, GraphEngine, GraphEngineConfig, GraphView, StorageType};

// =========================================================================
// Error Types
// =========================================================================

pub use crate::error::{GraphError, Result};

// =========================================================================
// Transaction Types
// =========================================================================

pub use crate::transaction::{
    IsolationLevel, TransactionConfig, TransactionId, TransactionManager,
};

// =========================================================================
// Schema Types
// =========================================================================

pub use crate::schema::{
    NodeLabelSchema, PropertyDefinition, PropertyType, RelationshipTypeSchema, SchemaManager,
};

// =========================================================================
// Config Types
// =========================================================================

pub use crate::config::Zeta4jConfig;

// =========================================================================
// Common Types (from zeta4j-common)
// =========================================================================

pub use zeta4j_common::prelude::*;
