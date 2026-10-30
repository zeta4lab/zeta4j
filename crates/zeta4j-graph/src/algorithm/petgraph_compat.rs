//! petgraph 호환 레이어 (v1.76.0)
//!
//! Zeta4j GraphEngine과 petgraph 라이브러리 간의 양방향 변환을 지원합니다.
//!
//! # 주요 기능
//!
//! - `GraphEngine` → `petgraph::Graph` 변환
//! - `petgraph::Graph` → `GraphEngine` 변환
//! - petgraph 알고리즘 직접 실행 지원
//! - 가중치 그래프 변환
//!
//! # 예제
//!
//! ```rust,ignore
//! use zeta4j_graph::engine::GraphEngine;
//! use zeta4j_graph::algorithm::petgraph_compat::{ToPetgraph, FromPetgraph};
//! use petgraph::algo::dijkstra;
//!
//! // GraphEngine → petgraph 변환
//! let engine = GraphEngine::new(Default::default())?;
//! // ... 노드/엣지 추가
//!
//! let pg = engine.to_petgraph().await?;
//!
//! // petgraph 알고리즘 사용
//! let distances = dijkstra(&pg, 0.into(), None, |e| *e.weight());
//!
//! // petgraph → GraphEngine 변환
//! let engine2 = GraphEngine::from_petgraph(&pg)?;
//! ```

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex, UnGraph};

use crate::core::{EdgeId, NodeId, PropertyContainer, Value};
use crate::engine::GraphEngine;

/// petgraph 변환 에러
#[derive(Debug, Clone, thiserror::Error)]
pub enum PetgraphError {
    /// 노드를 찾을 수 없음
    #[error("Node not found: {0}")]
    NodeNotFound(u64),

    /// 엣지를 찾을 수 없음
    #[error("Edge not found: {0}")]
    EdgeNotFound(u64),

    /// 변환 실패
    #[error("Conversion failed: {0}")]
    ConversionFailed(String),

    /// 가중치 없음
    #[error("Weight not found for edge: {0}")]
    WeightNotFound(u64),

    /// 그래프 엔진 에러
    #[error("Graph engine error: {0}")]
    EngineError(String),
}

impl From<crate::error::GraphError> for PetgraphError {
    fn from(e: crate::error::GraphError) -> Self {
        PetgraphError::EngineError(e.to_string())
    }
}

/// petgraph 변환 결과 타입
pub type PetgraphResult<T> = std::result::Result<T, PetgraphError>;

/// petgraph 노드 데이터
#[derive(Debug, Clone, Default)]
pub struct PetgraphNodeData {
    /// 원래 노드 ID
    pub original_id: NodeId,
    /// 라벨
    pub label: Option<String>,
    /// 프로퍼티 (이름)
    pub name: Option<String>,
}

/// petgraph 엣지 데이터
#[derive(Debug, Clone, Default)]
pub struct PetgraphEdgeData {
    /// 원래 엣지 ID
    pub original_id: EdgeId,
    /// 관계 타입
    pub rel_type: Option<String>,
    /// 가중치
    pub weight: f64,
}

/// petgraph로 변환 가능한 타입을 위한 트레이트
pub trait ToPetgraph {
    /// 유향 그래프로 변환
    fn to_petgraph_directed(
        &self,
    ) -> impl std::future::Future<
        Output = PetgraphResult<DiGraph<PetgraphNodeData, PetgraphEdgeData>>,
    > + Send;

    /// 무향 그래프로 변환
    fn to_petgraph_undirected(
        &self,
    ) -> impl std::future::Future<
        Output = PetgraphResult<UnGraph<PetgraphNodeData, PetgraphEdgeData>>,
    > + Send;

    /// 기본 유향 그래프 (편의 함수)
    fn to_petgraph(
        &self,
    ) -> impl std::future::Future<
        Output = PetgraphResult<DiGraph<PetgraphNodeData, PetgraphEdgeData>>,
    > + Send
    where
        Self: Sync,
    {
        async { self.to_petgraph_directed().await }
    }

    /// 가중치만 있는 유향 그래프
    fn to_petgraph_weighted(
        &self,
        weight_property: &str,
    ) -> impl std::future::Future<Output = PetgraphResult<DiGraph<NodeId, f64>>> + Send;
}

/// petgraph에서 변환 가능한 타입을 위한 트레이트
pub trait FromPetgraph: Sized {
    /// 유향 petgraph 그래프에서 생성
    fn from_petgraph_directed(
        graph: &DiGraph<PetgraphNodeData, PetgraphEdgeData>,
    ) -> PetgraphResult<Self>;

