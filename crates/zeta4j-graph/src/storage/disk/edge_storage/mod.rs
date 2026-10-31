// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 디스크 기반 엣지 스토리지
//!
//! Page-level WAL은 GraphEngine에서 통합 관리됩니다.
//! - Primary Index: B+Tree (EdgeId -> RecordId)
//! - Secondary Indexes: 디스크 기반 (v3.1.0)
//!
//! ## PageWAL 통합 (v3.0.1)
//!
//! `set_page_wal()`을 통해 BufferPoolManager에 PageWAL을 설정할 수 있습니다.
//!
//! ## DiskEdgeSecondaryIndexes 통합 (v3.1.0)
//!
//! 보조 인덱스가 디스크에 영속화됩니다. 서버 재시작 후에도 rebuild가 필요 없습니다.

pub mod disk_index;
pub mod index;
pub mod types;

pub use disk_index::DiskEdgeSecondaryIndexes;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::warn;

use crate::core::{Edge, EdgeId, NodeId, PostcardSerializable};
use crate::error::{GraphError, Result};
use crate::storage::cache::BufferPoolManager;
use crate::storage::disk::manager::DiskManager;
use crate::storage::disk::slotted_page::SlottedPage;
use crate::storage::index::btree::index::BPlusTree;
use crate::storage::index::btree::types::RecordId;
use crate::storage::util::config::StorageConfig;
use crate::transaction::{PageStorageType, PageWAL};

/// 디스크 기반 엣지 스토리지
pub struct DiskEdgeStorage {
    #[allow(dead_code)]
    disk_manager: Arc<DiskManager>,
    buffer_pool: Arc<BufferPoolManager>,
    /// Primary index: EdgeId -> RecordId
    primary_index: BPlusTree,
    /// Secondary indexes (disk-based, v3.1.0)
    secondary_indexes: DiskEdgeSecondaryIndexes,
    /// Heap 파일 관리용
    last_page_id: Mutex<u32>,
    base_path: PathBuf,
    /// Page-level WAL (Slot-level 로깅용)
    page_wal: Option<Arc<PageWAL>>,
}

impl fmt::Debug for DiskEdgeStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiskEdgeStorage")
            .field("base_path", &self.base_path)
            .finish()
    }
}

