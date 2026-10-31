// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Push-Relabel 알고리즘
//!
//! Preflow-Push 기반 최대 유량 알고리즘입니다.
//! 높이 함수를 사용하여 O(V²E) 시간복잡도를 달성합니다.

use super::edmonds_karp::{MaxFlowAlgorithm, MaxFlowConfig, MaxFlowResult};
use super::network::FlowNetwork;
use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// Push-Relabel 알고리즘
pub struct PushRelabel<'a> {
    engine: &'a GraphEngine,
    config: MaxFlowConfig,
}

impl<'a> PushRelabel<'a> {
    /// 새로운 PushRelabel 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            config: MaxFlowConfig::default(),
        }
    }

    /// 설정과 함께 생성
    pub fn with_config(engine: &'a GraphEngine, config: MaxFlowConfig) -> Self {
        Self { engine, config }
    }

    /// 최대 유량 계산
    pub async fn max_flow(&self, source: NodeId, sink: NodeId) -> Result<MaxFlowResult> {
        if source == sink {
            return Ok(MaxFlowResult::empty(
                source,
                sink,
                MaxFlowAlgorithm::PushRelabel,
            ));
        }

        // 유량 네트워크 구축
        let mut network = self.build_network().await?;

        if !network.contains_node(source) || !network.contains_node(sink) {
            return Ok(MaxFlowResult::empty(
                source,
                sink,
                MaxFlowAlgorithm::PushRelabel,
            ));
        }

        let nodes: Vec<NodeId> = network.nodes().to_vec();
        let n = nodes.len();

        // 높이와 초과 유량 초기화
        let mut height: HashMap<NodeId, usize> = HashMap::new();
        let mut excess: HashMap<NodeId, f64> = HashMap::new();

        for &node in &nodes {
            height.insert(node, 0);
            excess.insert(node, 0.0);
        }

        // 소스 높이 = n
        height.insert(source, n);

        // 소스에서 모든 이웃에 유량 밀어내기 (preflow 초기화)
        if let Some(edges) = network.edges(source) {
            let edge_indices: Vec<(usize, NodeId, f64)> = edges
                .iter()
                .enumerate()
                .map(|(i, e)| (i, e.to, e.capacity))
                .collect();

            for (idx, to, capacity) in edge_indices {
                if capacity > 0.0 {
                    network.push_flow(source, idx, capacity);
                    *excess.entry(to).or_insert(0.0) += capacity;
                    *excess.entry(source).or_insert(0.0) -= capacity;
                }
            }
        }

        // 활성 노드 큐 (excess > 0이고 source/sink가 아닌 노드)
        let mut active: VecDeque<NodeId> = VecDeque::new();
        for &node in &nodes {
            if node != source && node != sink && excess[&node] > 1e-9 {
                active.push_back(node);
            }
        }

        let mut in_queue: HashSet<NodeId> = active.iter().cloned().collect();

        // Push-Relabel 루프
        while let Some(u) = active.pop_front() {
            in_queue.remove(&u);

            if excess[&u] <= 1e-9 {
                continue;
            }

            // Push 또는 Relabel
            let mut pushed = false;

            if let Some(edges) = network.edges(u) {
                let edge_data: Vec<(usize, NodeId, f64)> = edges
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (i, e.to, e.residual()))
                    .collect();

                for (idx, v, residual) in edge_data {
                    if residual > 1e-9 && height[&u] == height[&v] + 1 {
                        // Push
                        let delta = excess[&u].min(residual);
                        network.push_flow(u, idx, delta);
                        *excess.entry(u).or_insert(0.0) -= delta;
                        *excess.entry(v).or_insert(0.0) += delta;

                        if v != source && v != sink && !in_queue.contains(&v) && excess[&v] > 1e-9 {
                            active.push_back(v);
                            in_queue.insert(v);
                        }

                        pushed = true;

                        if excess[&u] <= 1e-9 {
                            break;
                        }
                    }
                }
            }

            // Relabel if couldn't push
            if !pushed && excess[&u] > 1e-9 {
                let mut min_height = usize::MAX;

                if let Some(edges) = network.edges(u) {
                    for edge in edges {
                        if edge.residual() > 1e-9 {
                            min_height = min_height.min(height[&edge.to]);
                        }
                    }
                }

                if min_height < usize::MAX {
                    height.insert(u, min_height + 1);
                    active.push_back(u);
                    in_queue.insert(u);
                }
            } else if excess[&u] > 1e-9 {
                // 아직 excess가 남아있으면 다시 큐에 추가
                active.push_back(u);
                in_queue.insert(u);
            }
        }

        // 결과 생성
        let max_flow_value = excess[&sink];
        let edge_flows = network.get_flow_edges();

        Ok(MaxFlowResult {
            max_flow: max_flow_value,
            source,
            sink,
            algorithm: MaxFlowAlgorithm::PushRelabel,
            edge_flows,
            augmenting_paths: 0, // Push-Relabel은 증가 경로 개념이 없음
        })
    }

    /// 그래프에서 유량 네트워크 구축
    async fn build_network(&self) -> Result<FlowNetwork> {
        let mut network = FlowNetwork::new();
        let mut seen_edges = HashSet::new();

        let node_count = self.engine.node_count().await?;
        let edge_count = self.engine.edge_count().await;

        // 노드 추가
        for node_id in 1..=node_count as u64 {
            if self.engine.get_node(node_id).await?.is_some() {
                network.add_node(node_id);
            }
        }

        // 간선 추가
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

                // 중복 간선 체크
                let edge_key = (from, to);
                if seen_edges.contains(&edge_key) {
                    continue;
                }
                seen_edges.insert(edge_key);

                // 용량 결정
                let capacity = if let Some(ref prop_name) = self.config.capacity_property {
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

                network.add_edge(from, to, capacity);
            }
        }

        Ok(network)
    }
}

/// Push-Relabel 최대 유량 편의 함수
pub async fn max_flow_pr(
    engine: &GraphEngine,
    source: NodeId,
    sink: NodeId,
) -> Result<MaxFlowResult> {
    PushRelabel::new(engine).max_flow(source, sink).await
}

/// 용량 속성을 사용한 Push-Relabel 최대 유량
pub async fn max_flow_pr_with_capacity(
    engine: &GraphEngine,
    source: NodeId,
    sink: NodeId,
    capacity_property: &str,
) -> Result<MaxFlowResult> {
    let config = MaxFlowConfig::default().with_capacity_property(capacity_property.to_string());
    PushRelabel::with_config(engine, config)
        .max_flow(source, sink)
        .await
}