    /// 무향 petgraph 그래프에서 생성
    fn from_petgraph_undirected(
        graph: &UnGraph<PetgraphNodeData, PetgraphEdgeData>,
    ) -> PetgraphResult<Self>;

    /// 기본 유향 그래프에서 생성 (편의 함수)
    fn from_petgraph(graph: &DiGraph<PetgraphNodeData, PetgraphEdgeData>) -> PetgraphResult<Self> {
        Self::from_petgraph_directed(graph)
    }

    /// 가중치만 있는 그래프에서 생성
    fn from_petgraph_weighted(graph: &DiGraph<NodeId, f64>) -> PetgraphResult<Self>;
}

/// GraphEngine → petgraph 변환 구현
impl ToPetgraph for GraphEngine {
    async fn to_petgraph_directed(
        &self,
    ) -> PetgraphResult<DiGraph<PetgraphNodeData, PetgraphEdgeData>> {
        let mut pg = DiGraph::new();
        let mut node_map: HashMap<NodeId, NodeIndex> = HashMap::new();

        // 노드 추가
        let node_count = self.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if let Ok(Some(node)) = self.get_node(node_id).await {
                let data = PetgraphNodeData {
                    original_id: node.id(),
                    label: node.labels().iter().next().cloned(),
                    name: node.get_property("name").and_then(|v| {
                        if let Value::String(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    }),
                };
                let idx = pg.add_node(data);
                node_map.insert(node.id(), idx);
            }
        }

        // 엣지 추가
        let edge_count = self.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Ok(Some(edge)) = self.get_edge(edge_id).await {
                let source_idx = node_map
                    .get(&edge.from_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.from_node()))?;
                let target_idx = node_map
                    .get(&edge.to_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.to_node()))?;

                let weight = edge
                    .get_property("weight")
                    .and_then(|v| match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .unwrap_or(1.0);

                let data = PetgraphEdgeData {
                    original_id: edge.id(),
                    rel_type: Some(edge.relation_type().to_string()),
                    weight,
                };
                pg.add_edge(*source_idx, *target_idx, data);
            }
        }

        Ok(pg)
    }

    async fn to_petgraph_undirected(
        &self,
    ) -> PetgraphResult<UnGraph<PetgraphNodeData, PetgraphEdgeData>> {
        let mut pg = UnGraph::new_undirected();
        let mut node_map: HashMap<NodeId, NodeIndex> = HashMap::new();

        // 노드 추가
        let node_count = self.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if let Ok(Some(node)) = self.get_node(node_id).await {
                let data = PetgraphNodeData {
                    original_id: node.id(),
                    label: node.labels().iter().next().cloned(),
                    name: node.get_property("name").and_then(|v| {
                        if let Value::String(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    }),
                };
                let idx = pg.add_node(data);
                node_map.insert(node.id(), idx);
            }
        }

        // 엣지 추가 (중복 제거를 위해 정렬된 쌍 사용)
        let mut added_edges: std::collections::HashSet<(NodeId, NodeId)> =
            std::collections::HashSet::new();
        let edge_count = self.edge_count().await;

        for edge_id in 1..=edge_count as u64 {
            if let Ok(Some(edge)) = self.get_edge(edge_id).await {
                let pair = if edge.from_node() <= edge.to_node() {
                    (edge.from_node(), edge.to_node())
                } else {
                    (edge.to_node(), edge.from_node())
                };

                if added_edges.contains(&pair) {
                    continue;
                }
                added_edges.insert(pair);

                let source_idx = node_map
                    .get(&edge.from_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.from_node()))?;
                let target_idx = node_map
                    .get(&edge.to_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.to_node()))?;

                let weight = edge
                    .get_property("weight")
                    .and_then(|v| match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .unwrap_or(1.0);

                let data = PetgraphEdgeData {
                    original_id: edge.id(),
                    rel_type: Some(edge.relation_type().to_string()),
                    weight,
                };
                pg.add_edge(*source_idx, *target_idx, data);
            }
        }

        Ok(pg)
    }

    async fn to_petgraph_weighted(
        &self,
        weight_property: &str,
    ) -> PetgraphResult<DiGraph<NodeId, f64>> {
        let mut pg = DiGraph::new();
        let mut node_map: HashMap<NodeId, NodeIndex> = HashMap::new();

        // 노드 추가
        let node_count = self.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if let Ok(Some(node)) = self.get_node(node_id).await {
                let idx = pg.add_node(node.id());
                node_map.insert(node.id(), idx);
            }
        }

        // 엣지 추가
        let edge_count = self.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Ok(Some(edge)) = self.get_edge(edge_id).await {
                let source_idx = node_map
                    .get(&edge.from_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.from_node()))?;
                let target_idx = node_map
                    .get(&edge.to_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.to_node()))?;

                let weight = edge
                    .get_property(weight_property)
                    .and_then(|v| match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .unwrap_or(1.0);

                pg.add_edge(*source_idx, *target_idx, weight);
            }
        }

        Ok(pg)
    }
}

