// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Degree Centrality 알고리즘
//!
//! 노드의 연결 정도를 측정합니다.

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Degree Centrality 결과
#[derive(Debug, Clone, Default)]
pub struct DegreeCentrality {
    /// 들어오는 엣지 수 (in-degree)
    pub in_degree: usize,
    /// 나가는 엣지 수 (out-degree)
    pub out_degree: usize,
    /// 전체 연결 수 (undirected degree)
    pub total_degree: usize,
}

impl DegreeCentrality {
    /// 새로운 DegreeCentrality 생성
    pub fn new(in_degree: usize, out_degree: usize) -> Self {
        Self {
            in_degree,
            out_degree,
            total_degree: in_degree + out_degree,
        }
    }

    /// 정규화된 중심성 계산 (전체 노드 수 기준)
    pub fn normalized(&self, total_nodes: usize) -> f64 {
        if total_nodes <= 1 {
            return 0.0;
        }
        self.total_degree as f64 / (total_nodes - 1) as f64
    }

    /// In-degree 정규화 값
    pub fn normalized_in(&self, total_nodes: usize) -> f64 {
        if total_nodes <= 1 {
            return 0.0;
        }
        self.in_degree as f64 / (total_nodes - 1) as f64
    }

    /// Out-degree 정규화 값
    pub fn normalized_out(&self, total_nodes: usize) -> f64 {
        if total_nodes <= 1 {
            return 0.0;
        }
        self.out_degree as f64 / (total_nodes - 1) as f64
    }
}

/// 단일 노드의 Degree Centrality 계산
///
/// # Arguments
/// * `engine` - GraphEngine 참조
/// * `node_id` - 대상 노드 ID
/// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
///
/// # Returns
/// DegreeCentrality 구조체
pub async fn calculate_degree(
    engine: &GraphEngine,
    node_id: NodeId,
    rel_types: Option<&[String]>,
) -> Result<DegreeCentrality> {
    let edge_ids = engine.find_edges_by_node(node_id)?;

    let mut in_degree = 0;
    let mut out_degree = 0;

    for edge_id in edge_ids {
        if let Some(edge) = engine.get_edge(edge_id).await? {
            // 관계 타입 필터링
            if let Some(types) = rel_types
                && !types.contains(&edge.relation_type().to_string())
            {
                continue;
            }

            if edge.from_node() == node_id {
                out_degree += 1;
            }
            if edge.to_node() == node_id {
                in_degree += 1;
            }
        }
    }

    Ok(DegreeCentrality::new(in_degree, out_degree))
}

/// 전체 그래프의 모든 노드에 대한 Degree Centrality 계산
///
/// # Arguments
/// * `engine` - GraphEngine 참조
/// * `rel_types` - 고려할 관계 타입들 (None이면 모든 타입)
///
/// # Returns
/// 노드 ID -> DegreeCentrality 매핑
pub async fn calculate_all_degrees(
    engine: &GraphEngine,
    rel_types: Option<&[String]>,
) -> Result<HashMap<NodeId, DegreeCentrality>> {
    let mut result: HashMap<NodeId, DegreeCentrality> = HashMap::new();

    // 모든 노드에 대해 초기화
    let node_count = engine.node_count().await?;

    // 노드 ID 범위를 순회 (1부터 node_count까지, ID는 1부터 시작)
    for node_id in 1..=node_count as u64 {
        if engine.get_node(node_id).await?.is_some() {
            result.insert(node_id, DegreeCentrality::default());
        }
    }

    // 모든 엣지를 순회하며 degree 계산
    let edge_count = engine.edge_count().await;
    for edge_id in 1..=edge_count as u64 {
        if let Some(edge) = engine.get_edge(edge_id).await? {
            // 관계 타입 필터링
            if let Some(types) = rel_types
                && !types.contains(&edge.relation_type().to_string())
            {
                continue;
            }

            let from = edge.from_node();
            let to = edge.to_node();

            // from 노드의 out-degree 증가
            if let Some(centrality) = result.get_mut(&from) {
                centrality.out_degree += 1;
                centrality.total_degree += 1;
            }

            // to 노드의 in-degree 증가
            if let Some(centrality) = result.get_mut(&to) {
                centrality.in_degree += 1;
                centrality.total_degree += 1;
            }
        }
    }

    Ok(result)
}

/// Degree Centrality 통계
#[expect(dead_code)] // 향후 CALL gds.degree.stats() 프로시저에 사용
#[derive(Debug, Clone)]
pub struct DegreeStats {
    /// 최대 degree
    pub max_degree: usize,
    /// 최소 degree
    pub min_degree: usize,
    /// 평균 degree
    pub avg_degree: f64,
    /// 중앙값 degree
    pub median_degree: f64,
    /// 최대 degree 노드
    pub max_node: Option<NodeId>,
    /// 최소 degree 노드
    pub min_node: Option<NodeId>,
}

/// Degree 통계 계산
#[expect(dead_code)] // DegreeStats와 함께 향후 프로시저에 사용
pub fn calculate_degree_stats(degrees: &HashMap<NodeId, DegreeCentrality>) -> DegreeStats {
    if degrees.is_empty() {
        return DegreeStats {
            max_degree: 0,
            min_degree: 0,
            avg_degree: 0.0,
            median_degree: 0.0,
            max_node: None,
            min_node: None,
        };
    }

    let mut values: Vec<(NodeId, usize)> = degrees
        .iter()
        .map(|(&id, c)| (id, c.total_degree))
        .collect();

    values.sort_by_key(|(_, d)| *d);

    let min = values[0];
    let max = values[values.len() - 1];

    let sum: usize = values.iter().map(|(_, d)| d).sum();
    let avg = sum as f64 / values.len() as f64;

    let median = if values.len().is_multiple_of(2) {
        let mid = values.len() / 2;
        (values[mid - 1].1 + values[mid].1) as f64 / 2.0
    } else {
        values[values.len() / 2].1 as f64
    };

    DegreeStats {
        max_degree: max.1,
        min_degree: min.1,
        avg_degree: avg,
        median_degree: median,
        max_node: Some(max.0),
        min_node: Some(min.0),
    }
}

/// 상위 N개 노드 (degree 기준) 반환
#[expect(dead_code)] // 향후 topN 지원 프로시저에 사용
pub fn top_n_by_degree(
    degrees: &HashMap<NodeId, DegreeCentrality>,
    n: usize,
) -> Vec<(NodeId, DegreeCentrality)> {
    let mut items: Vec<_> = degrees.iter().map(|(&id, c)| (id, c.clone())).collect();

    items.sort_by_key(|item| std::cmp::Reverse(item.1.total_degree));
    items.truncate(n);
    items
}
