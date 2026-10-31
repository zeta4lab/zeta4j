// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 리스트/벡터 관련 표현식 AST
//!
//! 리스트 컴프리헨션, EXISTS, 벡터 표현식

use super::super::pattern::Pattern;
use super::Expression;

/// 리스트 컴프리헨션 표현식
///
/// `[variable IN list WHERE condition | expression]`
///
/// # 필드
///
/// - `variable`: 반복 변수명
/// - `list_expr`: 순회할 리스트 표현식
/// - `filter`: 선택적 WHERE 조건
/// - `map_expr`: 선택적 변환 표현식 (| 이후)
///
/// # 예제
///
/// ```text
/// // 기본 형태
/// [x IN [1, 2, 3] | x * 2]  // [2, 4, 6]
///
/// // 필터링
/// [x IN [1, 2, 3, 4] WHERE x % 2 = 0]  // [2, 4]
///
/// // 필터링 + 변환
/// [x IN [1, 2, 3, 4] WHERE x > 1 | x * x]  // [4, 9, 16]
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ListComprehensionExpr {
    /// 반복 변수명
    pub variable: String,
    /// 순회할 리스트 표현식
    pub list_expr: Box<Expression>,
    /// 선택적 필터 조건 (WHERE)
    pub filter: Option<Box<Expression>>,
    /// 선택적 변환 표현식 (| expr)
    pub map_expr: Option<Box<Expression>>,
}

/// EXISTS 서브쿼리 표현식
///
/// `EXISTS { (n)-[:REL]->(m) WHERE m.prop = value }`
///
/// 패턴이 존재하는지 여부를 Boolean으로 반환합니다.
///
/// # 예제
///
/// ```text
/// // 관계 존재 여부
/// MATCH (n:Person)
/// WHERE EXISTS { (n)-[:KNOWS]->(:Person) }
/// RETURN n
///
/// // 조건부 패턴 존재
/// MATCH (n:Person)
/// WHERE EXISTS { (n)-[:WORKS_AT]->(c:Company) WHERE c.name = 'Acme' }
/// RETURN n
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ExistsExpression {
    /// 매칭할 패턴
    pub pattern: Pattern,
    /// 선택적 WHERE 조건
    pub where_clause: Option<Box<Expression>>,
}

/// Phase 3: 벡터 표현식
///
/// `vector([0.1, 0.2, 0.3])` 또는 `vector($param)` 형식
///
/// # 예제
///
/// ```text
/// // 리터럴 배열
/// vector([0.1, 0.2, 0.3])
///
/// // 파라미터
/// CALL db.index.vector.queryNodes('idx', 10, vector($embedding))
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum VectorExpression {
    /// 리터럴 배열: `vector([0.1, 0.2, 0.3])`
    Literal(Vec<Expression>),
    /// 파라미터: `vector($embedding)`
    Parameter(String),
}
