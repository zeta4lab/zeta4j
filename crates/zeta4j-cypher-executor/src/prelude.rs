// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Prelude module for convenient imports.
//!
//! This module re-exports the most commonly used types from the crate.
//!
//! # Example
//!
//! ```rust,ignore
//! use zeta4j_cypher_executor::prelude::*;
//!
//! // Parse and execute a query
//! let (_, query) = parse_query("MATCH (n:Person) RETURN n.name").unwrap();
//! let result = executor.execute(query).await.unwrap();
//! ```

// =============================================================================
// Parser Functions
// =============================================================================

pub use zeta4j_cypher_core::parser::{
    parse_query, parse_query_safe, parse_single_query, parse_single_query_safe,
};

// =============================================================================
// Executor Types
// =============================================================================

pub use crate::executor::{ExecutionContext, QueryHints};
pub use crate::executor::{
    ExecutionStats, MaterializedRows, MutationStats, QueryExecutor, QueryResult,
};

// =============================================================================
// Planner Types
// =============================================================================

pub use zeta4j_cypher_core::planner::QueryPlanner;

// =============================================================================
// AST Types (Essential)
// =============================================================================

pub use zeta4j_cypher_core::ast::{
    EdgePattern, Expression, NodePattern, PlanNode, Query, QueryBody, QueryPlan, ReadingClause,
    ReturnClause, SingleQuery, Statement, UpdatingClause,
};

// =============================================================================
// Service Traits
// =============================================================================

pub use zeta4j_cypher_core::services::{
    AdminCapability, AdminService, AdminServiceError, DatabaseInfo, UserInfo,
};
