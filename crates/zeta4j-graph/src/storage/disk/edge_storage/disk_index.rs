//! Disk-based Edge Secondary Indexes
//!
//! 엣지의 보조 인덱스를 디스크에 영속화합니다.
//! `MultiValueBPlusTree`를 사용하여 다음 매핑을 저장합니다:
//! - outgoing: `NodeId → EdgeId set` (노드에서 나가는 엣지)
//! - incoming: `NodeId → EdgeId set` (노드로 들어오는 엣지)
//! - type_idx: `Type(String) → EdgeId set` (관계 타입별 엣지)
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::storage::disk_edge_storage::DiskEdgeSecondaryIndexes;
//!
//! let indexes = DiskEdgeSecondaryIndexes::create_new(&outgoing_bpm, &incoming_bpm, &type_bpm).await?;
//!
//! // 엣지 추가
//! indexes.insert(edge_id, from_node, to_node, "KNOWS").await?;
//!
//! // 조회
//! let outgoing_edges = indexes.get_outgoing(from_node).await?;
//! let incoming_edges = indexes.get_incoming(to_node).await?;
//! let knows_edges = indexes.get_by_type("KNOWS").await?;
//! ```

use std::path::Path;
use std::sync::Arc;

use crate::core::{EdgeId, NodeId};
use crate::error::{GraphError, Result};
use crate::storage::cache::BufferPoolManager;
use crate::storage::disk::manager::DiskManager;
use crate::storage::index::btree::MultiValueBPlusTree;
use crate::transaction::PageStorageType;

/// 디스크 기반 엣지 보조 인덱스
///
/// 서버 재시작 후에도 보조 인덱스가 유지되어 rebuild가 필요 없습니다.
#[derive(Debug)]
pub struct DiskEdgeSecondaryIndexes {
    /// 노드별 나가는 엣지 인덱스 (from_node → EdgeId set)
    outgoing: MultiValueBPlusTree,

    /// 노드별 들어오는 엣지 인덱스 (to_node → EdgeId set)
    incoming: MultiValueBPlusTree,

    /// 관계 타입별 엣지 인덱스 (Type → EdgeId set)
    type_idx: MultiValueBPlusTree,
}

impl DiskEdgeSecondaryIndexes {
    /// 기존 인덱스들 래핑
    pub fn new(
        outgoing: MultiValueBPlusTree,
        incoming: MultiValueBPlusTree,
        type_idx: MultiValueBPlusTree,
    ) -> Self {
        Self {
            outgoing,
            incoming,
            type_idx,
        }
    }

    /// 새 디스크 보조 인덱스 생성 (3개의 BufferPoolManager 필요)
    pub async fn create_new(
        outgoing_bpm: &Arc<BufferPoolManager>,
        incoming_bpm: &Arc<BufferPoolManager>,
        type_bpm: &Arc<BufferPoolManager>,
    ) -> Result<Self> {
        let outgoing = MultiValueBPlusTree::create_new(outgoing_bpm).await?;
        let incoming = MultiValueBPlusTree::create_new(incoming_bpm).await?;
        let type_idx = MultiValueBPlusTree::create_new(type_bpm).await?;

        Ok(Self {
            outgoing,
            incoming,
            type_idx,
        })
    }

    /// 디스크에서 복구
    pub async fn recover_from_disk(
        outgoing_bpm: Arc<BufferPoolManager>,
        incoming_bpm: Arc<BufferPoolManager>,
        type_bpm: Arc<BufferPoolManager>,
    ) -> Result<Self> {
        let outgoing = MultiValueBPlusTree::recover_from_disk(outgoing_bpm).await?;
        let incoming = MultiValueBPlusTree::recover_from_disk(incoming_bpm).await?;
        let type_idx = MultiValueBPlusTree::recover_from_disk(type_bpm).await?;

        Ok(Self {
            outgoing,
            incoming,
            type_idx,
        })
    }

    /// 경로로부터 인덱스 파일들 열기/생성
    ///
    /// # Arguments
    /// * `base_path` - 베이스 디렉토리 경로
    /// * `pool_size` - 각 인덱스의 BufferPool 크기
    pub async fn open_or_create(base_path: impl AsRef<Path>, pool_size: usize) -> Result<Self> {
        let base = base_path.as_ref();
        std::fs::create_dir_all(base)?;

        let outgoing_path = base.join("edges_outgoing.idx");
        let incoming_path = base.join("edges_incoming.idx");
        let type_path = base.join("edges_type.idx");

        // 파일 존재 여부 확인
        let outgoing_exists = outgoing_path.exists();
        let incoming_exists = incoming_path.exists();
        let type_exists = type_path.exists();

        // BufferPoolManager 생성 (각각의 storage type 사용)
        let outgoing_bpm =
            Self::create_bpm(&outgoing_path, pool_size, PageStorageType::EdgeOutgoing).await?;
        let incoming_bpm =
            Self::create_bpm(&incoming_path, pool_size, PageStorageType::EdgeIncoming).await?;
        let type_bpm = Self::create_bpm(&type_path, pool_size, PageStorageType::EdgeType).await?;

        // 모두 새 파일이거나 모두 기존 파일이어야 함
        if outgoing_exists && incoming_exists && type_exists {
            // 복구
            Self::recover_from_disk(outgoing_bpm, incoming_bpm, type_bpm).await
        } else if !outgoing_exists && !incoming_exists && !type_exists {
            // 새로 생성
            Self::create_new(&outgoing_bpm, &incoming_bpm, &type_bpm).await
        } else {
            // 일부만 존재 - 불일치 상태
            Err(GraphError::Storage {
                message: "Inconsistent secondary index files - some exist, some don't".to_string(),
            })
        }
    }

