// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Prelude module for convenient imports.
//!
//! This module re-exports the most commonly used types from the crate.
//!
//! # Example
//!
//! ```rust,ignore
//! use zeta4j_cypher_core::prelude::*;
//!
//! // Parse a query
//! let (_, query) = parse_query("MATCH (n:Person) RETURN n.name").unwrap();
//! ```

// =============================================================================
// Parser Functions
// =============================================================================

pub use crate::parser::{
    parse_query, parse_query_safe, parse_single_query, parse_single_query_safe,
};

// =============================================================================
// Planner Types
// =============================================================================

pub use crate::planner::QueryPlanner;

// =============================================================================
// AST Types (Essential)
// =============================================================================

pub use crate::ast::{
    EdgePattern, Expression, NodePattern, PlanNode, Query, QueryBody, QueryPlan, ReadingClause,
    ReturnClause, SingleQuery, Statement, UpdatingClause,
};

// =============================================================================
// Cache Types
// =============================================================================

pub use crate::cache::{CacheConfig, CacheStats, QueryCacheManager};

// =============================================================================
// Procedure Types
// =============================================================================

pub use crate::procedures::{Procedure, ProcedureRegistry};
