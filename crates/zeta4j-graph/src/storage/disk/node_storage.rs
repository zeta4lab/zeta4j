// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 디스크 기반 노드 스토리지
//!
//! Page-level WAL은 GraphEngine에서 통합 관리됩니다.
//!
//! ## PageWAL 통합 (v3.0.1)
//!
//! `set_page_wal()`을 통해 BufferPoolManager에 PageWAL을 설정할 수 있습니다.
//!
//! ## DiskLabelIndex 통합 (v3.1.0)
//!
//! 레이블 인덱스가 디스크에 영속화됩니다. 서버 재시작 후에도 레이블 검색이 가능합니다.

use crate::core::PostcardSerializable;
use crate::core::node::{Node, NodeId};
use crate::error::{GraphError, Result};
use crate::index::DiskLabelIndex;
use crate::storage::cache::BufferPoolManager;
use crate::storage::disk::manager::DiskManager;
use crate::storage::disk::slotted_page::SlottedPage;
use crate::storage::index::btree::index::BPlusTree;
use crate::storage::index::btree::types::RecordId;
use crate::storage::util::config::StorageConfig;
use crate::transaction::{PageStorageType, PageWAL, TransactionId};
use indexmap::IndexSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// 디스크 기반 노드 스토리지
pub struct DiskNodeStorage {
    #[allow(dead_code)] // BufferPoolManager 수명 보장용 보존
    disk_manager: Arc<DiskManager>,
    buffer_pool: Arc<BufferPoolManager>,
    index: BPlusTree,
    /// 레이블 인덱스 (디스크 기반)
    label_index: DiskLabelIndex,
    label_buffer_pool: Arc<BufferPoolManager>,
    /// Heap 파일 관리용
    last_page_id: Mutex<u32>,
    base_path: PathBuf,
    /// PageWAL 참조 (Slot-level 로깅용)
    page_wal: Option<Arc<PageWAL>>,
}

impl fmt::Debug for DiskNodeStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiskNodeStorage")
            .field("base_path", &self.base_path)
            .finish()
    }
}

