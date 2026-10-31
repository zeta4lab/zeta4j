// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Prim's Minimum Spanning Tree 알고리즘
//!
//! 하나의 시작 노드에서 출발하여 최소 가중치 간선을 선택하며
//! 트리를 확장해 나가는 방식입니다.
//!
//! ## 시간복잡도
//! - O(E log V) with binary heap
//!
//! ## 특징
//! - 밀집 그래프에 효율적
//! - 단일 연결 요소에서 동작
//! - 시작 노드 지정 가능

use super::{MSTAlgorithm, MSTResult};
use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};

/// 우선순위 큐용 간선 (최소 힙을 위해 역순 비교)
#[derive(Debug, Clone)]
struct Edge {
    weight: f64,
    from: NodeId,
    to: NodeId,
}

impl PartialEq for Edge {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}

impl Eq for Edge {}

impl PartialOrd for Edge {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Edge {
    fn cmp(&self, other: &Self) -> Ordering {
        // 최소 힙을 위해 역순
        other
            .weight
            .partial_cmp(&self.weight)
            .unwrap_or(Ordering::Equal)
    }
}

/// Prim 알고리즘 설정
#[derive(Debug, Clone, Default)]
pub struct PrimConfig {
    /// 시작 노드 (None이면 첫 번째 노드)
    pub start_node: Option<NodeId>,
    /// 가중치 속성 이름 (None이면 1.0)
    pub weight_property: Option<String>,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl PrimConfig {
    /// 시작 노드 설정
    pub fn with_start_node(mut self, node: NodeId) -> Self {
        self.start_node = Some(node);
        self
    }

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

/// Prim's MST 알고리즘
pub struct Prim<'a> {
    engine: &'a GraphEngine,
    config: PrimConfig,
}

impl<'a> Prim<'a> {
    /// 새로운 Prim 인스턴스 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: PrimConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: PrimConfig) -> Self {
        Self { engine, config }
    }

    /// MST 계산
    pub async fn run(&self) -> Result<MSTResult> {
        // 그래프 정보 수집
        let (nodes, adj) = self.build_adjacency_list().await?;

        if nodes.is_empty() {
            return Ok(MSTResult::empty(MSTAlgorithm::Prim));
        }

        if nodes.len() == 1 {
            return Ok(MSTResult::single_node(nodes[0], MSTAlgorithm::Prim));
        }

        // 시작 노드 결정
        let start_node = self.config.start_node.unwrap_or(nodes[0]);

        // Prim 알고리즘 실행
        let mut mst_edges = Vec::new();
        let mut visited = HashSet::new();
        let mut heap = BinaryHeap::new();

        // 시작 노드 추가
        visited.insert(start_node);
        if let Some(neighbors) = adj.get(&start_node) {
            for &(neighbor, weight) in neighbors {
                heap.push(Edge {
                    weight,
                    from: start_node,
                    to: neighbor,
                });
            }
        }

        // MST 구성
        while let Some(Edge { weight, from, to }) = heap.pop() {
            if visited.contains(&to) {
                continue;
            }

            visited.insert(to);
            mst_edges.push((from, to, weight));

            // 새 노드의 이웃 추가
            if let Some(neighbors) = adj.get(&to) {
                for &(neighbor, w) in neighbors {
                    if !visited.contains(&neighbor) {
                        heap.push(Edge {
                            weight: w,
                            from: to,
                            to: neighbor,
                        });
                    }
                }
            }
        }

        Ok(MSTResult::new(mst_edges, MSTAlgorithm::Prim))
    }

    /// 특정 시작 노드에서 MST 계산
    pub async fn run_from(&self, start_node: NodeId) -> Result<MSTResult> {
        let mut config = self.config.clone();
        config.start_node = Some(start_node);

        let prim = Prim::with_config(self.engine, config);
        prim.run().await
    }

    /// 인접 리스트 구축
    async fn build_adjacency_list(
        &self,
    ) -> Result<(Vec<NodeId>, HashMap<NodeId, Vec<(NodeId, f64)>>)> {
        let mut nodes = Vec::new();
        let mut adj: HashMap<NodeId, Vec<(NodeId, f64)>> = HashMap::new();

        // 모든 노드 수집
        let node_count = self.engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                nodes.push(node_id);
                adj.insert(node_id, Vec::new());
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

                // 무방향 그래프로 처리
                if adj.contains_key(&from) && adj.contains_key(&to) && from != to {
                    if let Some(list) = adj.get_mut(&from) {
                        list.push((to, weight));
                    }
                    if let Some(list) = adj.get_mut(&to) {
                        list.push((from, weight));
                    }
                }
            }
        }

        Ok((nodes, adj))
    }
}

/// Prim MST 편의 함수
pub async fn prim_mst(engine: &GraphEngine) -> Result<MSTResult> {
    Prim::new(engine).run().await
}

/// 특정 노드에서 시작하는 Prim MST 편의 함수
pub async fn prim_mst_from(engine: &GraphEngine, start_node: NodeId) -> Result<MSTResult> {
    let config = PrimConfig::default().with_start_node(start_node);
    Prim::with_config(engine, config).run().await
}
