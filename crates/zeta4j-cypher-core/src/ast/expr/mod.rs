//! 표현식 AST 모듈
//!
//! Cypher 쿼리의 모든 표현식 타입을 정의합니다.
//!
//! # 구조
//!
//! - [`Expression`]: 표현식 enum (리터럴, 변수, 연산, 함수 호출 등)
//! - [`PropertyPattern`]: 프로퍼티 맵 타입
//! - [`operator`]: 비교, 논리, 산술 연산자
//! - [`aggregate`]: 집계 함수 (COUNT, SUM, AVG 등)
//! - [`function`]: 스칼라 함수 (200+ 함수)
//! - [`case`]: CASE 표현식
//! - [`list`]: 리스트 컴프리헨션, EXISTS, 벡터 표현식

mod aggregate;
mod case;
mod function;
mod list;
mod operator;

pub use aggregate::{AggregateFunction, AggregateFunctionCall};
pub use case::{CaseExpression, WhenClause};
pub use function::{ScalarFunction, ScalarFunctionCall};
pub use list::{ExistsExpression, ListComprehensionExpr, VectorExpression};
pub use operator::{ArithmeticOp, ComparisonOp, LogicalOp};

use indexmap::IndexMap;
use zeta4j_graph::core::Value;

/// 패턴에서 사용되는 프로퍼티 맵 (Expression 지원)
///
/// CREATE (n {name: $param}) 과 같이 파라미터 사용 가능합니다.
/// 키는 프로퍼티 이름, 값은 [`Expression`]입니다.
/// IndexMap을 사용하여 삽입 순서를 유지합니다.
pub type PropertyPattern = IndexMap<String, Expression>;

/// 표현식 (Expression)
///
/// Cypher 쿼리에서 값을 계산하는 모든 표현식을 표현합니다.
/// WHERE 조건, RETURN 항목, SET 값 등 다양한 곳에서 사용됩니다.
///
/// # Variants
///
/// ## 기본 값
/// - `Literal` - 리터럴 값 (Integer, Float, String, Boolean, Null)
/// - `Parameter` - 파라미터 참조 (`$name`)
/// - `Variable` - 변수 참조
/// - `PropertyAccess` - 속성 접근 (`n.name`)
///
/// ## 연산
/// - `Comparison` - 비교 연산 (`=`, `<>`, `<`, `>` 등)
/// - `Logical` - 논리 연산 (`AND`, `OR`, `NOT`, `XOR`)
/// - `Arithmetic` - 산술 연산 (`+`, `-`, `*`, `/`, `%`, `^`)
/// - `IsNull` - NULL 체크 (`IS NULL`, `IS NOT NULL`)
///
/// ## 컬렉션
/// - `List` - 리스트 리터럴 (`[1, 2, 3]`)
/// - `Map` - 맵 리터럴 (`{key: value}`)
/// - `Subscript` - 인덱스 접근 (`list[0]`)
/// - `SubscriptRange` - 범위 슬라이싱 (`list[1..3]`)
///
/// ## 함수 호출
/// - `Aggregate` - 집계 함수 (`COUNT`, `SUM` 등)
/// - `FunctionCall` - 스칼라 함수 (200+ 함수)
///
/// ## 고급 표현식
/// - `Case` - CASE 표현식
/// - `Exists` - EXISTS 서브쿼리
/// - `ListComprehension` - 리스트 컴프리헨션
/// - `Vector` - 벡터 리터럴 (Phase 3)
///
/// # 예제
///
/// ```text
/// // 리터럴
/// 42, 'Alice', true, null
///
/// // 변수 및 속성
/// n, n.name, n.age
///
/// // 비교
/// n.age > 30, n.name = 'Alice'
///
/// // 논리
/// n.active AND n.verified
///
/// // 산술
/// n.price * 1.1
///
/// // 함수 호출
/// toUpper(n.name), count(*)
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// 리터럴 값 (Integer, Float, String, Boolean, Null)
    Literal(Value),
    /// 파라미터 참조 - `$name` 또는 `$1` 형식
    Parameter(String),
    /// 변수 참조 - MATCH에서 바인딩된 노드/관계/경로 변수
    Variable(String),
    /// 속성 접근 - `변수.속성` 형식 (예: `n.name`)
    PropertyAccess(String, String),
    /// 비교 연산 - 두 표현식의 비교 (`=`, `<>`, `<`, `<=`, `>`, `>=`, `CONTAINS`, `STARTS WITH`, `ENDS WITH`, `IN`)
    Comparison {
        /// 좌측 피연산자
        left: Box<Expression>,
        /// 비교 연산자
        op: ComparisonOp,
        /// 우측 피연산자
        right: Box<Expression>,
    },
    /// 논리 연산 - `AND`, `OR`, `NOT`, `XOR`
    Logical {
        /// 좌측 피연산자 (NOT의 경우 None)
        left: Option<Box<Expression>>,
        /// 논리 연산자
        op: LogicalOp,
        /// 우측 피연산자 (또는 NOT의 대상)
        right: Box<Expression>,
    },
    /// IS NULL / IS NOT NULL 체크
    IsNull {
        /// 검사할 표현식
        expr: Box<Expression>,
        /// true면 IS NOT NULL, false면 IS NULL
        negated: bool,
    },
    /// 리스트 리터럴 - `[1, 2, 3]` 또는 `['a', 'b']`
    List(Vec<Expression>),
    /// 맵 리터럴 - `{key: value, ...}`
    Map(Vec<(String, Expression)>),
    /// 집계 함수 호출 - `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `COLLECT`
    Aggregate(AggregateFunctionCall),
    /// 스칼라 함수 호출 - `toUpper`, `toLower`, `trim`, `size`, `substring` 등 200+ 함수
    FunctionCall(ScalarFunctionCall),
    /// CASE 표현식 - Simple CASE 또는 Generic CASE
    Case(CaseExpression),
    /// EXISTS 서브쿼리 - 패턴 존재 여부 확인
    Exists(ExistsExpression),
    /// 산술 연산 - 덧셈, 뺄셈, 곱셈, 나눗셈, 나머지
    Arithmetic {
        /// 좌측 피연산자
        left: Box<Expression>,
        /// 산술 연산자
        op: ArithmeticOp,
        /// 우측 피연산자
        right: Box<Expression>,
    },
    /// 리스트 컴프리헨션 - [x IN list WHERE cond | expr]
    ListComprehension(ListComprehensionExpr),
    /// 배열/리스트 인덱스 접근 - expr[index]
    Subscript {
        /// 배열/리스트 표현식
        expr: Box<Expression>,
        /// 인덱스 표현식
        index: Box<Expression>,
    },
    /// 배열/리스트 범위 슬라이싱 - expr[start..end]
    SubscriptRange {
        /// 배열/리스트 표현식
        expr: Box<Expression>,
        /// 시작 인덱스 (None이면 처음부터)
        start: Option<Box<Expression>>,
        /// 끝 인덱스 (None이면 끝까지, exclusive)
        end: Option<Box<Expression>>,
    },
    /// Phase 3: 벡터 리터럴 - vector([0.1, 0.2, 0.3]) 또는 vector($param)
    Vector(VectorExpression),
}
