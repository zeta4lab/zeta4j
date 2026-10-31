// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Cypher 쿼리 AST (Abstract Syntax Tree) 모듈
//!
//! 이 모듈은 Cypher 쿼리를 표현하는 AST 구조체들을 정의합니다.
//! Parser가 Cypher 문자열을 파싱하여 이 AST로 변환하고,
//! Executor가 이 AST를 순회하며 쿼리를 실행합니다.
//!
//! # 주요 구조
//!
//! - [`Query`]: 최상위 쿼리 구조 (EXPLAIN 포함)
//! - [`QueryBody`]: 쿼리 본체 (Single 또는 UNION)
//! - [`Statement`]: 개별 실행 단위 (MATCH, CREATE, RETURN 등)
//! - [`Expression`] - 표현식 (리터럴, 변수, 함수 호출 등)
//!
//! # 모듈 구조
//!
//! - [`clause`] - 절 (Reading, Updating, Projection)
//! - [`command`] - 명령 (Index, Schema, Constraint, Admin)
//! - [`expr`] - 표현식 (Expression, Operator, Function)
//! - [`pattern`] - 패턴 (Node, Edge, Pattern)
//! - [`plan`] - 실행 계획 (PlanNode, QueryPlan)
//! - [`query`] - 쿼리 (Query, Statement)
//!
//! # 예제
//!
//! ```ignore
//! // 파서를 통해 AST 생성
//! use crate::parser::parse_query;
//!
//! let query = parse_query("MATCH (n:Person) RETURN n.name").unwrap();
//! assert!(!query.explain);
//! ```
//!
//! # 구조 다이어그램
//!
//! ```text
//! Query
//! ├── explain: bool
//! └── body: QueryBody
//!     ├── Single(SingleQuery)
//!     │   └── statements: Vec<Statement>
//!     │       ├── Reading(ReadingClause)
//!     │       ├── Updating(UpdatingClause)
//!     │       ├── With(WithClause)
//!     │       ├── Return(ReturnClause)
//!     │       └── ...
//!     └── Union { left, right, all }
//! ```

pub mod clause;
pub mod command;
pub mod expr;
pub mod pattern;
pub mod plan;
mod query;

// ============================================================
// Query/Statement 구조체 re-export
// ============================================================
pub use query::{CallSubquery, ProcedureCall, Query, QueryBody, SingleQuery, Statement, YieldItem};

// ============================================================
// Clause 구조체 re-export
// ============================================================
pub use clause::{
    DatabaseTarget, LetClause, OrderItem, ReadingClause, RemoveItem, ReturnClause, ReturnItem,
    SetItem, UnwindClause, UpdatingClause, UseClause, WithClause,
};

// ============================================================
// Command 구조체 re-export
// ============================================================
pub use command::{
    AdminCommand, CascadeAction, ConstraintCommand, IndexCommand, ReferentialAction, SchemaCommand,
    SchemaPropertyDef,
};

// ============================================================
// Expression 구조체 re-export
// ============================================================
pub use expr::{
    AggregateFunction, AggregateFunctionCall, ArithmeticOp, CaseExpression, ComparisonOp,
    ExistsExpression, Expression, ListComprehensionExpr, LogicalOp, PropertyPattern,
    ScalarFunction, ScalarFunctionCall, VectorExpression, WhenClause,
};

// ============================================================
// Pattern 구조체 re-export
// ============================================================
pub use pattern::{
    EdgeDirection, EdgePattern, NodePattern, PathFunction, Pattern, PatternElement, PatternPart,
    VariableLengthRange,
};

// ============================================================
// Plan 구조체 re-export
// ============================================================
pub use plan::{IndexCondition, PlanNode, PlanNodeType, QueryPlan, RangeBound};
