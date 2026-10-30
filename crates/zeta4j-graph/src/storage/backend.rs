//! 저장소 백엔드 추상화 트레이트
//!
//! 노드와 엣지 저장소의 공통 인터페이스를 정의합니다.
//! 다양한 저장소 구현체(디스크, 분산 등)를 지원하기 위한 추상화 계층입니다.

use crate::core::{Edge, EdgeId, Node, NodeId, Value};
use crate::error::Result;
use crate::transaction::TransactionId;
use indexmap::IndexMap;

/// 노드 저장소 백엔드 트레이트
///
/// 노드 CRUD 연산의 최소 인터페이스를 정의합니다.
/// `DiskNodeStorage`, 분산 노드 저장소 등이 이 트레이트를 구현합니다.
///
/// # 구현 요구사항
///
/// - `Send + Sync`: 멀티스레드 환경에서 안전하게 사용 가능해야 함
/// - 모든 메서드는 `Result`를 반환하여 에러 처리가 가능해야 함
pub trait NodeStorageBackend: Send + Sync {
    /// 노드 삽입
    fn insert(&mut self, node: &Node) -> Result<()>;

    /// 노드 조회
    fn get(&self, node_id: NodeId) -> Result<Option<Node>>;

    /// 노드 업데이트
    fn update(&mut self, node: &Node) -> Result<()>;

    /// 노드 삭제
    fn delete(&mut self, node_id: NodeId) -> Result<Option<Node>>;

    /// 노드 존재 여부 확인
    fn contains(&self, node_id: NodeId) -> bool {
        self.get(node_id).ok().flatten().is_some()
    }

    /// 총 노드 개수
    fn count(&self) -> Result<usize>;

    /// 모든 노드 ID 반환
    fn all_ids(&self) -> Vec<NodeId>;

    /// 메모리 사용량 (바이트)
    fn memory_usage(&self) -> usize;
}

/// 트랜잭션 지원 노드 저장소 확장 트레이트
///
/// WAL 로깅과 트랜잭션 컨텍스트를 지원하는 노드 저장소입니다.
pub trait TransactionalNodeStorage: NodeStorageBackend {
    /// 트랜잭션 컨텍스트와 함께 노드 삽입 (WAL 로깅 포함)
    fn insert_with_tx(&mut self, node: &Node, tx_id: TransactionId) -> Result<()>;

    /// 트랜잭션 컨텍스트와 함께 노드 업데이트 (WAL 로깅 포함)
    fn update_with_tx(&mut self, node: &Node, tx_id: TransactionId) -> Result<()>;

    /// 트랜잭션 컨텍스트와 함께 노드 삭제 (WAL 로깅 포함)
    fn delete_with_tx(&mut self, node_id: NodeId, tx_id: TransactionId) -> Result<Option<Node>>;
}

/// 엣지 저장소 백엔드 트레이트
///
/// 엣지 CRUD 연산의 최소 인터페이스를 정의합니다.
/// `DiskEdgeStorage`, 분산 엣지 저장소 등이 이 트레이트를 구현합니다.
pub trait EdgeStorageBackend: Send + Sync {
    /// 엣지 삽입
    fn insert(&mut self, edge: &Edge) -> Result<()>;

    /// 엣지 조회
    fn get(&self, edge_id: EdgeId) -> Result<Option<Edge>>;

    /// 엣지 업데이트
    fn update(&mut self, edge: &Edge) -> Result<()>;

    /// 엣지 삭제
    fn delete(&mut self, edge_id: EdgeId) -> Result<Option<Edge>>;

    /// 엣지 존재 여부 확인
    fn contains(&self, edge_id: EdgeId) -> bool {
        self.get(edge_id).ok().flatten().is_some()
    }

    /// 총 엣지 개수
    fn count(&self) -> usize;

    /// 모든 엣지 ID 반환
    fn all_ids(&self) -> Vec<EdgeId>;

    /// 노드와 연결된 모든 엣지 ID (양방향)
    fn get_connected_edges(&self, node_id: NodeId) -> Vec<EdgeId>;

    /// 노드에서 나가는 엣지 ID
    fn get_outgoing_edges(&self, node_id: NodeId) -> Vec<EdgeId>;

    /// 노드로 들어오는 엣지 ID
    fn get_incoming_edges(&self, node_id: NodeId) -> Vec<EdgeId>;

    /// 두 노드 사이의 엣지 ID
    fn get_edges_between(&self, from: NodeId, to: NodeId) -> Vec<EdgeId>;

    /// 메모리 사용량 (바이트)
    fn memory_usage(&self) -> usize;
}

/// 트랜잭션 지원 엣지 저장소 확장 트레이트
pub trait TransactionalEdgeStorage: EdgeStorageBackend {
    /// 트랜잭션 컨텍스트와 함께 엣지 삽입 (WAL 로깅 포함)
    fn insert_with_tx(&mut self, edge: &Edge, tx_id: TransactionId) -> Result<()>;

    /// 트랜잭션 컨텍스트와 함께 엣지 업데이트 (WAL 로깅 포함)
    fn update_with_tx(&mut self, edge: &Edge, tx_id: TransactionId) -> Result<()>;

    /// 트랜잭션 컨텍스트와 함께 엣지 삭제 (WAL 로깅 포함)
    fn delete_with_tx(&mut self, edge_id: EdgeId, tx_id: TransactionId) -> Result<Option<Edge>>;
}

/// 프로퍼티 스토어 트레이트
///
/// 노드/엣지의 프로퍼티를 별도로 저장하는 컬럼형 저장소입니다.
/// 대규모 프로퍼티 데이터 최적화에 사용됩니다.
pub trait PropertyStore: Send + Sync {
    /// 프로퍼티 설정
    fn set(&mut self, entity_id: u64, key: &str, value: Value) -> Result<()>;

    /// 프로퍼티 조회
    fn get(&self, entity_id: u64, key: &str) -> Result<Option<Value>>;

    /// 엔티티의 모든 프로퍼티 조회
    fn get_all(&self, entity_id: u64) -> Result<IndexMap<String, Value>>;

    /// 프로퍼티 삭제
    fn remove(&mut self, entity_id: u64, key: &str) -> Result<Option<Value>>;

    /// 엔티티의 모든 프로퍼티 삭제
    fn remove_all(&mut self, entity_id: u64) -> Result<()>;
}

/// 레이블 인덱스 트레이트
///
/// 노드 레이블 기반 인덱싱을 지원합니다.
pub trait LabelIndex: Send + Sync {
    /// 레이블에 노드 추가
    fn add(&mut self, label: &str, node_id: NodeId) -> Result<()>;

    /// 레이블에서 노드 제거
    fn remove(&mut self, label: &str, node_id: NodeId) -> Result<bool>;

    /// 레이블로 노드 조회
    fn find_by_label(&self, label: &str) -> Vec<NodeId>;

    /// 모든 레이블 목록
    fn get_all_labels(&self) -> Vec<String>;

    /// 레이블의 노드 개수
    fn count_by_label(&self, label: &str) -> usize;
}
