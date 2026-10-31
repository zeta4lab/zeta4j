// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! GraphView 트레이트 - 알고리즘용 그래프 추상화
//!
//! 알고리즘이 `&GraphEngine` 직접 참조 대신 트레이트를 통해 접근하도록 합니다.
//! 이를 통해 테스트, 서브그래프 뷰, 분산 그래프 등 다양한 구현체를 지원할 수 있습니다.

use std::future::Future;

use crate::core::{Edge, EdgeId, Node, NodeId};
use crate::error::Result;

/// 그래프 읽기 전용 뷰 트레이트
///
/// 알고리즘이 그래프 데이터에 접근하기 위한 최소 인터페이스를 정의합니다.
/// `GraphEngine`뿐만 아니라 서브그래프 뷰, 필터링된 뷰, 분산 그래프 등
/// 다양한 구현체를 지원합니다.
///
/// # 사용 예시
///
/// ```ignore
/// use zeta4j_graph::engine::GraphView;
///
/// async fn calculate_degree<G: GraphView>(graph: &G, node_id: NodeId) -> Result<usize> {
///     let edges = graph.find_edges_by_node(node_id).await?;
///     Ok(edges.len())
/// }
/// ```
pub trait GraphView: Send + Sync {
    /// 그래프 내 노드 개수
    fn node_count(&self) -> impl Future<Output = Result<usize>> + Send;

    /// 그래프 내 엣지 개수
    fn edge_count(&self) -> impl Future<Output = usize> + Send;

    /// 노드 ID로 노드 조회
    fn get_node(&self, node_id: NodeId) -> impl Future<Output = Result<Option<Node>>> + Send;

    /// 엣지 ID로 엣지 조회
    fn get_edge(&self, edge_id: EdgeId) -> impl Future<Output = Result<Option<Edge>>> + Send;

    /// 노드와 연결된 모든 엣지 ID 조회 (양방향)
    fn find_edges_by_node(
        &self,
        node_id: NodeId,
    ) -> impl Future<Output = Result<Vec<EdgeId>>> + Send;

    /// 노드에서 나가는 엣지 ID 조회
    fn find_outgoing_edges(&self, node_id: NodeId) -> impl Future<Output = Vec<EdgeId>> + Send;

    /// 노드로 들어오는 엣지 ID 조회
    fn find_incoming_edges(&self, node_id: NodeId) -> impl Future<Output = Vec<EdgeId>> + Send;

    /// 모든 노드 ID 반환
    fn all_node_ids(&self) -> impl Future<Output = Vec<NodeId>> + Send;

    /// 모든 엣지 ID 반환
    fn all_edge_ids(&self) -> impl Future<Output = Vec<EdgeId>> + Send;

    /// 노드 존재 여부 확인
    fn contains_node(&self, node_id: NodeId) -> impl Future<Output = Result<bool>> + Send {
        async move { Ok(self.get_node(node_id).await?.is_some()) }
    }

    /// 엣지 존재 여부 확인
    fn contains_edge(&self, edge_id: EdgeId) -> impl Future<Output = Result<bool>> + Send {
        async move { Ok(self.get_edge(edge_id).await?.is_some()) }
    }

    /// 두 노드 사이의 엣지 ID 조회
    fn find_edges_between(
        &self,
        from: NodeId,
        to: NodeId,
    ) -> impl Future<Output = Vec<EdgeId>> + Send;

    /// 노드의 이웃 노드 ID 조회 (나가는 방향)
    fn get_neighbors(&self, node_id: NodeId) -> impl Future<Output = Result<Vec<NodeId>>> + Send {
        async move {
            let edges = self.find_outgoing_edges(node_id).await;
            let mut neighbors = Vec::with_capacity(edges.len());
            for edge_id in edges {
                if let Some(edge) = self.get_edge(edge_id).await? {
                    neighbors.push(edge.to_node());
                }
            }
            Ok(neighbors)
        }
    }

    /// 노드의 역방향 이웃 노드 ID 조회 (들어오는 방향)
    fn get_predecessors(
        &self,
        node_id: NodeId,
    ) -> impl Future<Output = Result<Vec<NodeId>>> + Send {
        async move {
            let edges = self.find_incoming_edges(node_id).await;
            let mut predecessors = Vec::with_capacity(edges.len());
            for edge_id in edges {
                if let Some(edge) = self.get_edge(edge_id).await? {
                    predecessors.push(edge.from_node());
                }
            }
            Ok(predecessors)
        }
    }

    /// 노드의 차수 (나가는 엣지 수)
    fn out_degree(&self, node_id: NodeId) -> impl Future<Output = usize> + Send {
        async move { self.find_outgoing_edges(node_id).await.len() }
    }

    /// 노드의 입차수 (들어오는 엣지 수)
    fn in_degree(&self, node_id: NodeId) -> impl Future<Output = usize> + Send {
        async move { self.find_incoming_edges(node_id).await.len() }
    }

    /// 노드의 전체 차수 (나가는 + 들어오는 엣지 수, self-loop는 2로 계산)
    fn degree(&self, node_id: NodeId) -> impl Future<Output = usize> + Send {
        async move { self.out_degree(node_id).await + self.in_degree(node_id).await }
    }
}
