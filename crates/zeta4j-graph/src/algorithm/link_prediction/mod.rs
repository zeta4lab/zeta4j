//! 링크 예측 알고리즘
//!
//! 그래프에서 두 노드 사이에 링크가 생성될 가능성을 예측합니다.
//!
//! - Common Neighbors: 공통 이웃 수
//! - Adamic-Adar Index: 공통 이웃의 degree 역로그 가중치
//! - Preferential Attachment: 두 노드 degree의 곱
//! - Resource Allocation: 공통 이웃의 degree 역수 가중치
//! - Total Neighbors: 이웃 합집합 크기

mod adamic_adar;
mod common_neighbors;
mod preferential_attachment;
mod resource_allocation;
mod total_neighbors;

pub use adamic_adar::{AdamicAdar, AdamicAdarConfig};
pub use common_neighbors::{CommonNeighbors, CommonNeighborsConfig};
pub use preferential_attachment::{PreferentialAttachment, PreferentialAttachmentConfig};
pub use resource_allocation::{ResourceAllocation, ResourceAllocationConfig};
pub use total_neighbors::{TotalNeighbors, TotalNeighborsConfig};

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashSet;

/// 링크 예측 결과
#[derive(Debug, Clone)]
pub struct LinkPredictionResult {
    /// 소스 노드
    pub source: NodeId,
    /// 타겟 노드 (예측된 링크 후보)
    pub target: NodeId,
    /// 예측 점수
    pub score: f64,
}

impl LinkPredictionResult {
    /// 새로운 링크 예측 결과 생성
    pub fn new(source: NodeId, target: NodeId, score: f64) -> Self {
        Self {
            source,
            target,
            score,
        }
    }
}

/// 노드의 이웃 집합을 가져오는 공통 함수
///
/// 유사도 모듈의 함수와 동일한 로직 사용
pub async fn get_neighbors(
    engine: &GraphEngine,
    node_id: NodeId,
    rel_types: &Option<Vec<String>>,
    directed: bool,
) -> Result<HashSet<NodeId>> {
    let mut neighbors = HashSet::new();

    let edge_ids = engine.get_connected_edges(node_id).await;

    for edge_id in edge_ids {
        if let Some(edge) = engine.get_edge(edge_id).await? {
            // 관계 타입 필터링
            if let Some(types) = rel_types
                && !types.contains(&edge.relation_type().to_string())
            {
                continue;
            }

            let from = edge.from_node();
            let to = edge.to_node();

            if directed {
                // 방향 고려: 나가는 엣지만
                if from == node_id {
                    neighbors.insert(to);
                }
            } else {
                // 양방향: 연결된 모든 노드
                if from == node_id {
                    neighbors.insert(to);
                } else if to == node_id {
                    neighbors.insert(from);
                }
            }
        }
    }

    Ok(neighbors)
}

/// 모든 노드 ID 목록 가져오기
pub async fn get_all_node_ids(engine: &GraphEngine) -> Result<Vec<NodeId>> {
    let mut nodes = Vec::new();
    let node_count = engine.node_count().await?;

    for node_id in 1..=node_count as u64 {
        if engine.get_node(node_id).await?.is_some() {
            nodes.push(node_id);
        }
    }

    Ok(nodes)
}

/// 두 노드가 이미 연결되어 있는지 확인
pub async fn are_connected(
    engine: &GraphEngine,
    node1: NodeId,
    node2: NodeId,
    rel_types: &Option<Vec<String>>,
) -> Result<bool> {
    let neighbors = get_neighbors(engine, node1, rel_types, false).await?;
    Ok(neighbors.contains(&node2))
}

/// 링크 예측 메트릭 종류
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LinkPredictionMetric {
    /// Common Neighbors
    #[default]
    CommonNeighbors,
    /// Adamic-Adar Index
    AdamicAdar,
    /// Preferential Attachment
    PreferentialAttachment,
    /// Resource Allocation Index
    ResourceAllocation,
    /// Total Neighbors
    TotalNeighbors,
}

/// 통합 링크 예측기
///
/// 다양한 메트릭을 사용하여 링크 예측을 수행합니다.
pub struct LinkPredictor<'a> {
    engine: &'a GraphEngine,
    /// 사용할 메트릭
    pub metric: LinkPredictionMetric,
    /// 관계 타입 필터
    pub relationship_types: Option<Vec<String>>,
    /// 방향 고려 여부
    pub directed: bool,
}

