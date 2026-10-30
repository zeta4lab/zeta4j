//! GraphEngine 기반 CypherBackend 구현
//!
//! 서버가 사용하는 GraphEngine 래퍼입니다.

use std::any::Any;
use std::sync::Arc;

use zeta4j_graph::Result;
use zeta4j_graph::core::{Edge, EdgeId, Node, NodeId};
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::transaction::TransactionId;

use zeta4j_cypher_core::CypherBackend;

/// GraphEngine 기반 CypherBackend 구현
///
/// 제품의 GraphEngine 기능을 직접 래핑합니다.
/// 완전한 ACID 트랜잭션과 MVCC 동시성 제어를 지원합니다.
pub struct GraphEngineBackend {
    engine: Arc<GraphEngine>,
}

impl GraphEngineBackend {
    /// 새 GraphEngineBackend 생성
    pub fn new(engine: Arc<GraphEngine>) -> Self {
        Self { engine }
    }

    /// 내부 GraphEngine 참조 반환
    ///
    /// 일부 고급 기능(스키마 관리, 알고리즘 등)에서 직접 접근이 필요할 때 사용합니다.
    pub fn engine(&self) -> &Arc<GraphEngine> {
        &self.engine
    }
}

#[async_trait::async_trait]
impl CypherBackend for GraphEngineBackend {
    // =========================================================================
    // 트랜잭션 관리
    // =========================================================================

    fn begin_transaction(&self) -> Result<TransactionId> {
        self.engine.begin_transaction()
    }

    fn commit_transaction(&self, tx_id: TransactionId) -> Result<()> {
        self.engine.commit_transaction(tx_id)
    }

    fn rollback_transaction(&self, tx_id: TransactionId) -> Result<()> {
        self.engine.rollback_transaction(tx_id)
    }

    // =========================================================================
    // 노드 CRUD
    // =========================================================================

    async fn create_node_internal(
        &self,
        tx_id: TransactionId,
        labels: Vec<String>,
    ) -> Result<NodeId> {
        self.engine.create_node_internal(tx_id, labels).await
    }

    async fn get_node(&self, node_id: NodeId) -> Result<Option<Node>> {
        self.engine.get_node(node_id).await
    }

    async fn update_node_internal(
        &self,
        tx_id: TransactionId,
        node_id: NodeId,
        update_fn: Box<dyn for<'a> FnOnce(&'a mut Node) + Send + 'static>,
    ) -> Result<()> {
        self.engine
            .update_node_internal(tx_id, node_id, update_fn)
            .await
    }

    async fn delete_node_internal(
        &self,
        tx_id: TransactionId,
        node_id: NodeId,
        detach: bool,
    ) -> Result<()> {
        self.engine
            .delete_node_internal(tx_id, node_id, detach)
            .await
    }

    // =========================================================================
    // 엣지 CRUD
    // =========================================================================

    async fn create_edge_internal(
        &self,
        tx_id: TransactionId,
        relation_type: String,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Result<EdgeId> {
        self.engine
            .create_edge_internal(tx_id, relation_type, from_node, to_node)
            .await
    }

    async fn get_edge(&self, edge_id: EdgeId) -> Result<Option<Edge>> {
        self.engine.get_edge(edge_id).await
    }

    async fn update_edge_internal(
        &self,
        tx_id: TransactionId,
        edge_id: EdgeId,
        update_fn: Box<dyn for<'a> FnOnce(&'a mut Edge) + Send + 'static>,
    ) -> Result<()> {
        self.engine
            .update_edge_internal(tx_id, edge_id, update_fn)
            .await
    }

    async fn delete_edge_internal(&self, tx_id: TransactionId, edge_id: EdgeId) -> Result<()> {
        self.engine.delete_edge_internal(tx_id, edge_id).await
    }

    // =========================================================================
    // 조회
    // =========================================================================

    async fn find_nodes_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
        self.engine.find_nodes_by_label(label).await
    }

    async fn get_connected_edges(&self, node_id: NodeId) -> Vec<EdgeId> {
        self.engine.get_connected_edges(node_id).await
    }

    async fn all_node_ids(&self) -> Vec<NodeId> {
        self.engine.all_node_ids().await
    }

    async fn all_edge_ids(&self) -> Vec<EdgeId> {
        self.engine.all_edge_ids().await
    }

    // =========================================================================
    // 인덱스
    // =========================================================================

    fn has_property_index(&self, label: &str, property: &str) -> bool {
        self.engine.has_property_index(label, property)
    }

    fn list_property_indexes(&self) -> Vec<(String, String, Vec<String>)> {
        self.engine.list_property_indexes()
    }

    // =========================================================================
    // 통계
    // =========================================================================

    async fn node_count(&self) -> usize {
        self.engine.node_count().await.unwrap_or(0)
    }

    async fn edge_count(&self) -> usize {
        self.engine.edge_count().await
    }

    // =========================================================================
    // 다운캐스팅 지원
    // =========================================================================

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_graph_engine(&self) -> Option<&Arc<GraphEngine>> {
        Some(&self.engine)
    }
}
