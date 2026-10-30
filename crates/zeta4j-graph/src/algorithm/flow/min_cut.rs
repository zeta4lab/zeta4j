//! 최소 컷 (Min-Cut) 알고리즘
//!
//! 최대 유량 = 최소 컷 (Max-Flow Min-Cut Theorem)을 활용하여
//! 그래프의 최소 컷을 계산합니다.

use super::edmonds_karp::{EdmondsKarp, MaxFlowConfig};
use super::network::FlowNetwork;
use crate::core::{NodeId, PropertyContainer};
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::{HashMap, HashSet, VecDeque};

/// 최소 컷 결과
#[derive(Debug, Clone)]
pub struct MinCutResult {
    /// 컷 값 (= 최대 유량)
    pub cut_value: f64,
    /// 소스 측 노드들
    pub source_side: Vec<NodeId>,
    /// 싱크 측 노드들
    pub sink_side: Vec<NodeId>,
    /// 컷 간선들 (from, to, capacity)
    pub cut_edges: Vec<(NodeId, NodeId, f64)>,
    /// 소스 노드
    pub source: NodeId,
    /// 싱크 노드
    pub sink: NodeId,
}

impl MinCutResult {
    /// 빈 결과 생성
    pub fn empty(source: NodeId, sink: NodeId) -> Self {
        Self {
            cut_value: 0.0,
            source_side: vec![source],
            sink_side: vec![sink],
            cut_edges: Vec::new(),
            source,
            sink,
        }
    }

    /// 컷 간선 수
    pub fn cut_edge_count(&self) -> usize {
        self.cut_edges.len()
    }

    /// 소스 측 노드 수
    pub fn source_side_count(&self) -> usize {
        self.source_side.len()
    }

    /// 싱크 측 노드 수
    pub fn sink_side_count(&self) -> usize {
        self.sink_side.len()
    }

    /// 특정 노드가 소스 측에 있는지
    pub fn is_on_source_side(&self, node: NodeId) -> bool {
        self.source_side.contains(&node)
    }

    /// 특정 노드가 싱크 측에 있는지
    pub fn is_on_sink_side(&self, node: NodeId) -> bool {
        self.sink_side.contains(&node)
    }
}

/// 최소 컷 계산기
pub struct MinCut<'a> {
    engine: &'a GraphEngine,
    config: MaxFlowConfig,
}

impl<'a> MinCut<'a> {
    /// 새로운 MinCut 생성
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

    /// 최소 컷 계산
    pub async fn find_min_cut(&self, source: NodeId, sink: NodeId) -> Result<MinCutResult> {
        if source == sink {
            return Ok(MinCutResult::empty(source, sink));
        }

        // 먼저 최대 유량 계산
        let ek = EdmondsKarp::with_config(self.engine, self.config.clone());
        let max_flow_result = ek.max_flow(source, sink).await?;

        if max_flow_result.max_flow == 0.0 {
            return Ok(MinCutResult::empty(source, sink));
        }

        // 유량 네트워크 재구축하고 잔여 그래프에서 소스에서 도달 가능한 노드 찾기
        let network = self.build_network_with_flow(&max_flow_result).await?;

        // BFS로 소스에서 도달 가능한 노드 찾기
        let source_side = self.find_reachable(&network, source);

        // 싱크 측 = 전체 노드 - 소스 측
        let all_nodes: HashSet<NodeId> = network.nodes().iter().cloned().collect();
        let source_set: HashSet<NodeId> = source_side.iter().cloned().collect();
        let sink_side: Vec<NodeId> = all_nodes.difference(&source_set).cloned().collect();

        // 컷 간선 찾기 (소스 측 -> 싱크 측으로 가는 포화 간선)
        let mut cut_edges = Vec::new();
        for &from in &source_side {
            if let Some(edges) = network.edges(from) {
                for edge in edges {
                    if !source_set.contains(&edge.to) && edge.capacity > 0.0 {
                        cut_edges.push((from, edge.to, edge.capacity));
                    }
                }
            }
        }

        Ok(MinCutResult {
            cut_value: max_flow_result.max_flow,
            source_side,
            sink_side,
            cut_edges,
            source,
            sink,
        })
    }

    /// 잔여 그래프에서 도달 가능한 노드 찾기
    fn find_reachable(&self, network: &FlowNetwork, source: NodeId) -> Vec<NodeId> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        visited.insert(source);
        queue.push_back(source);

        while let Some(current) = queue.pop_front() {
            if let Some(edges) = network.edges(current) {
                for edge in edges {
                    if edge.residual() > 1e-9 && !visited.contains(&edge.to) {
                        visited.insert(edge.to);
                        queue.push_back(edge.to);
                    }
                }
            }
        }

        visited.into_iter().collect()
    }

    /// 유량이 적용된 네트워크 구축
    async fn build_network_with_flow(
        &self,
        max_flow_result: &super::edmonds_karp::MaxFlowResult,
    ) -> Result<FlowNetwork> {
        let mut network = FlowNetwork::new();
        let mut seen_edges = HashSet::new();

        // 유량 맵 생성
        let mut flow_map: HashMap<(NodeId, NodeId), f64> = HashMap::new();
        for &(from, to, flow, _) in &max_flow_result.edge_flows {
            flow_map.insert((from, to), flow);
        }

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

                let edge_key = (from, to);
                if seen_edges.contains(&edge_key) {
                    continue;
                }
                seen_edges.insert(edge_key);

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

        // 유량 적용
        for (&(from, to), &flow) in &flow_map {
            if let Some(edges) = network.edges(from) {
                for (idx, edge) in edges.iter().enumerate() {
                    if edge.to == to {
                        network.push_flow(from, idx, flow);
                        break;
                    }
                }
            }
        }

        Ok(network)
    }
}

/// 최소 컷 편의 함수
pub async fn min_cut(engine: &GraphEngine, source: NodeId, sink: NodeId) -> Result<MinCutResult> {
    MinCut::new(engine).find_min_cut(source, sink).await
}

/// 용량 속성을 사용한 최소 컷
pub async fn min_cut_with_capacity(
    engine: &GraphEngine,
    source: NodeId,
    sink: NodeId,
    capacity_property: &str,
) -> Result<MinCutResult> {
    let config = MaxFlowConfig::default().with_capacity_property(capacity_property.to_string());
    MinCut::with_config(engine, config)
        .find_min_cut(source, sink)
        .await
}
