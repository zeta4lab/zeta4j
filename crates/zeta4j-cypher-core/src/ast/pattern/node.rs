// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 노드 패턴 AST

use super::super::expr::PropertyPattern;

/// 노드 패턴 - MATCH에서 노드를 매칭하는 패턴
///
/// Cypher의 `(변수:레이블 {프로퍼티})` 구문을 표현합니다.
/// 변수, 레이블, 프로퍼티 모두 선택적입니다.
///
/// # 예제
///
/// ```text
/// ()                          // 모든 노드
/// (n)                         // 모든 노드, 변수 n에 바인딩
/// (n:Person)                  // Person 레이블을 가진 노드
/// (n:Person:Employee)         // Person AND Employee 레이블
/// (n:Person {name: 'Alice'})  // Person 레이블 + name='Alice'
/// (n {age: $age})             // 파라미터 사용
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct NodePattern {
    /// 노드에 바인딩할 변수명 (선택적)
    pub variable: Option<String>,
    /// 매칭할 레이블들 (여러 개면 AND 조건)
    pub labels: Vec<String>,
    /// 매칭할 프로퍼티 조건 (Expression 지원: 리터럴, 파라미터, 변수 등)
    pub properties: Option<PropertyPattern>,
}
