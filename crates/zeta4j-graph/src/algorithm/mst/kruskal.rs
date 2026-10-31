// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Kruskal's Minimum Spanning Tree 알고리즘
//!
//! 모든 간선을 가중치 순으로 정렬한 후,
//! Union-Find를 사용하여 사이클을 방지하면서 간선을 추가합니다.
//!
//! ## 시간복잡도
//! - O(E log E) for sorting edges
//!
//! ## 특징
//! - 희소 그래프에 효율적
//! - 비연결 그래프에서도 Spanning Forest 생성 가능
//! - 간선 중심 접근

use super::{MSTAlgorithm, MSTResult, SpanningForestResult, UnionFind};
use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashMap;

/// Kruskal 알고리즘 설정
#[derive(Debug, Clone, Default)]
pub struct KruskalConfig {
    /// 가중치 속성 이름 (None이면 1.0)
    pub weight_property: Option<String>,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl KruskalConfig {
    /// 가중치 속성 설정
    pub fn with_weight_property(mut self, property: String) -> Self {
        self.weight_property = Some(property);
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Kruskal's MST 알고리즘
pub struct Kruskal<'a> {
    engine: &'a GraphEngine,
    config: KruskalConfig,
}

impl<'a> Kruskal<'a> {
    /// 새로운 Kruskal 인스턴스 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: KruskalConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: KruskalConfig) -> Self {
        Self { engine, config }
    }

    /// MST 계산
    pub async fn run(&self) -> Result<MSTResult> {
        let (nodes, edges) = self.collect_graph().await?;

        if nodes.is_empty() {
            return Ok(MSTResult::empty(MSTAlgorithm::Kruskal));
        }

        if nodes.len() == 1 {
            return Ok(MSTResult::single_node(nodes[0], MSTAlgorithm::Kruskal));
        }

        // 노드 ID를 인덱스로 매핑
        let node_to_idx: HashMap<NodeId, usize> =
            nodes.iter().enumerate().map(|(i, &n)| (n, i)).collect();

        // 간선을 가중치 순으로 정렬
        let mut sorted_edges = edges;
        sorted_edges.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        // Union-Find 초기화
        let mut uf = UnionFind::new(nodes.len());
        let mut mst_edges = Vec::new();

        // Kruskal 알고리즘
        for (from, to, weight) in sorted_edges {
            let from_idx = node_to_idx[&from];
            let to_idx = node_to_idx[&to];

            // 사이클이 생기지 않으면 간선 추가
            if uf.union(from_idx, to_idx) {
                mst_edges.push((from, to, weight));

                // N-1개 간선이 추가되면 완료
                if mst_edges.len() == nodes.len() - 1 {
                    break;
                }
            }
        }

        Ok(MSTResult::new(mst_edges, MSTAlgorithm::Kruskal))
    }

    /// Spanning Forest 계산 (비연결 그래프용)
    pub async fn spanning_forest(&self) -> Result<SpanningForestResult> {
        let (nodes, edges) = self.collect_graph().await?;

        if nodes.is_empty() {
            return Ok(SpanningForestResult::empty());
        }

        // 노드 ID를 인덱스로 매핑
        let node_to_idx: HashMap<NodeId, usize> =
            nodes.iter().enumerate().map(|(i, &n)| (n, i)).collect();
        let idx_to_node: HashMap<usize, NodeId> =
            nodes.iter().enumerate().map(|(i, &n)| (i, n)).collect();

        // 간선을 가중치 순으로 정렬
        let mut sorted_edges = edges;
        sorted_edges.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        // Union-Find 초기화
        let mut uf = UnionFind::new(nodes.len());
        let mut all_mst_edges: Vec<(NodeId, NodeId, f64)> = Vec::new();

        // Kruskal 알고리즘 (모든 간선 처리)
        for (from, to, weight) in sorted_edges {
            let from_idx = node_to_idx[&from];
            let to_idx = node_to_idx[&to];

            if uf.union(from_idx, to_idx) {
                all_mst_edges.push((from, to, weight));
            }
        }

        // 각 연결 요소별로 MST 분리
        let roots = uf.roots();
        let mut trees = Vec::new();

        for root in roots {
            let component_members: Vec<usize> = uf.members(root);
            let component_nodes: std::collections::HashSet<NodeId> =
                component_members.iter().map(|&i| idx_to_node[&i]).collect();

            let component_edges: Vec<(NodeId, NodeId, f64)> = all_mst_edges
                .iter()
                .filter(|(from, to, _)| {
                    component_nodes.contains(from) && component_nodes.contains(to)
                })
                .cloned()
                .collect();

            if !component_nodes.is_empty() {
                let mut mst = MSTResult::new(component_edges, MSTAlgorithm::Kruskal);
                // 고립 노드 처리
                if mst.node_count == 0 && component_nodes.len() == 1 {
                    let first_node = *component_nodes.iter().next().ok_or_else(|| {
                        crate::error::GraphError::System {
                            message: "empty component_nodes iterator".to_string(),
                        }
                    })?;
                    mst = MSTResult::single_node(first_node, MSTAlgorithm::Kruskal);
                }
                trees.push(mst);
            }
        }

        Ok(SpanningForestResult::new(trees))
    }

    /// 그래프 정보 수집
    async fn collect_graph(&self) -> Result<(Vec<NodeId>, Vec<(NodeId, NodeId, f64)>)> {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut seen_edges = std::collections::HashSet::new();

        // 모든 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
            }
        }

        // 모든 간선 수집
        let edge_count = self.engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Some(edge) = self.engine.get_edge(edge_id).await? {
                // 관계 타입 필터링
                if let Some(ref types) = self.config.relationship_types
                    && !types.contains(&edge.relation_type().to_string())
                {
                    continue;
                }

                let from = edge.from_node();
                let to = edge.to_node();

                if from == to {
                    continue;
                }

                // 중복 간선 방지 (무방향)
                let edge_key = if from < to { (from, to) } else { (to, from) };
                if seen_edges.contains(&edge_key) {
                    continue;
                }
                seen_edges.insert(edge_key);

                // 가중치 계산
                let weight = if let Some(ref prop_name) = self.config.weight_property {
                    edge.get_property(prop_name)
                        .and_then(|v| match v {
                            crate::core::Value::Integer(i) => Some(*i as f64),
                            crate::core::Value::Float(f) => Some(*f),
                            _ => None,
                        })
                        .unwrap_or(1.0)
                } else {
                    1.0
                };

                edges.push((from, to, weight));
            }
        }

        Ok((nodes, edges))
    }
}

/// Kruskal MST 편의 함수
pub async fn kruskal_mst(engine: &GraphEngine) -> Result<MSTResult> {
    Kruskal::new(engine).run().await
}
