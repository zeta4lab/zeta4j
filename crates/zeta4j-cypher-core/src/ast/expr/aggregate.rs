//! 집계 함수 AST
//!
//! COUNT, SUM, AVG, MIN, MAX, COLLECT

use super::Expression;

/// 집계 함수 종류 - 행 그룹을 단일 값으로 집계
///
/// GROUP BY와 함께 사용하거나, 전체 결과를 하나로 집계합니다.
///
/// # 예제
///
/// ```text
/// // 전체 행 수
/// MATCH (n:Person) RETURN count(*)
///
/// // 그룹별 집계
/// MATCH (n:Person)
/// RETURN n.city, count(n) AS cnt, avg(n.age) AS avgAge
/// ORDER BY cnt DESC
///
/// // DISTINCT 집계
/// MATCH (n:Person) RETURN count(DISTINCT n.city)
///
/// // 리스트 수집
/// MATCH (n:Person) RETURN collect(n.name) AS names
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum AggregateFunction {
    /// `COUNT(*)` - 전체 행 수 (NULL 포함)
    CountAll,
    /// `COUNT(expr)` - NULL 제외 수
    Count,
    /// `SUM(expr)` - 숫자 합계
    Sum,
    /// `AVG(expr)` - 숫자 평균
    Avg,
    /// `MIN(expr)` - 최소값
    Min,
    /// `MAX(expr)` - 최대값
    Max,
    /// `COLLECT(expr)` - 결과를 리스트로 수집
    Collect,
}

/// 집계 함수 호출 표현식
///
/// 집계 함수, 인자, DISTINCT 여부를 포함합니다.
///
/// # 예제
///
/// ```text
/// COUNT(*)                 // function: CountAll, argument: None
/// COUNT(n.name)            // function: Count, argument: n.name
/// COUNT(DISTINCT n.city)   // function: Count, argument: n.city, distinct: true
/// SUM(n.salary)            // function: Sum, argument: n.salary
/// COLLECT(n)               // function: Collect, argument: n
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct AggregateFunctionCall {
    /// 집계 함수 종류 (COUNT, SUM, AVG, MIN, MAX, COLLECT)
    pub function: AggregateFunction,
    /// 인자 표현식 (`COUNT(*)`는 None)
    pub argument: Option<Box<Expression>>,
    /// DISTINCT 여부 (`COUNT(DISTINCT expr)`)
    pub distinct: bool,
}