/// 노드 인덱스 매핑 (GraphEngine ID <-> petgraph NodeIndex)
#[derive(Debug, Clone, Default)]
pub struct NodeMapping {
    /// NodeId -> NodeIndex
    to_petgraph: HashMap<NodeId, NodeIndex>,
    /// NodeIndex -> NodeId
    from_petgraph: HashMap<NodeIndex, NodeId>,
}

impl NodeMapping {
    /// 새 매핑 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 매핑 추가
    pub fn insert(&mut self, node_id: NodeId, node_idx: NodeIndex) {
        self.to_petgraph.insert(node_id, node_idx);
        self.from_petgraph.insert(node_idx, node_id);
    }

    /// NodeId -> NodeIndex 변환
    pub fn to_index(&self, node_id: NodeId) -> Option<NodeIndex> {
        self.to_petgraph.get(&node_id).copied()
    }

    /// NodeIndex -> NodeId 변환
    pub fn to_id(&self, node_idx: NodeIndex) -> Option<NodeId> {
        self.from_petgraph.get(&node_idx).copied()
    }

    /// 매핑 수
    pub fn len(&self) -> usize {
        self.to_petgraph.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.to_petgraph.is_empty()
    }
}

/// 매핑과 함께 변환
pub struct GraphConverter;

impl GraphConverter {
    /// GraphEngine -> petgraph (매핑 포함)
    pub async fn to_petgraph_with_mapping(
        engine: &GraphEngine,
    ) -> PetgraphResult<(DiGraph<PetgraphNodeData, PetgraphEdgeData>, NodeMapping)> {
        let mut pg = DiGraph::new();
        let mut mapping = NodeMapping::new();

        // 노드 추가
        let node_count = engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if let Ok(Some(node)) = engine.get_node(node_id).await {
                let data = PetgraphNodeData {
                    original_id: node.id(),
                    label: node.labels().iter().next().cloned(),
                    name: node.get_property("name").and_then(|v| {
                        if let Value::String(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    }),
                };
                let idx = pg.add_node(data);
                mapping.insert(node.id(), idx);
            }
        }

        // 엣지 추가
        let edge_count = engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Ok(Some(edge)) = engine.get_edge(edge_id).await {
                let source_idx = mapping
                    .to_index(edge.from_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.from_node()))?;
                let target_idx = mapping
                    .to_index(edge.to_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.to_node()))?;

                let weight = edge
                    .get_property("weight")
                    .and_then(|v| match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .unwrap_or(1.0);

                let data = PetgraphEdgeData {
                    original_id: edge.id(),
                    rel_type: Some(edge.relation_type().to_string()),
                    weight,
                };
                pg.add_edge(source_idx, target_idx, data);
            }
        }

        Ok((pg, mapping))
    }

    /// 가중치 그래프로 변환 (매핑 포함)
    pub async fn to_weighted_with_mapping(
        engine: &GraphEngine,
        weight_property: &str,
    ) -> PetgraphResult<(DiGraph<NodeId, f64>, NodeMapping)> {
        let mut pg = DiGraph::new();
        let mut mapping = NodeMapping::new();

        // 노드 추가
        let node_count = engine.node_count().await?;
        for node_id in 1..=node_count as u64 {
            if let Ok(Some(node)) = engine.get_node(node_id).await {
                let idx = pg.add_node(node.id());
                mapping.insert(node.id(), idx);
            }
        }

        // 엣지 추가
        let edge_count = engine.edge_count().await;
        for edge_id in 1..=edge_count as u64 {
            if let Ok(Some(edge)) = engine.get_edge(edge_id).await {
                let source_idx = mapping
                    .to_index(edge.from_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.from_node()))?;
                let target_idx = mapping
                    .to_index(edge.to_node())
                    .ok_or(PetgraphError::NodeNotFound(edge.to_node()))?;

                let weight = edge
                    .get_property(weight_property)
                    .and_then(|v| match v {
                        Value::Integer(i) => Some(*i as f64),
                        Value::Float(f) => Some(*f),
                        _ => None,
                    })
                    .unwrap_or(1.0);

                pg.add_edge(source_idx, target_idx, weight);
            }
        }

        Ok((pg, mapping))
    }
}

/// petgraph 알고리즘 래퍼
pub mod algorithms {
    use super::*;
    use petgraph::algo;

