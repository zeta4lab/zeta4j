//! 쿼리 AST
//!
//! Cypher 쿼리의 최상위 구조체들
//!
//! - [`Query`]: 최상위 쿼리 구조
//! - [`QueryBody`]: 쿼리 본체 (Single 또는 UNION)
//! - [`SingleQuery`]: 단일 쿼리
//! - [`Statement`]: 개별 실행 단위

use super::clause::{
    LetClause, ReadingClause, ReturnClause, UnwindClause, UpdatingClause, UseClause, WithClause,
};
use super::command::{AdminCommand, ConstraintCommand, IndexCommand, SchemaCommand};
use super::expr::Expression;

/// Cypher 쿼리의 최상위 구조
///
/// 파서가 Cypher 문자열을 파싱하면 이 구조체가 반환됩니다.
/// `explain`과 `analyze` 플래그를 통해 실행 계획 분석 모드를 지정할 수 있습니다.
///
/// # 필드
///
/// * `explain` - `EXPLAIN` 키워드가 있으면 true. 실행 계획만 반환하고 실제 실행하지 않음
/// * `analyze` - `EXPLAIN ANALYZE` 키워드가 있으면 true. 실제 실행하며 통계 수집
/// * `body` - 쿼리 본체 (단일 쿼리 또는 UNION)
///
/// # 예제
///
/// ```ignore
/// use crate::parser::parse_query;
///
/// // 일반 쿼리
/// let (_, query) = parse_query("MATCH (n:Person) RETURN n").unwrap();
/// assert!(!query.explain);
/// assert!(!query.analyze);
///
/// // EXPLAIN 쿼리
/// let (_, explain_query) = parse_query("EXPLAIN MATCH (n) RETURN n").unwrap();
/// assert!(explain_query.explain);
///
/// // EXPLAIN ANALYZE 쿼리
/// let (_, analyze_query) = parse_query("EXPLAIN ANALYZE MATCH (n) RETURN n").unwrap();
/// assert!(analyze_query.explain);
/// assert!(analyze_query.analyze);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// EXPLAIN 모드 여부 - true면 실행 계획만 반환
    pub explain: bool,
    /// EXPLAIN ANALYZE 모드 여부 - true면 실제 실행하며 통계 수집
    pub analyze: bool,
    /// 쿼리 본체 (단일 쿼리 또는 UNION)
    pub body: QueryBody,
}

/// 쿼리 본체 - 단일 쿼리 또는 UNION
///
/// Cypher는 `UNION` 또는 `UNION ALL`을 사용하여 여러 쿼리의 결과를 결합할 수 있습니다.
///
/// # Variants
///
/// * `Single` - 단일 쿼리 (가장 일반적인 형태)
/// * `Union` - 두 쿼리의 UNION (중복 제거 또는 전체)
///
/// # 예제
///
/// ```text
/// // 단일 쿼리
/// MATCH (n:Person) RETURN n.name
///
/// // UNION (중복 제거)
/// MATCH (n:Person) RETURN n.name
/// UNION
/// MATCH (n:Company) RETURN n.name
///
/// // UNION ALL (중복 포함)
/// MATCH (n:Person) RETURN n.name
/// UNION ALL
/// MATCH (n:Company) RETURN n.name
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum QueryBody {
    /// 단일 쿼리 (가장 일반적인 형태)
    Single(SingleQuery),
    /// UNION 쿼리 - 두 쿼리의 결과를 결합
    Union {
        /// 좌측 쿼리
        left: Box<QueryBody>,
        /// 우측 쿼리
        right: Box<QueryBody>,
        /// true: UNION ALL (중복 포함), false: UNION (중복 제거)
        all: bool,
    },
}

/// 단일 쿼리 - Statement들의 순차적 리스트
///
/// Cypher 쿼리는 여러 절(Statement)들이 순서대로 실행되는 파이프라인 구조입니다.
/// 예를 들어 `MATCH ... WHERE ... WITH ... RETURN`은 4개의 Statement로 구성됩니다.
///
/// # 예제
///
/// ```text
/// MATCH (n:Person)           // Statement 1: Reading(Match)
/// WHERE n.age > 30           // WHERE는 Match의 일부
/// WITH n, n.salary AS sal    // Statement 2: With
/// ORDER BY sal DESC
/// RETURN n.name, sal         // Statement 3: Return
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct SingleQuery {
    /// 순서대로 실행될 Statement들
    pub statements: Vec<Statement>,
}

