// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # Zeta4j Cypher Executor
//!
//! GraphEngine 기반 Cypher 쿼리 실행기
//!
//! 이 크레이트는 서버가 사용하는 Cypher 실행기를 제공합니다.
//! 쿼리는 GraphEngine 트랜잭션에서 실행됩니다.
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use zeta4j_cypher_executor::prelude::*;
//! use std::sync::Arc;
//!
//! // GraphEngine으로 QueryExecutor 생성
//! let executor = QueryExecutor::new(Arc::clone(&engine));
//!
//! // 쿼리 파싱 및 실행
//! let (_, query) = parse_query("MATCH (n:Person) RETURN n.name").unwrap();
//! let result = executor.execute(query).await.unwrap();
//! ```

pub mod backend;
pub mod executor;
pub mod prelude;

// Re-export core types from cypher-core
pub use zeta4j_cypher_core::{
    AdminCapability,
    AdminCommand,
    AdminService,
    AdminServiceError,
    AdminServiceResult,
    AggregateFunction,
    ArithmeticOp,
    // Cache
    CacheConfig,
    CacheStats,
    CallSubquery,
    ComparisonOp,
    ConstraintCommand,
    // Optimizer
    CostModel,
    // Cursor
    CursorManager,
    DatabaseInfo,
    DatabaseTarget,
    EdgeDirection,
    EdgePattern,
    // Services
    Expression,
    FetchResult,
    // Hint
    HintApplier,
    HintManager,
    HintParser,
    HintValidator,
    IndexCommand,
    IndexInfo,
    InvalidationEvent,
    LetClause,
    LogicalOp,
    NodePattern,
    OrderItem,
    PageRequest,
    PagedResult,
    PaginationConfig,
    PlanNode,
    PlanNodeType,
    // Procedure
    Procedure,
    ProcedureCall,
    ProcedureError,
    ProcedureRegistry,
    ProcedureResult,
    // AST
    Query,
    QueryBody,
    QueryCacheManager,
    QueryHint,
    QueryPlan,
    // Planner
    QueryPlanner,
    ReadingClause,
    ReturnClause,
    ReturnItem,
    ScalarFunction,
    SchemaCommand,
    SingleQuery,
    Statement,
    StatisticsManager,
    UnwindClause,
    UpdatingClause,
    UseClause,
    UserInfo,
    WithClause,
    // Parser
    parse_query,
    parse_query_safe,
    parse_single_query,
    parse_single_query_safe,
};

// Re-export executor types
pub use executor::{
    AnalyzeFormatter, ExecutionContext, ExecutionStats, MaterializedRows, MutationStats,
    OperatorStats, QueryExecutor, QueryHints, QueryResult,
};

// Re-export modules from cypher-core for direct access
pub use zeta4j_cypher_core::{
    CypherBackend, DynCypherBackend, ast, backend as core_backend, cache, cursor, functions, hint,
    optimizer, parser, planner, procedures, services,
};

// Re-export backend implementation
pub use backend::GraphEngineBackend;
