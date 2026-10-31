// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Edmonds-Karp 알고리즘
//!
//! BFS 기반 Ford-Fulkerson 알고리즘입니다.
//! 최단 증가 경로를 사용하여 O(VE²) 시간복잡도를 보장합니다.

use super::network::FlowNetwork;
use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// Maximum Flow 알고리즘 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxFlowAlgorithm {
    EdmondsKarp,
    PushRelabel,
}

/// Maximum Flow 설정
#[derive(Debug, Clone, Default)]
pub struct MaxFlowConfig {
    /// 용량 속성 이름 (None이면 모든 간선 용량 1.0)
    pub capacity_property: Option<String>,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
}

impl MaxFlowConfig {
    /// 용량 속성 설정
    pub fn with_capacity_property(mut self, prop: String) -> Self {
        self.capacity_property = Some(prop);
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }
}

/// Maximum Flow 결과
#[derive(Debug, Clone)]
pub struct MaxFlowResult {
    /// 최대 유량 값
    pub max_flow: f64,
    /// 소스 노드
    pub source: NodeId,
    /// 싱크 노드
    pub sink: NodeId,
    /// 사용된 알고리즘
    pub algorithm: MaxFlowAlgorithm,
    /// 간선별 유량 (from, to, flow, capacity)
    pub edge_flows: Vec<(NodeId, NodeId, f64, f64)>,
    /// 증가 경로 수
    pub augmenting_paths: usize,
}

impl MaxFlowResult {
    /// 빈 결과 생성
    pub fn empty(source: NodeId, sink: NodeId, algorithm: MaxFlowAlgorithm) -> Self {
        Self {
            max_flow: 0.0,
            source,
            sink,
            algorithm,
            edge_flows: Vec::new(),
            augmenting_paths: 0,
        }
    }

    /// 포화 간선들 (유량 = 용량)
    pub fn saturated_edges(&self) -> Vec<(NodeId, NodeId)> {
        self.edge_flows
            .iter()
            .filter(|(_, _, flow, cap)| (*flow - *cap).abs() < 1e-9)
            .map(|(from, to, _, _)| (*from, *to))
            .collect()
    }

    /// 특정 간선의 유량
    pub fn flow_on_edge(&self, from: NodeId, to: NodeId) -> f64 {
        self.edge_flows
            .iter()
            .find(|(f, t, _, _)| *f == from && *t == to)
            .map(|(_, _, flow, _)| *flow)
            .unwrap_or(0.0)
    }
}

/// Edmonds-Karp 알고리즘
pub struct EdmondsKarp<'a> {
    engine: &'a GraphEngine,
    config: MaxFlowConfig,
}

impl<'a> EdmondsKarp<'a> {
    /// 새로운 EdmondsKarp 생성
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
                MaxFlowAlgorithm::EdmondsKarp,
            ));
        }

        // 유량 네트워크 구축
        let mut network = self.build_network().await?;

        if !network.contains_node(source) || !network.contains_node(sink) {
            return Ok(MaxFlowResult::empty(
                source,
                sink,
                MaxFlowAlgorithm::EdmondsKarp,
            ));
        }

        let mut max_flow = 0.0;
        let mut augmenting_paths = 0;

        // BFS로 증가 경로 찾기
        while let Some((path, bottleneck)) = self.find_augmenting_path(&network, source, sink) {
            // 경로를 따라 유량 증가
            for i in 0..path.len() - 1 {
                let from = path[i];
                let to = path[i + 1];

                // from -> to 간선 찾기
                if let Some(edges) = network.edges(from) {
                    for (idx, edge) in edges.iter().enumerate() {
                        if edge.to == to && edge.residual() > 0.0 {
                            network.push_flow(from, idx, bottleneck);
                            break;
                        }
                    }
                }
            }

            max_flow += bottleneck;
            augmenting_paths += 1;
        }

        // 결과 생성
        let edge_flows = network.get_flow_edges();

        Ok(MaxFlowResult {
            max_flow,
            source,
            sink,
            algorithm: MaxFlowAlgorithm::EdmondsKarp,
            edge_flows,
            augmenting_paths,
        })
    }

    /// BFS로 증가 경로 찾기
    fn find_augmenting_path(
        &self,
        network: &FlowNetwork,
        source: NodeId,
        sink: NodeId,
    ) -> Option<(Vec<NodeId>, f64)> {
        let mut visited = HashSet::new();
        let mut parent: HashMap<NodeId, (NodeId, usize, f64)> = HashMap::new();
        let mut queue = VecDeque::new();

        visited.insert(source);
        queue.push_back(source);

        while let Some(current) = queue.pop_front() {
            if current == sink {
                // 경로 재구성 및 bottleneck 계산
                let mut path = vec![sink];
                let mut node = sink;
                let mut bottleneck = f64::MAX;

                while let Some(&(prev, _edge_idx, residual)) = parent.get(&node) {
                    path.push(prev);
                    bottleneck = bottleneck.min(residual);
                    node = prev;
                }

                path.reverse();
                return Some((path, bottleneck));
            }

            if let Some(edges) = network.edges(current) {
                for (idx, edge) in edges.iter().enumerate() {
                    let residual = edge.residual();
                    if residual > 0.0 && !visited.contains(&edge.to) {
                        visited.insert(edge.to);
                        parent.insert(edge.to, (current, idx, residual));
                        queue.push_back(edge.to);
                    }
                }
            }
        }

        None
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

                // 중복 간선 체크 (같은 방향)
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

/// 최대 유량 편의 함수
pub async fn max_flow(engine: &GraphEngine, source: NodeId, sink: NodeId) -> Result<MaxFlowResult> {
    EdmondsKarp::new(engine).max_flow(source, sink).await
}

/// 용량 속성을 사용한 최대 유량
pub async fn max_flow_with_capacity(
    engine: &GraphEngine,
    source: NodeId,
    sink: NodeId,
    capacity_property: &str,
) -> Result<MaxFlowResult> {
    let config = MaxFlowConfig::default().with_capacity_property(capacity_property.to_string());
    EdmondsKarp::with_config(engine, config)
        .max_flow(source, sink)
        .await
}