    /// Dijkstra 최단 경로 (petgraph 사용)
    pub async fn dijkstra_petgraph(
        engine: &GraphEngine,
        start: NodeId,
        weight_property: &str,
    ) -> PetgraphResult<HashMap<NodeId, f64>> {
        let (pg, mapping) =
            GraphConverter::to_weighted_with_mapping(engine, weight_property).await?;

        let start_idx = mapping
            .to_index(start)
            .ok_or(PetgraphError::NodeNotFound(start))?;

        let result = algo::dijkstra(&pg, start_idx, None, |e| *e.weight());

        let mut distances = HashMap::new();
        for (idx, dist) in result {
            if let Some(node_id) = mapping.to_id(idx) {
                distances.insert(node_id, dist);
            }
        }

        Ok(distances)
    }

    /// Bellman-Ford 최단 경로 (petgraph 사용)
    pub async fn bellman_ford_petgraph(
        engine: &GraphEngine,
        start: NodeId,
        weight_property: &str,
    ) -> PetgraphResult<Option<HashMap<NodeId, f64>>> {
        let (pg, mapping) =
            GraphConverter::to_weighted_with_mapping(engine, weight_property).await?;

        let start_idx = mapping
            .to_index(start)
            .ok_or(PetgraphError::NodeNotFound(start))?;

        match algo::bellman_ford(&pg, start_idx) {
            Ok(result) => {
                let mut distances = HashMap::new();
                for (idx, dist) in result.distances.iter().enumerate() {
                    if let Some(node_id) = mapping.to_id(NodeIndex::new(idx)) {
                        distances.insert(node_id, *dist);
                    }
                }
                Ok(Some(distances))
            }
            Err(_) => Ok(None), // 음수 사이클 존재
        }
    }

    /// 토폴로지 정렬 (petgraph 사용)
    pub async fn topological_sort_petgraph(
        engine: &GraphEngine,
    ) -> PetgraphResult<Option<Vec<NodeId>>> {
        let (pg, mapping) = GraphConverter::to_petgraph_with_mapping(engine).await?;

        match algo::toposort(&pg, None) {
            Ok(sorted) => {
                let result: Vec<NodeId> = sorted
                    .iter()
                    .filter_map(|idx| mapping.to_id(*idx))
                    .collect();
                Ok(Some(result))
            }
            Err(_) => Ok(None), // 사이클 존재
        }
    }

    /// 사이클 검출 (petgraph 사용)
    pub async fn is_cyclic_petgraph(engine: &GraphEngine) -> PetgraphResult<bool> {
        let pg = engine.to_petgraph_directed().await?;
        Ok(algo::is_cyclic_directed(&pg))
    }

    /// 강연결 요소 (petgraph 사용, Kosaraju)
    pub async fn strongly_connected_petgraph(
        engine: &GraphEngine,
    ) -> PetgraphResult<Vec<Vec<NodeId>>> {
        let (pg, mapping) = GraphConverter::to_petgraph_with_mapping(engine).await?;

        let sccs = algo::kosaraju_scc(&pg);

        let result: Vec<Vec<NodeId>> = sccs
            .into_iter()
            .map(|scc| {
                scc.into_iter()
                    .filter_map(|idx| mapping.to_id(idx))
                    .collect()
            })
            .collect();

        Ok(result)
    }

    /// 연결 요소 수 (petgraph 사용)
    pub async fn connected_component_count_petgraph(engine: &GraphEngine) -> PetgraphResult<usize> {
        let pg = engine.to_petgraph_undirected().await?;
        Ok(algo::connected_components(&pg))
    }
}

/// 그래프 통계 (petgraph 사용)
pub mod stats {
    use super::*;

    /// 그래프 기본 통계
    #[derive(Debug, Clone, Default)]
    pub struct GraphStats {
        /// 노드 수
        pub node_count: usize,
        /// 엣지 수
        pub edge_count: usize,
        /// 밀도
        pub density: f64,
        /// 평균 차수
        pub average_degree: f64,
    }

    /// GraphEngine 통계 계산
    pub async fn compute_stats(engine: &GraphEngine) -> PetgraphResult<GraphStats> {
        let pg = engine.to_petgraph_directed().await?;

        let node_count = pg.node_count();
        let edge_count = pg.edge_count();

        let density = if node_count > 1 {
            edge_count as f64 / (node_count * (node_count - 1)) as f64
        } else {
            0.0
        };

        let average_degree = if node_count > 0 {
            2.0 * edge_count as f64 / node_count as f64
        } else {
            0.0
        };

        Ok(GraphStats {
            node_count,
            edge_count,
            density,
            average_degree,
        })
    }
}