impl DiskNodeStorage {
    /// 기본 설정으로 스토리지 생성
    pub async fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        Self::with_config(base_path, StorageConfig::default()).await
    }

    /// 설정을 지정하여 스토리지 생성
    pub async fn with_config(base_path: impl AsRef<Path>, config: StorageConfig) -> Result<Self> {
        let base = base_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&base)?;

        let heap_path = base.join("nodes.db");
        let label_index_path = base.join("nodes_label.db");

        let disk_manager = Arc::new(DiskManager::new(&heap_path).await?);
        let buffer_pool = Arc::new(BufferPoolManager::new(
            disk_manager.clone(),
            config.buffer_pool_size,
            PageStorageType::Node,
        ));

        // 새 파일인지 확인
        let is_new_file = disk_manager.file_size().await? == 0;

        // Index 초기화
        let index = if is_new_file {
            BPlusTree::create_new_tree_with_metadata(&buffer_pool).await?
        } else {
            BPlusTree::recover_from_disk(buffer_pool.clone()).await?
        };

        // 레이블 인덱스 초기화 (별도 파일)
        let label_disk_manager = Arc::new(DiskManager::new(&label_index_path).await?);
        let label_buffer_pool = Arc::new(BufferPoolManager::new(
            label_disk_manager.clone(),
            config.buffer_pool_size / 2, // 레이블 인덱스는 작은 버퍼 풀 사용
            PageStorageType::NodeLabel,
        ));

        let is_new_label_file = label_disk_manager.file_size().await? == 0;
        let label_index = if is_new_label_file {
            DiskLabelIndex::create_new(&label_buffer_pool).await?
        } else {
            DiskLabelIndex::recover_from_disk(label_buffer_pool.clone()).await?
        };

        // last_page_id 계산
        let file_size = disk_manager.file_size().await?;
        let last_page = if file_size > 0 {
            (file_size / config.page_size as u64)
                .saturating_sub(1)
                .min(u32::MAX as u64) as u32
        } else {
            1 // Page 0=metadata, Page 1=root leaf
        };

        let storage = Self {
            disk_manager,
            buffer_pool,
            index,
            label_index,
            label_buffer_pool,
            last_page_id: Mutex::new(last_page),
            base_path: base,
            page_wal: None,
        };

        // 새 파일이면 메타데이터를 디스크에 flush
        if is_new_file || is_new_label_file {
            storage.flush().await?;
        }

        Ok(storage)
    }

    /// 노드 삽입
    pub async fn insert(&mut self, node: &Node) -> Result<()> {
        let bytes = node.to_bytes()?;
        let record_id = self.insert_to_heap(&bytes).await?;
        self.index.upsert(node.id(), record_id).await?;

        // 레이블 인덱스 업데이트
        for label in node.labels() {
            self.label_index.add(label, node.id()).await?;
        }

        Ok(())
    }

    /// 노드 삽입 (트랜잭션 컨텍스트 포함, WAL 로깅)
    ///
    /// Heap과 B+Tree 인덱스 모두 PageWAL에 로깅합니다.
    pub async fn insert_with_tx(&mut self, node: &Node, tx_id: TransactionId) -> Result<()> {
        let bytes = node.to_bytes()?;
        let record_id = self.insert_to_heap_with_tx(&bytes, tx_id).await?;

        // B+Tree 작업 전에 current_tx 설정 (자동 WAL 로깅 활성화)
        {
            self.buffer_pool.set_current_tx(tx_id).await;
        }

        // B+Tree 인덱스 업데이트 (BufferPoolManager가 자동으로 WAL 로깅)
        let result = self.index.upsert(node.id(), record_id).await;

        // current_tx 해제
        {
            self.buffer_pool.clear_current_tx().await;
        }

        // 레이블 인덱스 업데이트 (Label BPM에도 WAL 로깅)
        // Note: UNDO 시 restore_label_index/remove_from_label_index로 복구됨 (#87)
        self.label_buffer_pool.set_current_tx(tx_id).await;
        for label in node.labels() {
            self.label_index.add(label, node.id()).await?;
        }
        self.label_buffer_pool.clear_current_tx().await;

        result.map(|_| ())
    }

    /// 노드 Upsert (트랜잭션 컨텍스트 포함, WAL 로깅)
    ///
    /// 기존 노드가 있으면 삭제 후 새로 삽입 (SlotDelete + SlotInsert)
    /// 롤백 시 이전 상태로 복원 가능
    pub async fn upsert_with_tx(&mut self, node: &Node, tx_id: TransactionId) -> Result<()> {
        // 기존 레코드 ID 조회
        let old_record_id = self.index.get_value(node.id()).await?;

        if let Some(old_rid) = old_record_id {
            // 기존 슬롯 삭제 (SlotDelete 로깅)
            self.delete_slot_with_tx(old_rid, tx_id).await?;
        }

        // 새 데이터 삽입 (SlotInsert 로깅)
        let bytes = node.to_bytes()?;
        let new_record_id = self.insert_to_heap_with_tx(&bytes, tx_id).await?;

        // B+Tree 인덱스 업데이트
        {
            self.buffer_pool.set_current_tx(tx_id).await;
        }

        let result = self.index.upsert(node.id(), new_record_id).await;

        self.buffer_pool.clear_current_tx().await;

        // 레이블 인덱스 업데이트 (Label BPM에도 WAL 로깅)
        self.label_buffer_pool.set_current_tx(tx_id).await;
        for label in node.labels() {
            self.label_index.add(label, node.id()).await?;
        }
        self.label_buffer_pool.clear_current_tx().await;

        result.map(|_| ())
    }

    /// 노드 삭제 (트랜잭션 컨텍스트 포함, WAL 로깅)
    ///
    /// 슬롯을 삭제하고 SlotDelete를 WAL에 기록합니다.
    /// 롤백 시 슬롯이 복원됩니다.
    pub async fn delete_with_tx(&mut self, node_id: NodeId, tx_id: TransactionId) -> Result<bool> {
        // 레이블 인덱스에서 제거하기 위해 먼저 노드 조회 (Label BPM WAL 로깅)
        self.label_buffer_pool.set_current_tx(tx_id).await;
        if let Some(node) = self.get(node_id).await? {
            for label in node.labels() {
                self.label_index.remove(label, node_id).await?;
            }
        }
        self.label_buffer_pool.clear_current_tx().await;

        // 레코드 ID 조회
        let record_id = match self.index.get_value(node_id).await? {
            Some(rid) => rid,
            None => return Ok(false),
        };

        // 슬롯 삭제 (SlotDelete 로깅)
        self.delete_slot_with_tx(record_id, tx_id).await?;

        // B+Tree에서 삭제
        {
            self.buffer_pool.set_current_tx(tx_id).await;
        }

        let deleted = self.index.delete(node_id).await?;

        self.buffer_pool.clear_current_tx().await;

        Ok(deleted.is_some())
    }

    /// 슬롯 삭제 (WAL 로깅)
    async fn delete_slot_with_tx(&self, record_id: RecordId, tx_id: TransactionId) -> Result<()> {
        let page_guard = self.buffer_pool.fetch_page(record_id.page_id).await?;
        let mut page = page_guard.lock().await;

        let mut slotted = SlottedPage::new(&mut page);

        // Slot-level 로깅 사용 (PageWAL이 있는 경우)
        if let Some(ref page_wal) = self.page_wal {
            slotted.delete_record_with_wal(
                record_id.slot_id,
                page_wal,
                tx_id,
                PageStorageType::Node,
            )?;
        } else {
            slotted.delete_record(record_id.slot_id)?;
        }

        drop(page);
        self.buffer_pool.unpin_page(record_id.page_id, true).await?;

        Ok(())
    }

    /// UNDO 시 인덱스 복구
    ///
    /// SlotDelete UNDO 후 인덱스를 새 RecordId로 업데이트합니다.
    /// 롤백/복구 시에만 사용됩니다.
    pub async fn restore_index(&mut self, node_id: NodeId, record_id: RecordId) -> Result<()> {
        self.index.upsert(node_id, record_id).await?;
        Ok(())
    }

    /// UNDO 시 레이블 인덱스 복구
    ///
    /// SlotDelete UNDO 후 레이블 인덱스에 노드를 다시 추가합니다.
    pub async fn restore_label_index(
        &mut self,
        node_id: NodeId,
        labels: &IndexSet<String>,
    ) -> Result<()> {
        for label in labels {
            self.label_index.add(label, node_id).await?;
        }
        Ok(())
    }

    /// UNDO 시 레이블 인덱스에서 제거
    ///
    /// SlotInsert UNDO 시 레이블 인덱스에서 노드를 제거합니다.
    pub async fn remove_from_label_index(
        &mut self,
        node_id: NodeId,
        labels: &IndexSet<String>,
    ) -> Result<()> {
        for label in labels {
            self.label_index.remove(label, node_id).await?;
        }
        Ok(())
    }

    /// UNDO 시 B+Tree 인덱스에서 노드 제거
    ///
    /// SlotInsert UNDO 시 B+Tree 인덱스에서 노드 엔트리를 제거합니다.
    /// 슬롯 페이지에서는 이미 삭제되었으므로 인덱스만 정리합니다.
    pub async fn undo_insert(&mut self, node_id: NodeId) -> Result<()> {
        self.index.delete(node_id).await?;
        Ok(())
    }

    /// 노드 조회
    ///
    /// UNDO로 삭제된 슬롯은 None을 반환합니다.
    /// (인덱스에는 남아있지만 슬롯이 삭제된 경우)
    pub async fn get(&self, node_id: NodeId) -> Result<Option<Node>> {
        let record_id = match self.index.get_value(node_id).await? {
            Some(rid) => rid,
            None => return Ok(None),
        };

        // UNDO로 슬롯이 삭제된 경우 None 반환
        match self.read_from_heap(record_id).await? {
            Some(bytes) => {
                let node = Node::from_bytes(&bytes)?;
                Ok(Some(node))
            }
            None => Ok(None),
        }
    }

    /// 노드 삭제
    pub async fn delete(&mut self, node_id: NodeId) -> Result<bool> {
        // 레이블 인덱스에서 제거하기 위해 먼저 노드 조회
        if let Some(node) = self.get(node_id).await? {
            for label in node.labels() {
                self.label_index.remove(label, node_id).await?;
            }
        }

        let deleted = self.index.delete(node_id).await?;
        Ok(deleted.is_some())
    }

    /// 버퍼 풀 플러시
    pub async fn flush(&self) -> Result<()> {
        // 메인 버퍼 풀 플러시
        {
            self.buffer_pool.flush_all_pages().await?;
        }

        // 레이블 인덱스 버퍼 풀 플러시
        {
            self.label_buffer_pool.flush_all_pages().await?;
        }

        Ok(())
    }

    /// last_page_id 재계산 (WAL 복구 후 호출)
    ///
    /// 디스크 파일 크기를 기반으로 last_page_id를 다시 계산합니다.
    /// WAL 복구 후 flush가 완료된 다음 호출해야 합니다.
    pub async fn recalculate_last_page_id(&self) -> Result<()> {
        let file_size = self.disk_manager.file_size().await?;
        let last_page = if file_size > 0 {
            (file_size / 4096_u64) as u32 - 1 // PAGE_SIZE = 4096
        } else {
            1
        };

        let mut last_pid = self.last_page_id.lock().await;
        *last_pid = last_page;

        Ok(())
    }

    /// BufferPoolManager 참조 반환 (Page-level WAL 통합용)
    pub fn buffer_pool(&self) -> Arc<BufferPoolManager> {
        self.buffer_pool.clone()
    }

    /// Label BufferPoolManager 참조 반환 (WAL 복구용)
    pub fn label_buffer_pool(&self) -> Arc<BufferPoolManager> {
        self.label_buffer_pool.clone()
    }

    /// PageWAL 설정 (GraphEngine에서 호출)
    ///
    /// BufferPoolManager와 DiskNodeStorage 모두에 PageWAL을 설정합니다.
    /// BufferPoolManager: B+Tree 페이지의 PageWrite 로깅
    /// DiskNodeStorage: Heap 페이지의 Slot-level 로깅
    pub async fn set_page_wal(&mut self, page_wal: Arc<PageWAL>) -> Result<()> {
        // BufferPoolManager에 설정 (Heap + Primary B+Tree용)
        self.buffer_pool.set_page_wal(page_wal.clone()).await;
        // Label B+Tree BufferPoolManager에도 설정
        self.label_buffer_pool.set_page_wal(page_wal.clone()).await;
        // DiskNodeStorage에도 설정 (Slot-level 로깅용)
        self.page_wal = Some(page_wal);
        Ok(())
    }

    /// 기본 경로 반환
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    // --- Label Index ---

    /// 레이블로 노드 ID 검색
    ///
    /// 디스크 기반 레이블 인덱스를 사용하여 빠르게 검색합니다.
    pub async fn find_by_label(&self, label: &str) -> Result<Vec<NodeId>> {
        self.label_index.get_nodes(label).await
    }

    /// 레이블로 노드 검색 (전체 노드 반환)
    ///
    /// 레이블 인덱스에서 노드 ID를 찾고, 각 노드를 heap에서 조회합니다.
    pub async fn find_nodes_by_label(&self, label: &str) -> Result<Vec<Node>> {
        let node_ids = self.label_index.get_nodes(label).await?;
        let mut nodes = Vec::with_capacity(node_ids.len());

        for node_id in node_ids {
            if let Some(node) = self.get(node_id).await? {
                nodes.push(node);
            }
        }

        Ok(nodes)
    }

    /// 레이블에 노드가 존재하는지 확인
    pub async fn label_contains(&self, label: &str, node_id: NodeId) -> Result<bool> {
        self.label_index.contains(label, node_id).await
    }

    /// 레이블에 속한 노드 수
    pub async fn label_count(&self, label: &str) -> Result<usize> {
        self.label_index.count(label).await
    }

    // --- Diagnostics ---

    /// B+Tree leaf chain 무결성 진단
    ///
    /// range_scan과 get_value의 결과를 비교하여 linked list 누락을 탐지합니다.
    pub async fn verify_leaf_chain(
        &self,
    ) -> Result<crate::storage::index::btree::index::LeafChainReport> {
        self.index.verify_leaf_chain().await
    }

    // --- Range Scan ---

    /// 노드 ID 범위로 검색
    ///
    /// UNDO로 삭제된 슬롯은 건너뜁니다.
    pub async fn range_scan(&self, start_id: NodeId, end_id: NodeId) -> Result<Vec<Node>> {
        let record_ids = self.index.range_scan(start_id, end_id).await?;
        let mut nodes = Vec::with_capacity(record_ids.len());

        for (_key, rid) in record_ids {
            // UNDO로 삭제된 슬롯은 건너뜀
            if let Some(bytes) = self.read_from_heap(rid).await? {
                let node = Node::from_bytes(&bytes)?;
                nodes.push(node);
            }
        }

        Ok(nodes)
    }

    /// 전체 노드 수
    pub async fn count(&self) -> Result<usize> {
        let results = self.index.range_scan(0, u64::MAX).await?;
        Ok(results.len())
    }

    /// 노드 ID 범위로 검색 (Vec 반환)
    ///
    /// UNDO로 삭제된 슬롯은 건너뜁니다.
    pub async fn range_scan_iter(
        &self,
        start_id: NodeId,
        end_id: NodeId,
    ) -> Result<Vec<Result<Node>>> {
        let record_ids = self.index.range_scan(start_id, end_id).await?;
        let mut results = Vec::new();
        for (_key, rid) in record_ids {
            if let Some(bytes) = self.read_from_heap(rid).await? {
                results.push(Node::from_bytes(&bytes));
            }
        }
        Ok(results)
    }

    // --- Heap Management ---

    /// 첫 번째 Heap 페이지 ID (Page 0, 1은 인덱스용)
    const FIRST_HEAP_PAGE_ID: u32 = 2;

    async fn insert_to_heap(&self, data: &[u8]) -> Result<RecordId> {
        let mut last_pid = self.last_page_id.lock().await;

        // last_pid가 인덱스 페이지 영역이면 새 Heap 페이지 할당
        if *last_pid < Self::FIRST_HEAP_PAGE_ID {
            drop(last_pid);
            return self.allocate_first_heap_page(data).await;
        }

        let page_guard = self.buffer_pool.fetch_page(*last_pid).await?;
        let mut page = page_guard.lock().await;

        let mut slotted = SlottedPage::new(&mut page);

        // 페이지가 초기화되지 않았으면 초기화
        if slotted.get_free_space_ptr() == 0 {
            slotted.init(*last_pid);
        }

        match slotted.insert_record(data) {
            Ok(slot_id) => {
                let rid = RecordId {
                    page_id: *last_pid,
                    slot_id,
                };
                drop(page);
                self.buffer_pool.unpin_page(*last_pid, true).await?;
                Ok(rid)
            }
            Err(_) => {
                drop(page);
                self.buffer_pool.unpin_page(*last_pid, false).await?;

                let new_page = self.buffer_pool.new_page().await?;
                let mut new_page_guard = new_page.lock().await;
                let new_pid = new_page_guard.id.ok_or_else(|| GraphError::Storage {
                    message: "New page has no ID".to_string(),
                })?;
                let mut slotted = SlottedPage::new(&mut new_page_guard);
                slotted.init(new_pid);

                let slot_id = slotted.insert_record(data)?;
                *last_pid = new_pid;

                let rid = RecordId {
                    page_id: new_pid,
                    slot_id,
                };
                drop(new_page_guard);
                self.buffer_pool.unpin_page(new_pid, true).await?;
                Ok(rid)
            }
        }
    }

    /// 첫 번째 Heap 페이지 할당 및 데이터 삽입
    async fn allocate_first_heap_page(&self, data: &[u8]) -> Result<RecordId> {
        let mut last_pid = self.last_page_id.lock().await;

        let new_page = self.buffer_pool.new_page().await?;
        let mut new_page_guard = new_page.lock().await;
        let new_pid = new_page_guard.id.ok_or_else(|| GraphError::Storage {
            message: "New page has no ID".to_string(),
        })?;

        let mut slotted = SlottedPage::new(&mut new_page_guard);
        slotted.init(new_pid);

        let slot_id = slotted.insert_record(data)?;
        *last_pid = new_pid;

        let rid = RecordId {
            page_id: new_pid,
            slot_id,
        };
        drop(new_page_guard);
        self.buffer_pool.unpin_page(new_pid, true).await?;

        Ok(rid)
    }

    /// Heap에 레코드 삽입 (트랜잭션 컨텍스트 포함, Slot-level WAL 로깅)
    ///
    /// PageWAL이 설정된 경우 Slot-level 로깅 (SlotInsert) 사용.
    /// PageWAL이 없는 경우 WAL 로깅 없이 삽입.
    async fn insert_to_heap_with_tx(&self, data: &[u8], tx_id: TransactionId) -> Result<RecordId> {
        let mut last_pid = self.last_page_id.lock().await;

        // last_pid가 인덱스 페이지 영역이면 새 Heap 페이지 할당
        if *last_pid < Self::FIRST_HEAP_PAGE_ID {
            drop(last_pid);
            return self.allocate_first_heap_page_with_tx(data, tx_id).await;
        }

        let page_guard = self.buffer_pool.fetch_page(*last_pid).await?;
        let mut page = page_guard.lock().await;

        let mut slotted = SlottedPage::new(&mut page);

        // 페이지가 초기화되지 않았으면 초기화
        if slotted.get_free_space_ptr() == 0 {
            slotted.init(*last_pid);
        }

        // Slot-level 로깅 사용 (PageWAL이 있는 경우)
        let insert_result = if let Some(ref page_wal) = self.page_wal {
            slotted
                .insert_record_with_wal(data, page_wal, tx_id, PageStorageType::Node)
                .map(|(slot_id, _lsn)| slot_id)
        } else {
            slotted.insert_record(data)
        };

        match insert_result {
            Ok(slot_id) => {
                let rid = RecordId {
                    page_id: *last_pid,
                    slot_id,
                };
                drop(page);
                self.buffer_pool.unpin_page(*last_pid, true).await?;
                Ok(rid)
            }
            Err(_) => {
                drop(page);
                self.buffer_pool.unpin_page(*last_pid, false).await?;

                let new_page = self.buffer_pool.new_page().await?;
                let new_pid = {
                    let guard = new_page.lock().await;
                    guard.id.ok_or_else(|| GraphError::Storage {
                        message: "New page has no ID".to_string(),
                    })?
                };
                self.buffer_pool.unpin_page(new_pid, false).await?;

                let page_guard = self.buffer_pool.fetch_page(new_pid).await?;
                let mut new_page_guard = page_guard.lock().await;
                let mut slotted = SlottedPage::new(&mut new_page_guard);
                slotted.init(new_pid);

                // Slot-level 로깅 사용
                let slot_id = if let Some(ref page_wal) = self.page_wal {
                    let (sid, _lsn) = slotted.insert_record_with_wal(
                        data,
                        page_wal,
                        tx_id,
                        PageStorageType::Node,
                    )?;
                    sid
                } else {
                    slotted.insert_record(data)?
                };
                *last_pid = new_pid;

                let rid = RecordId {
                    page_id: new_pid,
                    slot_id,
                };
                drop(new_page_guard);
                self.buffer_pool.unpin_page(new_pid, true).await?;
                Ok(rid)
            }
        }
    }

    /// 첫 번째 Heap 페이지 할당 (트랜잭션 컨텍스트 포함, Slot-level WAL 로깅)
    async fn allocate_first_heap_page_with_tx(
        &self,
        data: &[u8],
        tx_id: TransactionId,
    ) -> Result<RecordId> {
        let mut last_pid = self.last_page_id.lock().await;

        let new_page = self.buffer_pool.new_page().await?;
        let new_pid = {
            let guard = new_page.lock().await;
            guard.id.ok_or_else(|| GraphError::Storage {
                message: "New page has no ID".to_string(),
            })?
        };
        self.buffer_pool.unpin_page(new_pid, false).await?;

        let page_guard = self.buffer_pool.fetch_page(new_pid).await?;
        let mut new_page_guard = page_guard.lock().await;

        let mut slotted = SlottedPage::new(&mut new_page_guard);
        slotted.init(new_pid);

        // Slot-level 로깅 사용
        let slot_id = if let Some(ref page_wal) = self.page_wal {
            let (sid, _lsn) =
                slotted.insert_record_with_wal(data, page_wal, tx_id, PageStorageType::Node)?;
            sid
        } else {
            slotted.insert_record(data)?
        };
        *last_pid = new_pid;

        let rid = RecordId {
            page_id: new_pid,
            slot_id,
        };
        drop(new_page_guard);
        self.buffer_pool.unpin_page(new_pid, true).await?;

        Ok(rid)
    }

    /// Heap에서 레코드 읽기
    ///
    /// 삭제된 슬롯(UNDO로 삭제됨)인 경우 None 반환.
    async fn read_from_heap(&self, rid: RecordId) -> Result<Option<Vec<u8>>> {
        let page_guard = self.buffer_pool.fetch_page(rid.page_id).await?;
        let mut page = page_guard.lock().await;

        let slotted = SlottedPage::new(&mut page);

        // 삭제된 슬롯(len=0)이면 None 반환
        let result = slotted.get_record(rid.slot_id).map(|data| data.to_vec());

        drop(page);
        self.buffer_pool.unpin_page(rid.page_id, false).await?;

        Ok(result)
    }
}

impl Drop for DiskNodeStorage {
    fn drop(&mut self) {
        // Note: async flush cannot be called in Drop.
        // BufferPoolManager's own Drop will handle flushing.
    }
}
