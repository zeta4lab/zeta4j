//! Disk-based Label Index
//!
//! 레이블 기반 노드 검색을 위한 디스크 영속 인덱스입니다.
//! `MultiValueBPlusTree`를 사용하여 `Label(String) → NodeId set`을 저장합니다.
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::index::DiskLabelIndex;
//!
//! let mut index = DiskLabelIndex::create_new(&bpm).await?;
//!
//! // 레이블에 노드 추가
//! index.add("Person", 1).await?;
//! index.add("Person", 2).await?;
//! index.add("Movie", 10).await?;
//!
//! // 레이블로 노드 조회
//! let persons = index.get_nodes("Person").await?;
//! assert_eq!(persons.len(), 2);
//! ```

use std::sync::Arc;

use crate::core::NodeId;
use crate::error::Result;
use crate::storage::cache::BufferPoolManager;
use crate::storage::disk::page::PageId;
use crate::storage::index::btree::MultiValueBPlusTree;

/// 디스크 기반 레이블 인덱스
///
/// B+Tree를 사용하여 레이블 → 노드 ID 매핑을 디스크에 저장합니다.
/// 서버 재시작 후에도 레이블 인덱스가 유지됩니다.
#[derive(Debug)]
pub struct DiskLabelIndex {
    /// 내부 Multi-value B+Tree (Label → NodeId set)
    tree: MultiValueBPlusTree,
}

impl DiskLabelIndex {
    /// 기존 인덱스 래핑
    pub fn new(tree: MultiValueBPlusTree) -> Self {
        Self { tree }
    }

    /// 새 디스크 라벨 인덱스 생성
    pub async fn create_new(bpm: &Arc<BufferPoolManager>) -> Result<Self> {
        let tree = MultiValueBPlusTree::create_new(bpm).await?;
        Ok(Self { tree })
    }

    /// 디스크에서 복구
    pub async fn recover_from_disk(bpm: Arc<BufferPoolManager>) -> Result<Self> {
        let tree = MultiValueBPlusTree::recover_from_disk(bpm).await?;
        Ok(Self { tree })
    }

    /// BufferPoolManager 참조 반환 (WAL 연결/복구용)
    pub fn buffer_pool_manager(&self) -> &Arc<BufferPoolManager> {
        self.tree.buffer_pool_manager()
    }

    /// 레이블에 노드 추가
    ///
    /// # Arguments
    /// * `label` - 레이블 문자열
    /// * `node_id` - 노드 ID
    pub async fn add(&mut self, label: &str, node_id: NodeId) -> Result<()> {
        self.tree.insert_str(label, node_id).await
    }

    /// 레이블에서 노드 제거
    ///
    /// # Arguments
    /// * `label` - 레이블 문자열
    /// * `node_id` - 노드 ID
    ///
    /// # Returns
    /// 해당 (label, node_id) 쌍이 존재했으면 `true`, 아니면 `false`
    pub async fn remove(&mut self, label: &str, node_id: NodeId) -> Result<bool> {
        self.tree.remove_str(label, node_id).await
    }

    /// 레이블에 속한 모든 노드 조회
    ///
    /// # Arguments
    /// * `label` - 레이블 문자열
    ///
    /// # Returns
    /// 해당 레이블을 가진 노드 ID 목록
    pub async fn get_nodes(&self, label: &str) -> Result<Vec<NodeId>> {
        self.tree.get_all_str(label).await
    }

    /// 레이블에 특정 노드가 존재하는지 확인
    pub async fn contains(&self, label: &str, node_id: NodeId) -> Result<bool> {
        self.tree.contains_str(label, node_id).await
    }

    /// 레이블에 속한 노드 수 반환
    pub async fn count(&self, label: &str) -> Result<usize> {
        Ok(self.get_nodes(label).await?.len())
    }

    /// 레이블 존재 여부 확인
    pub async fn contains_label(&self, label: &str) -> Result<bool> {
        Ok(!self.get_nodes(label).await?.is_empty())
    }

    /// 루트 페이지 ID 반환
    pub fn root_page_id(&self) -> PageId {
        self.tree.root_page_id()
    }

    /// 내부 트리 참조
    pub fn inner(&self) -> &MultiValueBPlusTree {
        &self.tree
    }
}
