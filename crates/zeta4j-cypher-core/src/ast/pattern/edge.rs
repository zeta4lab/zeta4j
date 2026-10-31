// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 엣지(관계) 패턴 AST

use super::super::expr::PropertyPattern;

/// 가변 길이 관계 패턴 범위
///
/// 그래프에서 경로 탐색 시 홉 수를 지정합니다.
///
/// # 패턴 형식
///
/// * `[*]` - 1홉 이상 무제한 (기본 최대값 적용)
/// * `[*2]` - 정확히 2홉
/// * `[*1..3]` - 1~3홉
/// * `[*..3]` - 1~3홉 (최소 생략시 1)
/// * `[*2..]` - 2홉 이상 무제한
///
/// # 예제
///
/// ```ignore
/// // 정확히 2홉
/// let range = VariableLengthRange::exact(2);
/// assert_eq!(range.min, 2);
/// assert_eq!(range.max, Some(2));
///
/// // 1~3홉
/// let range = VariableLengthRange::new(1, Some(3));
///
/// // 무제한
/// let range = VariableLengthRange::unbounded();
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct VariableLengthRange {
    /// 최소 홉 수 (기본값: 1)
    pub min: u32,
    /// 최대 홉 수 (None이면 무제한, 실제로는 기본 제한 15 적용)
    pub max: Option<u32>,
}

impl VariableLengthRange {
    /// 새 가변 길이 범위 생성
    pub fn new(min: u32, max: Option<u32>) -> Self {
        Self { min, max }
    }

    /// 정확히 n홉 범위 생성
    pub fn exact(n: u32) -> Self {
        Self {
            min: n,
            max: Some(n),
        }
    }

    /// 무제한 범위 생성 (min=1)
    pub fn unbounded() -> Self {
        Self { min: 1, max: None }
    }
}

/// 엣지(관계) 패턴 - 두 노드 사이의 관계를 매칭하는 패턴
///
/// Cypher의 `-[변수:타입 {프로퍼티}]->` 구문을 표현합니다.
/// 방향(`->`, `<-`, `--`)과 가변 길이(`[*1..3]`)를 포함할 수 있습니다.
///
/// # 예제
///
/// ```text
/// -[r]->                      // 나가는 관계, 변수 r
/// <-[r:KNOWS]-                // 들어오는 KNOWS 관계
/// -[:KNOWS|LIKES]->           // KNOWS 또는 LIKES 관계
/// -[r:KNOWS {since: 2020}]->  // 프로퍼티 조건 포함
/// -[*1..3]->                  // 1~3홉 경로
/// -[r:KNOWS*2]->              // 정확히 2홉 KNOWS 경로
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct EdgePattern {
    /// 관계에 바인딩할 변수명 (선택적)
    pub variable: Option<String>,
    /// 매칭할 관계 타입들 (여러 개면 OR 조건, 예: `KNOWS|LIKES`)
    pub rel_types: Vec<String>,
    /// 매칭할 프로퍼티 조건 (Expression 지원: 리터럴, 파라미터, 변수 등)
    pub properties: Option<PropertyPattern>,
    /// 관계 방향 (Outgoing: `->`, Incoming: `<-`, Both: `--`)
    pub direction: EdgeDirection,
    /// 가변 길이 패턴 (예: `[*1..3]`)
    pub variable_length: Option<VariableLengthRange>,
}

/// 엣지(관계) 방향
///
/// 그래프에서 관계의 탐색 방향을 지정합니다.
///
/// # Variants
///
/// * `Outgoing` - 나가는 방향 (`->`) - 시작 노드에서 끝 노드로
/// * `Incoming` - 들어오는 방향 (`<-`) - 끝 노드에서 시작 노드로
/// * `Both` - 양방향 (`--`) - 방향 무관
///
/// # 예제
///
/// ```text
/// (a)-[r:KNOWS]->(b)   // Outgoing: a에서 b로
/// (a)<-[r:KNOWS]-(b)   // Incoming: b에서 a로
/// (a)-[r:KNOWS]-(b)    // Both: 양방향
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum EdgeDirection {
    /// 나가는 방향 (`->`) - 시작 노드 → 끝 노드
    Outgoing,
    /// 들어오는 방향 (`<-`) - 끝 노드 → 시작 노드
    Incoming,
    /// 양방향 (`--`) - 방향 무관하게 탐색
    Both,
}
