// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 인덱스 조회 메서드

use crate::core::{Edge, EdgeId, NodeId};
use crate::error::Result;

use super::EdgeStorage;

impl EdgeStorage {
    /// 특정 노드에서 나가는 엣지들 조회
    pub async fn get_outgoing_edges(&self, node_id: NodeId) -> Vec<EdgeId> {
        self.disk.read().await.get_outgoing_edges(node_id).await
    }

    /// 특정 노드로 들어오는 엣지들 조회
    pub async fn get_incoming_edges(&self, node_id: NodeId) -> Vec<EdgeId> {
        self.disk.read().await.get_incoming_edges(node_id).await
    }

    /// 특정 노드와 연결된 모든 엣지들 조회 (양방향)
    pub async fn get_connected_edges(&self, node_id: NodeId) -> Vec<EdgeId> {
        self.disk.read().await.get_connected_edges(node_id).await
    }

    /// 특정 관계 타입의 엣지들 조회
    pub async fn get_edges_by_type(&self, relation_type: &str) -> Vec<EdgeId> {
        self.disk
            .read()
            .await
            .get_edges_by_type(relation_type)
            .await
    }

    /// 두 노드 간의 직접 연결 엣지들 조회
    pub async fn get_edges_between(&self, from_node: NodeId, to_node: NodeId) -> Vec<EdgeId> {
        self.disk
            .read()
            .await
            .get_edges_between(from_node, to_node)
            .await
    }

    /// 특정 노드에서 특정 관계 타입으로 나가는 엣지들
    pub async fn get_outgoing_edges_by_type(
        &self,
        node_id: NodeId,
        relation_type: &str,
    ) -> Vec<EdgeId> {
        self.disk
            .read()
            .await
            .get_outgoing_edges_by_type(node_id, relation_type)
            .await
    }

    /// 노드와 연결된 엣지가 있는지 확인
    pub async fn has_connections(&self, node_id: NodeId) -> bool {
        self.disk.read().await.has_connections(node_id).await
    }

    /// 특정 노드와 관련된 모든 엣지 삭제
    pub async fn delete_node_edges(&self, node_id: NodeId) -> Result<Vec<Edge>> {
        let connected_edges = self.get_connected_edges(node_id).await;
        let mut deleted_edges = Vec::new();

        for edge_id in connected_edges {
            match self.remove(edge_id).await {
                Ok(edge) => deleted_edges.push(edge),
                Err(e) => return Err(e),
            }
        }

        Ok(deleted_edges)
    }
}