impl DiskEdgeStorage {
    /// 기본 설정으로 스토리지 생성
    pub async fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        Self::with_config(base_path, StorageConfig::default()).await
    }

    /// 설정을 지정하여 스토리지 생성
    pub async fn with_config(base_path: impl AsRef<Path>, config: StorageConfig) -> Result<Self> {
        let base = base_path.as_ref().to_path_buf();
        std::fs::create_dir_all(&base)?;

        let heap_path = base.join("edges.db");

        let disk_manager = Arc::new(DiskManager::new(&heap_path).await?);
        let buffer_pool = Arc::new(BufferPoolManager::new(
            disk_manager.clone(),
            config.buffer_pool_size,
            PageStorageType::Edge,
        ));

        // 새 파일인지 확인
        let is_new_file = disk_manager.file_size().await? == 0;

        // Primary Index 초기화
        let primary_index = if is_new_file {
            BPlusTree::create_new_tree_with_metadata(&buffer_pool).await?
        } else {
            BPlusTree::recover_from_disk(buffer_pool.clone()).await?
        };

        // Secondary Indexes 초기화 (디스크 기반)
        let secondary_pool_size = config.buffer_pool_size / 3; // 각 인덱스에 1/3 할당
        let secondary_indexes =
            DiskEdgeSecondaryIndexes::open_or_create(&base, secondary_pool_size).await?;

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
            primary_index,
            secondary_indexes,
            last_page_id: Mutex::new(last_page),
            base_path: base,
            page_wal: None,
        };

        if is_new_file {
            storage.flush().await?;
        }

        Ok(storage)
    }

    /// 모든 엣지 스캔
    pub async fn scan_all(&self) -> Result<Vec<Edge>> {
        let entries = self.primary_index.scan_all().await?;
        let mut edges = Vec::with_capacity(entries.len());

        for (_edge_id, record_id) in entries {
            if let Some(edge) = self.read_from_heap(record_id).await? {
                edges.push(edge);
            }
        }

        Ok(edges)
    }

    /// 엣지 삽입
    pub async fn insert(&mut self, edge: &Edge) -> Result<()> {
        // Heap에 저장
        let bytes = edge.to_bytes()?;
        let record_id = self.insert_to_heap(&bytes).await?;

        // Primary index 업데이트
        self.primary_index.upsert(edge.id(), record_id).await?;

        // Secondary indexes 업데이트 (디스크 기반)
        self.secondary_indexes
            .insert(
                edge.id(),
                edge.from_node(),
                edge.to_node(),
                edge.relation_type(),
            )
            .await?;

        Ok(())
    }

    /// 트랜잭션 컨텍스트에서 엣지 삽입 (WAL 로깅 포함)
    ///
    /// SlotInsert 레코드를 WAL에 기록하여 롤백/복구를 지원합니다.
    pub async fn insert_with_tx(
        &mut self,
        edge: &Edge,
        tx_id: crate::transaction::TransactionId,
    ) -> Result<()> {
        // Heap에 저장 (WAL 로깅 포함)
        let bytes = edge.to_bytes()?;
        let record_id = self.insert_to_heap_with_tx(&bytes, tx_id).await?;

        // B+Tree 작업 전에 current_tx 설정
        self.buffer_pool.set_current_tx(tx_id).await;

        // Primary index 업데이트
        let result = self.primary_index.upsert(edge.id(), record_id).await;

        // current_tx 해제
        self.buffer_pool.clear_current_tx().await;

        result?;

        // Secondary indexes 업데이트 (WAL 로깅 포함)
        // Note: UNDO 시 restore_secondary_indexes/remove_from_secondary_indexes로 복구됨 (#87)
        self.secondary_indexes.set_current_tx(tx_id).await;
        self.secondary_indexes
            .insert(
                edge.id(),
                edge.from_node(),
                edge.to_node(),
                edge.relation_type(),
            )
            .await?;
        self.secondary_indexes.clear_current_tx().await;

        Ok(())
    }

    /// 엣지 조회
    pub async fn get(&self, edge_id: EdgeId) -> Result<Option<Edge>> {
        let record_id = match self.primary_index.get_value(edge_id).await? {
            Some(rid) => rid,
            None => return Ok(None),
        };

        self.read_from_heap(record_id).await
    }

    /// 엣지 삭제
    pub async fn delete(&mut self, edge_id: EdgeId) -> Result<Option<Edge>> {
        let edge = match self.get(edge_id).await? {
            Some(e) => e,
            None => return Ok(None),
        };

        // Primary index에서 삭제
        self.primary_index.delete(edge_id).await?;

        // Secondary indexes에서 삭제 (디스크 기반)
        self.secondary_indexes
            .remove(
                edge_id,
                edge.from_node(),
                edge.to_node(),
                edge.relation_type(),
            )
            .await?;

        Ok(Some(edge))
    }

    /// 엣지 개수
    pub async fn count(&self) -> Result<usize> {
        Ok(self.primary_index.scan_all().await?.len())
    }

    // === 보조 인덱스 조회 메서드 (디스크 기반) ===

    /// 특정 노드에서 나가는 엣지들
    pub async fn get_outgoing_edges(&self, node_id: NodeId) -> Vec<EdgeId> {
        self.secondary_indexes
            .get_outgoing(node_id)
            .await
            .unwrap_or_default()
    }

    /// 특정 노드로 들어오는 엣지들
    pub async fn get_incoming_edges(&self, node_id: NodeId) -> Vec<EdgeId> {
        self.secondary_indexes
            .get_incoming(node_id)
            .await
            .unwrap_or_default()
    }

    /// 특정 관계 타입의 엣지들
    pub async fn get_edges_by_type(&self, relation_type: &str) -> Vec<EdgeId> {
        self.secondary_indexes
            .get_by_type(relation_type)
            .await
            .unwrap_or_default()
    }

    /// 특정 노드와 연결된 모든 엣지들
    pub async fn get_connected_edges(&self, node_id: NodeId) -> Vec<EdgeId> {
        self.secondary_indexes
            .get_connected(node_id)
            .await
            .unwrap_or_default()
    }

    /// 노드에 연결된 엣지가 있는지
    pub async fn has_connections(&self, node_id: NodeId) -> bool {
        self.secondary_indexes
            .has_connections(node_id)
            .await
            .unwrap_or(false)
    }

    /// 두 노드 간의 엣지들 조회
    pub async fn get_edges_between(&self, from_node: NodeId, to_node: NodeId) -> Vec<EdgeId> {
        let outgoing = self
            .secondary_indexes
            .get_outgoing(from_node)
            .await
            .unwrap_or_default();
        let mut result = Vec::new();

        for edge_id in outgoing {
            if let Ok(Some(edge)) = self.get(edge_id).await
                && edge.to_node() == to_node
            {
                result.push(edge_id);
            }
        }

        result
    }

    /// 특정 노드에서 특정 관계 타입으로 나가는 엣지들
    pub async fn get_outgoing_edges_by_type(
        &self,
        node_id: NodeId,
        relation_type: &str,
    ) -> Vec<EdgeId> {
        let outgoing = self
            .secondary_indexes
            .get_outgoing(node_id)
            .await
            .unwrap_or_default();
        let mut result = Vec::new();

        for edge_id in outgoing {
            if let Ok(Some(edge)) = self.get(edge_id).await
                && edge.relation_type() == relation_type
            {
                result.push(edge_id);
            }
        }

        result
    }

    /// 엣지 존재 여부 확인
    pub async fn contains(&self, edge_id: EdgeId) -> bool {
        self.primary_index
            .get_value(edge_id)
            .await
            .ok()
            .flatten()
            .is_some()
    }

    /// 엣지 업데이트
    pub async fn update(&mut self, edge: &Edge) -> Result<()> {
        let edge_id = edge.id();

        let old_edge = match self.get(edge_id).await? {
            Some(e) => e,
            None => {
                return Err(GraphError::EdgeNotFound { id: edge_id });
            }
        };

        // 기존 인덱스에서 제거
        self.secondary_indexes
            .remove(
                edge_id,
                old_edge.from_node(),
                old_edge.to_node(),
                old_edge.relation_type(),
            )
            .await?;

        // 새 데이터로 Heap에 삽입
        let bytes = edge.to_bytes()?;
        let record_id = self.insert_to_heap(&bytes).await?;

        // Primary index 업데이트
        self.primary_index.upsert(edge_id, record_id).await?;

        // Secondary indexes 업데이트
        self.secondary_indexes
            .insert(
                edge_id,
                edge.from_node(),
                edge.to_node(),
                edge.relation_type(),
            )
            .await?;

        Ok(())
    }

    /// 트랜잭션 컨텍스트에서 엣지 업데이트 (WAL 로깅 포함)
    ///
    /// 기존 슬롯 삭제 (SlotDelete) + 새 슬롯 삽입 (SlotInsert)
    /// 롤백 시 SlotDelete UNDO → SlotInsert UNDO 순서로 복구됩니다.
    pub async fn update_with_tx(
        &mut self,
        edge: &Edge,
        tx_id: crate::transaction::TransactionId,
    ) -> Result<()> {
        let edge_id = edge.id();

        // 기존 레코드 ID 조회
        let old_record_id = self.primary_index.get_value(edge_id).await?;

        if let Some(old_rid) = old_record_id {
            // 기존 슬롯 삭제 (SlotDelete 로깅)
            self.delete_slot_with_tx(old_rid, tx_id).await?;
        }

        // 새 데이터 삽입 (SlotInsert 로깅)
        let bytes = edge.to_bytes()?;
        let new_record_id = self.insert_to_heap_with_tx(&bytes, tx_id).await?;

        // B+Tree 인덱스 업데이트
        self.buffer_pool.set_current_tx(tx_id).await;

        let result = self.primary_index.upsert(edge_id, new_record_id).await;

        self.buffer_pool.clear_current_tx().await;

        result?;

        // Secondary indexes는 edge 자체가 변경되지 않으면 그대로 유지
        // (from_node, to_node, relation_type이 변경되면 업데이트 필요)

        Ok(())
    }

    /// 트랜잭션 컨텍스트에서 엣지 삭제 (WAL 로깅 포함)
    ///
    /// SlotDelete 레코드를 WAL에 기록하여 롤백/복구를 지원합니다.
    pub async fn delete_with_tx(
        &mut self,
        edge_id: EdgeId,
        tx_id: crate::transaction::TransactionId,
    ) -> Result<Option<Edge>> {
        // 먼저 엣지 조회 (Secondary index 정리용)
        let edge = match self.get(edge_id).await? {
            Some(e) => e,
            None => return Ok(None),
        };

        // Secondary indexes에서 삭제 (WAL 로깅 포함)
        self.secondary_indexes.set_current_tx(tx_id).await;
        self.secondary_indexes
            .remove(
                edge_id,
                edge.from_node(),
                edge.to_node(),
                edge.relation_type(),
            )
            .await?;
        self.secondary_indexes.clear_current_tx().await;

        // 레코드 ID 조회
        let record_id = match self.primary_index.get_value(edge_id).await? {
            Some(rid) => rid,
            None => return Ok(None),
        };

        // 슬롯 삭제 (SlotDelete 로깅)
        self.delete_slot_with_tx(record_id, tx_id).await?;

        // B+Tree에서 삭제
        self.buffer_pool.set_current_tx(tx_id).await;

        let _deleted = self.primary_index.delete(edge_id).await?;

        self.buffer_pool.clear_current_tx().await;

        Ok(Some(edge))
    }

    /// 슬롯 삭제 (WAL 로깅)
    async fn delete_slot_with_tx(
        &self,
        record_id: RecordId,
        tx_id: crate::transaction::TransactionId,
    ) -> Result<()> {
        let page_guard = self.buffer_pool.fetch_page(record_id.page_id).await?;
        let mut page = page_guard.lock().await;

        let mut slotted = SlottedPage::new(&mut page);

        // Slot-level 로깅 사용 (PageWAL이 있는 경우)
        if let Some(ref page_wal) = self.page_wal {
            slotted.delete_record_with_wal(
                record_id.slot_id,
                page_wal,
                tx_id,
                PageStorageType::Edge,
            )?;
        } else {
            slotted.delete_record(record_id.slot_id)?;
        }

        drop(page);
        self.buffer_pool.unpin_page(record_id.page_id, true).await?;

        Ok(())
    }

    /// 모든 엣지 삭제
    pub async fn clear(&mut self) -> Result<()> {
        let all_entries = self.primary_index.scan_all().await?;
        let edge_ids: Vec<EdgeId> = all_entries.into_iter().map(|(id, _)| id).collect();

        for edge_id in edge_ids {
            self.delete(edge_id).await?;
        }

        Ok(())
    }

    // === Heap 관리 ===

    /// 첫 번째 Heap 페이지 ID (Page 0, 1은 인덱스용)
    const FIRST_HEAP_PAGE_ID: u32 = 2;

    /// Heap에 데이터 삽입
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

    /// Heap에 데이터 삽입 (WAL 로깅 포함)
    async fn insert_to_heap_with_tx(
        &self,
        data: &[u8],
        tx_id: crate::transaction::TransactionId,
    ) -> Result<RecordId> {
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
                .insert_record_with_wal(data, page_wal, tx_id, PageStorageType::Edge)
                .map(|(slot_id, _lsn)| slot_id)
        } else {
            warn!("Edge insert without WAL - page_wal is None!");
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
                let mut new_page_guard = new_page.lock().await;
                let new_pid = new_page_guard.id.ok_or_else(|| GraphError::Storage {
                    message: "New page has no ID".to_string(),
                })?;
                let mut slotted = SlottedPage::new(&mut new_page_guard);
                slotted.init(new_pid);

                // Slot-level 로깅 사용 (PageWAL이 있는 경우)
                let slot_id = if let Some(ref page_wal) = self.page_wal {
                    slotted
                        .insert_record_with_wal(data, page_wal, tx_id, PageStorageType::Edge)
                        .map(|(slot_id, _lsn)| slot_id)?
                } else {
                    warn!("Edge insert (new page) without WAL - page_wal is None!");
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

    /// 첫 번째 Heap 페이지 할당 및 데이터 삽입 (WAL 로깅 포함)
    async fn allocate_first_heap_page_with_tx(
        &self,
        data: &[u8],
        tx_id: crate::transaction::TransactionId,
    ) -> Result<RecordId> {
        let mut last_pid = self.last_page_id.lock().await;

        let new_page = self.buffer_pool.new_page().await?;
        let mut new_page_guard = new_page.lock().await;
        let new_pid = new_page_guard.id.ok_or_else(|| GraphError::Storage {
            message: "New page has no ID".to_string(),
        })?;

        let mut slotted = SlottedPage::new(&mut new_page_guard);
        slotted.init(new_pid);

        // Slot-level 로깅 사용 (PageWAL이 있는 경우)
        let slot_id = if let Some(ref page_wal) = self.page_wal {
            slotted
                .insert_record_with_wal(data, page_wal, tx_id, PageStorageType::Edge)
                .map(|(slot_id, _lsn)| slot_id)?
        } else {
            warn!("Edge insert (first heap page) without WAL - page_wal is None!");
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

    /// Heap에서 데이터 읽기
    async fn read_from_heap(&self, record_id: RecordId) -> Result<Option<Edge>> {
        let page_guard = self.buffer_pool.fetch_page(record_id.page_id).await?;
        let mut page = page_guard.lock().await;

        let slotted = SlottedPage::new(&mut page);

        let data = match slotted.get_record(record_id.slot_id) {
            Some(d) => d.to_vec(),
            None => {
                drop(page);
                self.buffer_pool
                    .unpin_page(record_id.page_id, false)
                    .await?;
                return Ok(None);
            }
        };

        drop(page);
        self.buffer_pool
            .unpin_page(record_id.page_id, false)
            .await?;

        let edge = Edge::from_bytes(&data)?;
        Ok(Some(edge))
    }

    /// 버퍼 풀 플러시 (Primary + Secondary Indexes)
    pub async fn flush(&self) -> Result<()> {
        // Primary buffer pool flush
        self.buffer_pool.flush_all_pages().await?;

        // Secondary indexes flush (v3.1.0)
        self.secondary_indexes.flush().await?;

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

    /// Secondary Index BufferPoolManagers 반환 (WAL 복구용)
    pub fn secondary_index_buffer_pools(&self) -> Vec<Arc<BufferPoolManager>> {
        self.secondary_indexes.buffer_pool_managers()
    }

    /// PageWAL 설정 (GraphEngine에서 호출)
    ///
    /// BufferPoolManager에 PageWAL을 설정합니다.
    /// (DiskEdgeStorage는 현재 트랜잭션 컨텍스트 없이 동작)
    pub async fn set_page_wal(&mut self, page_wal: Arc<PageWAL>) -> Result<()> {
        // Heap + Primary B+Tree BufferPoolManager에 WAL 연결
        self.buffer_pool.set_page_wal(page_wal.clone()).await;
        // Secondary Indexes (Outgoing, Incoming, Type) BufferPoolManager에 WAL 연결
        self.secondary_indexes.set_page_wal(page_wal.clone()).await;
        self.page_wal = Some(page_wal);
        Ok(())
    }

    // ========== UNDO 지원 메서드 ==========

    /// UNDO 시 Primary Index 복구
    ///
    /// 트랜잭션 롤백 시 SlotDelete의 UNDO로 호출됩니다.
    /// B+Tree에 edge_id → RecordId 매핑을 복원합니다.
    pub async fn restore_index(
        &mut self,
        edge_id: EdgeId,
        page_id: crate::storage::disk::page::PageId,
        slot_id: crate::storage::disk::slotted_page::SlotId,
    ) -> Result<()> {
        let record_id = RecordId { page_id, slot_id };
        self.primary_index.insert(edge_id, record_id).await?;
        Ok(())
    }

    /// UNDO 시 Secondary Indexes 복구
    ///
    /// 트랜잭션 롤백 시 SlotDelete의 UNDO로 호출됩니다.
    /// outgoing, incoming, type 인덱스에 엣지 정보를 복원합니다.
    pub async fn restore_secondary_indexes(
        &mut self,
        edge_id: EdgeId,
        from_node: NodeId,
        to_node: NodeId,
        relation_type: &str,
    ) -> Result<()> {
        self.secondary_indexes
            .insert(edge_id, from_node, to_node, relation_type)
            .await
    }

    /// UNDO 시 Secondary Indexes에서 제거
    ///
    /// 트랜잭션 롤백 시 SlotInsert의 UNDO로 호출됩니다.
    /// outgoing, incoming, type 인덱스에서 엣지 정보를 제거합니다.
    pub async fn remove_from_secondary_indexes(
        &mut self,
        edge_id: EdgeId,
        from_node: NodeId,
        to_node: NodeId,
        relation_type: &str,
    ) -> Result<()> {
        self.secondary_indexes
            .remove(edge_id, from_node, to_node, relation_type)
            .await
    }

    /// UNDO 시 B+Tree 인덱스에서 엣지 제거
    ///
    /// SlotInsert UNDO 시 B+Tree 인덱스에서 엣지 엔트리를 제거합니다.
    /// 슬롯 페이지에서는 이미 삭제되었으므로 인덱스만 정리합니다.
    pub async fn undo_insert(&mut self, edge_id: EdgeId) -> Result<()> {
        self.primary_index.delete(edge_id).await?;
        Ok(())
    }
}

impl Drop for DiskEdgeStorage {
    fn drop(&mut self) {
        // Note: async flush cannot be called in Drop.
        // BufferPoolManager's own Drop will handle flushing.
    }
}
