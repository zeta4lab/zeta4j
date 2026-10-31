// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # Zeta4j Cypher Core
//!
//! Cypher 쿼리 언어 파서 및 코어 컴포넌트
//!
//! 이 크레이트는 Cypher 쿼리 처리의 공통 기능을 제공합니다:
//! - AST (Abstract Syntax Tree) 정의
//! - Cypher 쿼리 파서
//! - 쿼리 플래너
//! - 쿼리 옵티마이저
//! - 내장 함수
//! - 쿼리 캐시
//! - 커서/페이지네이션
//! - 프로시저 레지스트리
//! - 외부 서비스 트레이트
//!
//! ## 크레이트 구조
//!
//! ```text
//! zeta4j-cypher-core (파서, AST, 공통 로직)
//!     ↑
//! zeta4j-cypher-executor (GraphEngine + QueryExecutor)
//!     ↑
//! zeta4j-server (HTTP/Bolt/gRPC)
//! ```
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use zeta4j_cypher_core::prelude::*;
//!
//! // 쿼리 파싱
//! let (_, query) = parse_query("MATCH (n:Person) RETURN n.name").unwrap();
//! ```
//!
//! ## API Tiers
//!
//! | Tier | Access | Description |
//! |------|--------|-------------|
//! | **prelude** | `use zeta4j_cypher_core::prelude::*` | Core types for everyday use |
//! | **root** | `use zeta4j_cypher_core::*` | Commonly used types |
//! | **module** | `use zeta4j_cypher_core::ast::*` | AST types and advanced features |

// =============================================================================
// Module Structure
// =============================================================================

pub mod ast;
pub mod backend;
pub mod cache;
pub mod cursor;
pub mod functions;
pub mod hint;
pub mod optimizer;
pub mod parser;
pub mod planner;
pub mod prelude;
pub mod procedures;
pub mod services;

// =============================================================================
// Public API - Parser Functions
// =============================================================================

pub use parser::{parse_query, parse_query_safe, parse_single_query, parse_single_query_safe};

// =============================================================================
// Public API - Planner Types
// =============================================================================

pub use planner::QueryPlanner;

// =============================================================================
// Public API - Optimizer Types
// =============================================================================

pub use optimizer::{CostModel, StatisticsManager};

// =============================================================================
// Public API - Hint Types
// =============================================================================

pub use hint::{HintApplier, HintManager, HintParser, HintValidator, IndexInfo, QueryHint};

// =============================================================================
// Public API - Cache Types
// =============================================================================

pub use cache::{CacheConfig, CacheStats, InvalidationEvent, QueryCacheManager};

// =============================================================================
// Public API - Cursor Types
// =============================================================================

pub use cursor::{CursorManager, FetchResult, PageRequest, PagedResult, PaginationConfig};

// =============================================================================
// Public API - Procedure Types
// =============================================================================

pub use procedures::{Procedure, ProcedureError, ProcedureRegistry, ProcedureResult};

// =============================================================================
// Public API - Service Traits
// =============================================================================

pub use services::{
    AdminCapability, AdminService, AdminServiceError, AdminServiceResult, DatabaseInfo, UserInfo,
};

// =============================================================================
// Public API - Backend Trait
// =============================================================================

pub use backend::{CypherBackend, DynCypherBackend};

// =============================================================================
// Re-export AST Types at Root Level
// =============================================================================

pub use ast::{
    AdminCommand, AggregateFunction, ArithmeticOp, CallSubquery, ComparisonOp, ConstraintCommand,
    DatabaseTarget, EdgeDirection, EdgePattern, Expression, IndexCommand, LetClause, LogicalOp,
    NodePattern, OrderItem, PlanNode, PlanNodeType, ProcedureCall, Query, QueryBody, QueryPlan,
    ReadingClause, ReturnClause, ReturnItem, ScalarFunction, SchemaCommand, SingleQuery, Statement,
    UnwindClause, UpdatingClause, UseClause, WithClause,
};
