// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 그래프 알고리즘 함수 위임 모듈
//!
//! 그래프 함수들은 그래프 엔진(GraphEngine) 접근이 필요하므로
//! `executor/` 모듈에서 직접 구현됩니다.
//!
//! ## 기본 함수 (`executor/mod.rs`)
//! - type(): 관계 타입 반환
//! - labels(): 노드 라벨 반환
//! - id(): 노드/엣지 ID 반환
//! - properties(): 속성 맵 반환
//!
//! ## 그래프 알고리즘 (`executor/scalar_graph.rs`)
//! - 경로 탐색: shortestPath, dijkstra, bellmanFord, aStar
//! - 중심성: pageRank, betweenness, closeness
//! - 커뮤니티: connectedComponents, louvain, labelPropagation
//! - 유사도: jaccard, overlap, cosine
//! - 링크 예측: commonNeighbors, adamicAdar
//! - 그래프 메트릭: graphDensity, triangleCount
//!
//! ## 왜 위임 스텁인가?
//! 그래프 함수들은 `GraphEngine`에 대한 참조가 필요하므로
//! 순수 함수로 구현할 수 없습니다. 따라서 이 모듈은
//! `functions/mod.rs`의 라우팅 구조를 위한 스텁만 제공합니다.
//!
//! 실제 구현은 `executor/mod.rs`와 `executor/scalar_graph.rs`에서
//! `QueryExecutor`의 메서드로 구현되어 있습니다.

use crate::ast::ScalarFunction;
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

/// 그래프 알고리즘 함수 평가 (위임 스텁)
///
/// 그래프 함수는 `GraphEngine` 접근이 필요하므로
/// `QueryExecutor`에서 직접 처리됩니다.
///
/// # 참고
/// - 기본 함수 (type, labels, id, properties): `executor/mod.rs`
/// - 그래프 알고리즘: `executor/scalar_graph.rs`
pub fn evaluate(func: &ScalarFunction, _args: &[Value]) -> Result<Value> {
    // 그래프 알고리즘은 그래프 엔진 접근이 필요
    Err(GraphError::Validation {
        message: format!(
            "{:?} requires GraphEngine access - handled by QueryExecutor",
            func
        ),
    })
}
