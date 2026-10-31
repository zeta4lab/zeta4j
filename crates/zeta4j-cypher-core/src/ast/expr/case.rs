// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! CASE 표현식 AST
//!
//! Simple CASE와 Generic CASE 표현식 정의

use super::Expression;

/// CASE 표현식
///
/// Simple CASE: `CASE expr WHEN value THEN result ... ELSE default END`
/// Generic CASE: `CASE WHEN condition THEN result ... ELSE default END`
///
/// # 예제
///
/// ```text
/// // Simple CASE
/// CASE n.status
///   WHEN 'active' THEN 1
///   WHEN 'pending' THEN 2
///   ELSE 0
/// END
///
/// // Generic CASE
/// CASE
///   WHEN n.age >= 18 THEN 'adult'
///   WHEN n.age >= 13 THEN 'teen'
///   ELSE 'child'
/// END
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct CaseExpression {
    /// Simple CASE의 비교 대상 (Generic CASE는 None)
    pub operand: Option<Box<Expression>>,
    /// WHEN ... THEN ... 절들
    pub when_clauses: Vec<WhenClause>,
    /// ELSE 절 (없으면 None, NULL 반환)
    pub else_result: Option<Box<Expression>>,
}

/// WHEN ... THEN ... 절
///
/// # 예제
///
/// ```text
/// WHEN 'active' THEN 1       // Simple CASE: 값 비교
/// WHEN n.age >= 18 THEN 'adult'  // Generic CASE: 조건식
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct WhenClause {
    /// WHEN 조건 (Simple CASE: 비교 값, Generic CASE: 조건식)
    pub condition: Expression,
    /// THEN 결과
    pub result: Expression,
}