    /// BufferPoolManager 생성 헬퍼
    async fn create_bpm(
        path: &Path,
        pool_size: usize,
        storage_type: PageStorageType,
    ) -> Result<Arc<BufferPoolManager>> {
        let dm = Arc::new(DiskManager::new(path).await?);
        Ok(Arc::new(BufferPoolManager::new(
            dm,
            pool_size,
            storage_type,
        )))
    }

    /// 엣지 추가 시 인덱스 업데이트
    pub async fn insert(
        &mut self,
        edge_id: EdgeId,
        from_node: NodeId,
        to_node: NodeId,
        relation_type: &str,
    ) -> Result<()> {
        // outgoing index
        self.outgoing
            .insert(&from_node.to_le_bytes(), edge_id)
            .await?;

        // incoming index
        self.incoming
            .insert(&to_node.to_le_bytes(), edge_id)
            .await?;

        // type index
        self.type_idx.insert_str(relation_type, edge_id).await?;

        Ok(())
    }

    /// 엣지 삭제 시 인덱스 업데이트
    pub async fn remove(
        &mut self,
        edge_id: EdgeId,
        from_node: NodeId,
        to_node: NodeId,
        relation_type: &str,
    ) -> Result<()> {
        // outgoing index
        self.outgoing
            .remove(&from_node.to_le_bytes(), edge_id)
            .await?;

        // incoming index
        self.incoming
            .remove(&to_node.to_le_bytes(), edge_id)
            .await?;

        // type index
        self.type_idx.remove_str(relation_type, edge_id).await?;

        Ok(())
    }

    /// 특정 노드에서 나가는 엣지들
    pub async fn get_outgoing(&self, node_id: NodeId) -> Result<Vec<EdgeId>> {
        self.outgoing.get_all(&node_id.to_le_bytes()).await
    }

    /// 특정 노드로 들어오는 엣지들
    pub async fn get_incoming(&self, node_id: NodeId) -> Result<Vec<EdgeId>> {
        self.incoming.get_all(&node_id.to_le_bytes()).await
    }

    /// 특정 관계 타입의 엣지들
    pub async fn get_by_type(&self, relation_type: &str) -> Result<Vec<EdgeId>> {
        self.type_idx.get_all_str(relation_type).await
    }

    /// 특정 노드와 연결된 모든 엣지들 (양방향)
    pub async fn get_connected(&self, node_id: NodeId) -> Result<Vec<EdgeId>> {
        let mut result = std::collections::HashSet::new();

        for edge_id in self.get_outgoing(node_id).await? {
            result.insert(edge_id);
        }

        for edge_id in self.get_incoming(node_id).await? {
            result.insert(edge_id);
        }

        Ok(result.into_iter().collect())
    }

    /// 노드에 연결된 엣지가 있는지
    pub async fn has_connections(&self, node_id: NodeId) -> Result<bool> {
        let has_outgoing = !self.get_outgoing(node_id).await?.is_empty();
        if has_outgoing {
            return Ok(true);
        }
        let has_incoming = !self.get_incoming(node_id).await?.is_empty();
        Ok(has_incoming)
    }

    /// outgoing 인덱스에 특정 노드가 있는지
    pub async fn has_outgoing(&self, node_id: NodeId) -> Result<bool> {
        Ok(!self.get_outgoing(node_id).await?.is_empty())
    }

    /// incoming 인덱스에 특정 노드가 있는지
    pub async fn has_incoming(&self, node_id: NodeId) -> Result<bool> {
        Ok(!self.get_incoming(node_id).await?.is_empty())
    }

    /// 관계 타입 존재 여부
    pub async fn has_type(&self, relation_type: &str) -> Result<bool> {
        Ok(!self.get_by_type(relation_type).await?.is_empty())
    }

    /// 모든 인덱스 flush
    pub async fn flush(&self) -> Result<()> {
        self.outgoing.flush().await?;
        self.incoming.flush().await?;
        self.type_idx.flush().await?;
        Ok(())
    }

    /// 모든 보조 인덱스의 BufferPoolManager에 WAL 연결
    pub async fn set_page_wal(&self, page_wal: Arc<crate::transaction::PageWAL>) {
        self.outgoing
            .buffer_pool_manager()
            .set_page_wal(page_wal.clone())
            .await;
        self.incoming
            .buffer_pool_manager()
            .set_page_wal(page_wal.clone())
            .await;
        self.type_idx
            .buffer_pool_manager()
            .set_page_wal(page_wal)
            .await;
    }

    /// 모든 보조 인덱스 BPM에 current_tx 설정 (WAL 로깅 활성화)
    pub async fn set_current_tx(&self, tx_id: crate::transaction::TransactionId) {
        self.outgoing
            .buffer_pool_manager()
            .set_current_tx(tx_id)
            .await;
        self.incoming
            .buffer_pool_manager()
            .set_current_tx(tx_id)
            .await;
        self.type_idx
            .buffer_pool_manager()
            .set_current_tx(tx_id)
            .await;
    }

    /// 모든 보조 인덱스 BPM에서 current_tx 해제
    pub async fn clear_current_tx(&self) {
        self.outgoing.buffer_pool_manager().clear_current_tx().await;
        self.incoming.buffer_pool_manager().clear_current_tx().await;
        self.type_idx.buffer_pool_manager().clear_current_tx().await;
    }

    /// 모든 보조 인덱스의 BufferPoolManager 참조 반환 (WAL 복구용)
    pub fn buffer_pool_managers(&self) -> Vec<Arc<BufferPoolManager>> {
        vec![
            self.outgoing.buffer_pool_manager().clone(),
            self.incoming.buffer_pool_manager().clone(),
            self.type_idx.buffer_pool_manager().clone(),
        ]
    }
}