/// 개별 실행 단위 (Statement)
///
/// Cypher 쿼리의 각 절을 표현합니다. 쿼리는 여러 Statement들의 파이프라인으로 구성되며,
/// 앞 Statement의 결과가 다음 Statement의 입력이 됩니다.
///
/// # Variants
///
/// * `Reading` - 데이터 읽기 (MATCH, OPTIONAL MATCH)
/// * `Updating` - 데이터 변경 (CREATE, DELETE, SET, MERGE)
/// * `With` - 중간 결과 변환 (WITH ... AS ...)
/// * `Unwind` - 리스트를 행으로 펼침 (UNWIND ... AS ...)
/// * `Return` - 최종 결과 반환 (RETURN ...)
/// * `Finish` - 결과 없이 종료 (FINISH) - Neo4j 5.x
/// * `Call` - 서브쿼리 실행 (CALL { ... })
/// * `Index` - 인덱스 관리 (CREATE INDEX, DROP INDEX, SHOW INDEXES)
/// * `Schema` - 스키마 관리 (CREATE NODE LABEL, SHOW NODE LABELS)
/// * `Constraint` - 제약조건 관리 (CREATE CONSTRAINT, DROP CONSTRAINT)
/// * `Procedure` - 프로시저 호출 (CALL db.index.vector.queryNodes(...) YIELD ...)
/// * `Admin` - 관리 명령 (CREATE/DROP USER, CREATE/DROP DATABASE 등)
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// 데이터베이스 선택 절 (USE)
    ///
    /// 쿼리를 실행할 데이터베이스를 지정합니다.
    ///
    /// # 예제
    /// ```text
    /// USE mydb
    /// MATCH (n) RETURN n
    /// ```
    Use(UseClause),
    /// 데이터 읽기 절 (MATCH, OPTIONAL MATCH)
    Reading(ReadingClause),
    /// 데이터 변경 절 (CREATE, DELETE, SET, MERGE)
    Updating(UpdatingClause),
    /// 중간 결과 변환 절 (WITH)
    With(WithClause),
    /// 리스트 펼침 절 (UNWIND)
    Unwind(UnwindClause),
    /// 변수 바인딩 절 (LET) - Neo4j 5.x
    ///
    /// WITH 없이 변수를 표현식에 바인딩합니다.
    ///
    /// # 예제
    /// ```text
    /// LET threshold = 100
    /// MATCH (n:Product) WHERE n.price > threshold RETURN n
    /// ```
    Let(LetClause),
    /// 결과 반환 절 (RETURN)
    Return(ReturnClause),
    /// 결과 없이 종료 (FINISH) - Neo4j 5.x
    ///
    /// 쿼리가 결과를 반환하지 않음을 명시합니다.
    /// 부작용(side-effect)만 있는 쿼리에 사용됩니다.
    ///
    /// # 예제
    /// ```text
    /// MATCH (n:Person) SET n.processed = true FINISH
    /// ```
    Finish,
    /// 서브쿼리 실행 절 (CALL { ... })
    Call(CallSubquery),
    /// 프로시저 호출 절 (CALL procedure() YIELD ...)
    Procedure(ProcedureCall),
    /// 인덱스 관리 명령 (CREATE/DROP/SHOW INDEX)
    Index(IndexCommand),
    /// 스키마 관리 명령 (CREATE/DROP/SHOW NODE LABEL)
    Schema(SchemaCommand),
    /// 제약조건 관리 명령 (CREATE/DROP/SHOW CONSTRAINT)
    Constraint(ConstraintCommand),
    /// 관리 명령 (CREATE/DROP USER, CREATE/DROP DATABASE 등)
    Admin(AdminCommand),
}

/// CALL 서브쿼리 절
///
/// `CALL { subquery }`
///
/// 서브쿼리를 실행하고 결과를 현재 컨텍스트에 병합
#[derive(Debug, Clone, PartialEq)]
pub struct CallSubquery {
    /// 서브쿼리 (SingleQuery)
    pub subquery: SingleQuery,
}

/// Phase 3: 프로시저 호출
///
/// `CALL db.index.vector.queryNodes('idx', 10, $vec) YIELD node, score`
///
/// 프로시저를 호출하고 YIELD로 결과를 바인딩
#[derive(Debug, Clone, PartialEq)]
pub struct ProcedureCall {
    /// 프로시저 이름 (e.g., "db.index.vector.queryNodes")
    pub name: String,
    /// 인수
    pub arguments: Vec<Expression>,
    /// YIELD 절 (선택적)
    pub yields: Option<Vec<YieldItem>>,
}

/// Phase 3: YIELD 항목
///
/// `YIELD node, score AS similarity`
#[derive(Debug, Clone, PartialEq)]
pub struct YieldItem {
    /// 변수 이름
    pub variable: String,
    /// 별칭 (AS)
    pub alias: Option<String>,
}