impl<'a> LinkPredictor<'a> {
    /// 새로운 LinkPredictor 생성
    pub fn new(engine: &'a GraphEngine) -> Self {
        Self {
            engine,
            metric: LinkPredictionMetric::default(),
            relationship_types: None,
            directed: false,
        }
    }

    /// 메트릭 설정
    pub fn with_metric(mut self, metric: LinkPredictionMetric) -> Self {
        self.metric = metric;
        self
    }

    /// 관계 타입 필터 설정
    pub fn with_relationship_types(mut self, types: Vec<String>) -> Self {
        self.relationship_types = Some(types);
        self
    }

    /// 방향 설정
    pub fn with_directed(mut self, directed: bool) -> Self {
        self.directed = directed;
        self
    }

    /// 두 노드 간 점수 계산
    pub async fn score(&self, node1: NodeId, node2: NodeId) -> Result<f64> {
        match self.metric {
            LinkPredictionMetric::CommonNeighbors => {
                let cn = CommonNeighbors::with_config(
                    self.engine,
                    CommonNeighborsConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                cn.score(node1, node2).await
            }
            LinkPredictionMetric::AdamicAdar => {
                let aa = AdamicAdar::with_config(
                    self.engine,
                    AdamicAdarConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                aa.score(node1, node2).await
            }
            LinkPredictionMetric::PreferentialAttachment => {
                let pa = PreferentialAttachment::with_config(
                    self.engine,
                    PreferentialAttachmentConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                pa.score(node1, node2).await
            }
            LinkPredictionMetric::ResourceAllocation => {
                let ra = ResourceAllocation::with_config(
                    self.engine,
                    ResourceAllocationConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                ra.score(node1, node2).await
            }
            LinkPredictionMetric::TotalNeighbors => {
                let tn = TotalNeighbors::with_config(
                    self.engine,
                    TotalNeighborsConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                tn.score(node1, node2).await
            }
        }
    }

    /// 특정 노드의 상위 K개 링크 예측 후보
    pub async fn predict(&self, source: NodeId, top_k: usize) -> Result<Vec<LinkPredictionResult>> {
        match self.metric {
            LinkPredictionMetric::CommonNeighbors => {
                let cn = CommonNeighbors::with_config(
                    self.engine,
                    CommonNeighborsConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                cn.predict(source, top_k).await
            }
            LinkPredictionMetric::AdamicAdar => {
                let aa = AdamicAdar::with_config(
                    self.engine,
                    AdamicAdarConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                aa.predict(source, top_k).await
            }
            LinkPredictionMetric::PreferentialAttachment => {
                let pa = PreferentialAttachment::with_config(
                    self.engine,
                    PreferentialAttachmentConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                pa.predict(source, top_k).await
            }
            LinkPredictionMetric::ResourceAllocation => {
                let ra = ResourceAllocation::with_config(
                    self.engine,
                    ResourceAllocationConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                ra.predict(source, top_k).await
            }
            LinkPredictionMetric::TotalNeighbors => {
                let tn = TotalNeighbors::with_config(
                    self.engine,
                    TotalNeighborsConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                tn.predict(source, top_k).await
            }
        }
    }

    /// 전체 노드 쌍의 점수 계산 (임계값 이상)
    pub async fn calculate_all(&self, threshold: f64) -> Result<Vec<LinkPredictionResult>> {
        match self.metric {
            LinkPredictionMetric::CommonNeighbors => {
                let cn = CommonNeighbors::with_config(
                    self.engine,
                    CommonNeighborsConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                cn.calculate_all(threshold).await
            }
            LinkPredictionMetric::AdamicAdar => {
                let aa = AdamicAdar::with_config(
                    self.engine,
                    AdamicAdarConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                aa.calculate_all(threshold).await
            }
            LinkPredictionMetric::PreferentialAttachment => {
                let pa = PreferentialAttachment::with_config(
                    self.engine,
                    PreferentialAttachmentConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                pa.calculate_all(threshold).await
            }
            LinkPredictionMetric::ResourceAllocation => {
                let ra = ResourceAllocation::with_config(
                    self.engine,
                    ResourceAllocationConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                ra.calculate_all(threshold).await
            }
            LinkPredictionMetric::TotalNeighbors => {
                let tn = TotalNeighbors::with_config(
                    self.engine,
                    TotalNeighborsConfig {
                        relationship_types: self.relationship_types.clone(),
                        directed: self.directed,
                    },
                );
                tn.calculate_all(threshold).await
            }
        }
    }
}
