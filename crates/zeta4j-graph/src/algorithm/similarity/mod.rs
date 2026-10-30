//! 유사도 알고리즘
//!
//! 그래프에서 노드 간 유사도를 측정하는 알고리즘을 제공합니다.
//!
//! - Jaccard Similarity: 이웃 집합의 교집합/합집합
//! - Overlap Coefficient: 교집합/작은 집합 크기
//! - Cosine Similarity: 코사인 유사도 (이웃 기반 또는 속성 기반)
//! - Node Similarity: 전체 노드에 대한 top-K 유사 노드 계산

mod cosine;
mod jaccard;
mod node_similarity;
mod overlap;

pub use jaccard::{JaccardConfig, JaccardSimilarity, SimilarityResult};

pub use overlap::{OverlapCoefficient, OverlapConfig};

pub use cosine::{CosineConfig, CosineSimilarity};

pub use node_similarity::{
    NodeSimilarity, NodeSimilarityConfig, NodeSimilarityResult, SimilarityMetric,
};

use crate::core::NodeId;
use crate::engine::GraphEngine;
use crate::error::Result;
use std::collections::HashSet;

/// 노드의 이웃 집합을 가져오는 공통 함수
///
/// # Arguments
/// * `engine` - 그래프 엔진
/// * `node_id` - 대상 노드 ID
/// * `rel_types` - 고려할 관계 타입 (None이면 모든 타입)
/// * `directed` - 방향 고려 여부 (false면 양방향)
///
/// # Returns
/// 이웃 노드 ID 집합
pub async fn get_neighbors(
    engine: &GraphEngine,
    node_id: NodeId,
    rel_types: &Option<Vec<String>>,
    directed: bool,
) -> Result<HashSet<NodeId>> {
    let mut neighbors = HashSet::new();

    // 연결된 모든 엣지 조회
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
                // 방향 고려: 나가는 엣지만 (현재 노드가 source인 경우)
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
