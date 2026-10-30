//! CypherBackend 추상화 레이어
//!
//! Cypher 쿼리 실행에 필요한 그래프 저장소 확장 인터페이스를 정의합니다.
//! 현재 제공되는 구현은 `zeta4j-cypher-executor`의 `GraphEngineBackend`입니다.
//!
//! ## 예제
//!
//! ```rust,ignore
//! use zeta4j_cypher_core::backend::CypherBackend;
//!
//! // 백엔드를 사용한 트랜잭션
//! let tx_id = backend.begin_transaction()?;
//! let node_id = backend.create_node_internal(tx_id, vec!["Person".to_string()]).await?;
//! backend.commit_transaction(tx_id)?;
//! ```

use std::any::Any;
use std::sync::Arc;

use zeta4j_graph::Result;
use zeta4j_graph::core::{Edge, EdgeId, Node, NodeId};
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::transaction::TransactionId;

/// Cypher 쿼리 실행을 위한 그래프 저장소 인터페이스
///
/// Cypher 실행기가 요구하는 그래프 저장소 연산의 추상화 레이어입니다.
/// 현재는 `GraphEngineBackend`가 이 트레이트를 구현합니다.
///
/// ## 트랜잭션 의미론
///
/// `GraphEngineBackend`는 MVCC 기반의 완전한 ACID 트랜잭션을 지원합니다.
#[async_trait::async_trait]
pub trait CypherBackend: Send + Sync {
    // =========================================================================
    // 트랜잭션 관리
    // =========================================================================

    /// 새 트랜잭션 시작
    fn begin_transaction(&self) -> Result<TransactionId>;

    /// 트랜잭션 커밋
    fn commit_transaction(&self, tx_id: TransactionId) -> Result<()>;

    /// 트랜잭션 롤백
    fn rollback_transaction(&self, tx_id: TransactionId) -> Result<()>;

    // =========================================================================
    // 노드 CRUD
    // =========================================================================

    /// 노드 생성 (트랜잭션 컨텍스트)
    async fn create_node_internal(
        &self,
        tx_id: TransactionId,
        labels: Vec<String>,
    ) -> Result<NodeId>;

    /// 노드 조회
    async fn get_node(&self, node_id: NodeId) -> Result<Option<Node>>;

    /// 노드 업데이트
    async fn update_node_internal(
        &self,
        tx_id: TransactionId,
        node_id: NodeId,
        update_fn: Box<dyn for<'a> FnOnce(&'a mut Node) + Send + 'static>,
    ) -> Result<()>;

    /// 노드 삭제
    async fn delete_node_internal(
        &self,
        tx_id: TransactionId,
        node_id: NodeId,
        detach: bool,
    ) -> Result<()>;

    // =========================================================================
    // 엣지 CRUD
    // =========================================================================

    /// 엣지 생성 (트랜잭션 컨텍스트)
    async fn create_edge_internal(
        &self,
        tx_id: TransactionId,
        relation_type: String,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Result<EdgeId>;

    /// 엣지 조회
    async fn get_edge(&self, edge_id: EdgeId) -> Result<Option<Edge>>;

    /// 엣지 업데이트
    async fn update_edge_internal(
        &self,
        tx_id: TransactionId,
        edge_id: EdgeId,
        update_fn: Box<dyn for<'a> FnOnce(&'a mut Edge) + Send + 'static>,
    ) -> Result<()>;

    /// 엣지 삭제
    async fn delete_edge_internal(&self, tx_id: TransactionId, edge_id: EdgeId) -> Result<()>;

    // =========================================================================
    // 조회
    // =========================================================================

    /// 라벨로 노드 찾기
    async fn find_nodes_by_label(&self, label: &str) -> Result<Vec<NodeId>>;

    /// 노드의 연결된 엣지들 조회
    async fn get_connected_edges(&self, node_id: NodeId) -> Vec<EdgeId>;

    /// 모든 노드 ID 조회
    async fn all_node_ids(&self) -> Vec<NodeId>;

    /// 모든 엣지 ID 조회
    async fn all_edge_ids(&self) -> Vec<EdgeId>;

    // =========================================================================
    // 인덱스
    // =========================================================================

    /// 프로퍼티 인덱스 존재 여부
    fn has_property_index(&self, label: &str, property: &str) -> bool;

    /// 프로퍼티 인덱스 목록
    fn list_property_indexes(&self) -> Vec<(String, String, Vec<String>)>;

    // =========================================================================
    // 통계
    // =========================================================================

    /// 노드 수
    async fn node_count(&self) -> usize;

    /// 엣지 수
    async fn edge_count(&self) -> usize;

    // =========================================================================
    // 다운캐스팅 지원
    // =========================================================================

    /// Any로 변환 (다운캐스팅 지원)
    fn as_any(&self) -> &dyn Any;

    /// GraphEngine 참조 반환 (GraphEngineBackend인 경우에만)
    ///
    /// 알고리즘, 스키마 관리 등 GraphEngine 전용 기능에서 사용합니다.
    /// GraphEngine을 래핑하지 않는 구현은 None을 반환합니다.
    fn as_graph_engine(&self) -> Option<&Arc<GraphEngine>>;
}

/// CypherBackend 트레이트 객체 타입 별칭
pub type DynCypherBackend = Arc<dyn CypherBackend>;
