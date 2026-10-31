// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 디스크 기반 노드 스토리지
//!
//! v1.9.1: Memory 백엔드 제거 - Disk 백엔드만 사용하여 동작 일관성 확보

use crate::core::{Node, NodeId, PropertyContainer};
use crate::error::{GraphError, Result};
use crate::transaction::{PageWAL, TransactionId};
use indexmap::IndexSet;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::RwLock;

use super::common::InternalElementStats;
use crate::storage::disk::node_storage::DiskNodeStorage;

/// 디스크 기반 노드 스토리지
///
/// v1.9.1: Memory 백엔드 제거 - Disk 백엔드만 사용
#[derive(Debug)]
pub struct NodeStorage {
    /// 디스크 스토리지 (내부 Lock으로 세분화된 동시성)
    disk: RwLock<DiskNodeStorage>,

    /// 다음 노드 ID 생성기 (원자적 카운터)
    next_node_id: AtomicU64,

    /// 스토리지 통계
    stats: RwLock<InternalElementStats>,
}

impl NodeStorage {
    /// 디스크 기반 노드 스토리지 생성
    ///
    /// v1.9.1: Memory 백엔드 제거로 new_disk()가 유일한 생성자
    pub async fn new_disk(path: impl AsRef<Path>) -> Result<Self> {
        let storage = DiskNodeStorage::new(path).await?;
        Ok(Self {
            disk: RwLock::new(storage),
            next_node_id: AtomicU64::new(1),
            stats: RwLock::new(InternalElementStats::default()),
        })
    }

    pub fn generate_id(&self) -> NodeId {
        self.next_node_id.fetch_add(1, Ordering::SeqCst)
    }

    pub async fn insert(&self, node: Node) -> Result<()> {
        let node_id = node.id();
        let memory_size = node.memory_size();

        {
            let mut disk = self.disk.write().await;
            if disk.get(node_id).await?.is_some() {
                return Err(GraphError::Validation {
                    message: format!("Node ID {} already exists", node_id),
                });
            }
            disk.insert(&node).await?;
        }

        let mut stats = self.stats.write().await;
        stats.total_elements += 1;
        stats.memory_usage += memory_size;
        stats.insert_count += 1;

        Ok(())
    }

    /// 노드 삽입 (트랜잭션 컨텍스트 포함, WAL 로깅)
    ///
    /// PageWAL에 페이지 변경을 기록합니다.
    pub async fn insert_with_tx(&self, node: Node, tx_id: TransactionId) -> Result<()> {
        let node_id = node.id();
        let memory_size = node.memory_size();

        {
            let mut disk = self.disk.write().await;
            if disk.get(node_id).await?.is_some() {
                return Err(GraphError::Validation {
                    message: format!("Node ID {} already exists", node_id),
                });
            }
            disk.insert_with_tx(&node, tx_id).await?;
        }

        let mut stats = self.stats.write().await;
        stats.total_elements += 1;
        stats.memory_usage += memory_size;
        stats.insert_count += 1;

        Ok(())
    }

    pub async fn upsert(&self, node: Node) -> Result<()> {
        let new_size = node.memory_size();

        {
            let mut disk = self.disk.write().await;
            disk.insert(&node).await?;
        }

        let mut stats = self.stats.write().await;
        stats.total_elements += 1;
        stats.insert_count += 1;
        stats.memory_usage += new_size;

        Ok(())
    }

    /// 노드 upsert (WAL 로깅 포함)
    pub async fn upsert_with_tx(&self, node: Node, tx_id: TransactionId) -> Result<()> {
        let new_size = node.memory_size();

        {
            let mut disk = self.disk.write().await;
            disk.upsert_with_tx(&node, tx_id).await?;
        }

        let mut stats = self.stats.write().await;
        stats.update_count += 1;
        stats.memory_usage += new_size;

        Ok(())
    }

    pub async fn create(&self) -> Result<NodeId> {
        let node_id = self.next_node_id.fetch_add(1, Ordering::SeqCst);
        let node = Node::new(node_id);
        self.insert(node).await?;
        Ok(node_id)
    }

    pub async fn count(&self) -> Result<usize> {
        self.disk.read().await.count().await
    }

    /// 모든 노드 ID 반환
    pub async fn all_ids(&self) -> Vec<NodeId> {
        self.disk
            .read()
            .await
            .range_scan(0, u64::MAX)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|node| node.id())
            .collect()
    }

    /// 모든 노드 스캔 (복구 시 GraphIndex 재구축용)
    ///
    /// B+Tree leaf chain 무결성을 검증하고, 누락된 키가 있으면
    /// 개별 get으로 보완하여 완전한 결과를 반환합니다.
    pub async fn scan_all_nodes(&self) -> Result<Vec<Node>> {
        let disk = self.disk.read().await;
        let mut nodes = disk.range_scan(0, u64::MAX).await?;

        // leaf chain 무결성 검증
        let report = disk.verify_leaf_chain().await?;
        if !report.is_valid && !report.missing_keys.is_empty() {
            tracing::warn!(
                missing_count = report.missing_keys.len(),
                "B+Tree leaf chain has missing keys, recovering via individual get"
            );
            tracing::warn!("Missing keys: {:?}", &report.missing_keys);

            let existing_ids: std::collections::HashSet<u64> =
                nodes.iter().map(|n| n.id()).collect();

            for &key in &report.missing_keys {
                if !existing_ids.contains(&key)
                    && let Some(node) = disk.get(key).await?
                {
                    nodes.push(node);
                }
            }

            // ID 순서로 정렬
            nodes.sort_by_key(|n| n.id());
        }

        Ok(nodes)
    }

    pub async fn get(&self, node_id: NodeId) -> Result<Option<Node>> {
        self.stats.write().await.lookup_count += 1;
        self.disk.read().await.get(node_id).await
    }

    pub async fn get_ref<F, R>(&self, node_id: NodeId, f: F) -> Result<Option<R>>
    where
        F: FnOnce(&Node) -> R,
    {
        self.stats.write().await.lookup_count += 1;

        if let Some(node) = self.disk.read().await.get(node_id).await? {
            Ok(Some(f(&node)))
        } else {
            Ok(None)
        }
    }

    pub async fn update<F>(&self, node_id: NodeId, f: F) -> Result<()>
    where
        F: FnOnce(&mut Node),
    {
        {
            let mut disk = self.disk.write().await;
            if let Some(mut node) = disk.get(node_id).await? {
                f(&mut node);
                disk.insert(&node).await?;
            } else {
                return Err(GraphError::NodeNotFound { id: node_id });
            }
        }

        let mut stats = self.stats.write().await;
        stats.update_count += 1;
        Ok(())
    }

    pub async fn remove(&self, node_id: NodeId) -> Result<Node> {
        let node = {
            let mut disk = self.disk.write().await;
            let node = disk
                .get(node_id)
                .await?
                .ok_or(GraphError::NodeNotFound { id: node_id })?;
            disk.delete(node_id).await?;
            node
        };

        let mut stats = self.stats.write().await;
        stats.total_elements = stats.total_elements.saturating_sub(1);
        stats.delete_count += 1;
        Ok(node)
    }

    /// 트랜잭션 컨텍스트에서 노드 삭제 (WAL 로깅 포함)
    ///
    /// SlotDelete 레코드를 WAL에 기록하여 롤백/복구를 지원합니다.
    pub async fn remove_with_tx(&self, node_id: NodeId, tx_id: TransactionId) -> Result<Node> {
        let node = {
            let mut disk = self.disk.write().await;
            let node = disk
                .get(node_id)
                .await?
                .ok_or(GraphError::NodeNotFound { id: node_id })?;
            disk.delete_with_tx(node_id, tx_id).await?;
            node
        };

        let mut stats = self.stats.write().await;
        stats.total_elements = stats.total_elements.saturating_sub(1);
        stats.delete_count += 1;
        Ok(node)
    }

    /// UNDO 시 인덱스 복구
    ///
    /// SlotDelete UNDO 후 인덱스를 새 RecordId로 업데이트합니다.
    pub async fn restore_index(
        &self,
        node_id: NodeId,
        page_id: crate::storage::disk::page::PageId,
        slot_id: u16,
    ) -> Result<()> {
        use crate::storage::index::btree::types::RecordId;
        let record_id = RecordId { page_id, slot_id };
        self.disk
            .write()
            .await
            .restore_index(node_id, record_id)
            .await
    }

    /// UNDO 시 레이블 인덱스 복구
    ///
    /// SlotDelete UNDO 후 레이블 인덱스에 노드를 다시 추가합니다.
    pub async fn restore_label_index(
        &self,
        node_id: NodeId,
        labels: &IndexSet<String>,
    ) -> Result<()> {
        self.disk
            .write()
            .await
            .restore_label_index(node_id, labels)
            .await
    }

    /// UNDO 시 레이블 인덱스에서 제거
    ///
    /// SlotInsert UNDO 시 레이블 인덱스에서 노드를 제거합니다.
    pub async fn remove_from_label_index(
        &self,
        node_id: NodeId,
        labels: &IndexSet<String>,
    ) -> Result<()> {
        self.disk
            .write()
            .await
            .remove_from_label_index(node_id, labels)
            .await
    }

    /// UNDO 시 B+Tree 인덱스에서 제거 + 통계 갱신
    ///
    /// SlotInsert UNDO 시:
    /// 1. B+Tree 인덱스에서 노드 엔트리 제거
    /// 2. 노드 카운트 감소
    pub async fn undo_insert(&self, node_id: NodeId) -> Result<()> {
        self.disk.write().await.undo_insert(node_id).await?;
        let current_total = self.stats.read().await.total_elements;
        self.stats.write().await.total_elements = current_total.saturating_sub(1);
        Ok(())
    }

    /// 버퍼 풀의 모든 dirty 페이지를 디스크에 flush
    ///
    /// 커밋 시 WAL flush 후 호출하여 데이터 영속성을 보장합니다.
    pub async fn flush(&self) -> Result<()> {
        self.disk.read().await.flush().await
    }

    /// B+Tree leaf chain 무결성 진단
    pub async fn verify_leaf_chain(
        &self,
    ) -> Result<crate::storage::index::btree::index::LeafChainReport> {
        self.disk.read().await.verify_leaf_chain().await
    }

    /// last_page_id 재계산 (WAL 복구 후 호출)
    ///
    /// 디스크 파일 크기를 기반으로 last_page_id를 다시 계산합니다.
    pub async fn recalculate_last_page_id(&self) -> Result<()> {
        self.disk.read().await.recalculate_last_page_id().await
    }

    pub async fn contains_node(&self, node_id: NodeId) -> bool {
        self.disk
            .read()
            .await
            .get(node_id)
            .await
            .unwrap_or(None)
            .is_some()
    }

    // 추가 조회 메서드
    pub async fn node_ids(&self) -> Vec<NodeId> {
        self.all_ids().await
    }

    pub async fn find_nodes_by_label(&self, label: &str) -> Vec<NodeId> {
        self.disk
            .read()
            .await
            .find_by_label(label)
            .await
            .unwrap_or_default()
    }

    pub async fn find_nodes_by_property(
        &self,
        key: &str,
        value: &crate::core::Value,
    ) -> Vec<NodeId> {
        self.disk
            .read()
            .await
            .range_scan(0, u64::MAX)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|node| node.get_property(key) == Some(value))
            .map(|node| node.id())
            .collect()
    }

    pub async fn find_nodes<F>(&self, predicate: F) -> Vec<NodeId>
    where
        F: Fn(&Node) -> bool,
    {
        self.disk
            .read()
            .await
            .range_scan(0, u64::MAX)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|node| predicate(node))
            .map(|node| node.id())
            .collect()
    }

    pub async fn node_count(&self) -> usize {
        self.stats.read().await.total_elements
    }

    pub async fn memory_usage(&self) -> usize {
        self.stats.read().await.memory_usage
    }

    pub async fn stats(&self) -> NodeStorageStats {
        let stats = self.stats.read().await;
        NodeStorageStats {
            total_nodes: stats.total_elements,
            memory_usage: stats.memory_usage,
            insert_count: stats.insert_count,
            lookup_count: stats.lookup_count,
            update_count: stats.update_count,
            delete_count: stats.delete_count,
        }
    }

    pub async fn clear(&self) {
        // 디스크 파일은 런타임에 쉽게 지울 수 없음
        *self.stats.write().await = InternalElementStats::default();
        self.next_node_id.store(1, Ordering::SeqCst);
    }

    /// 다음 노드 ID 설정 (복구 시 사용)
    ///
    /// WAL 복구 후 복원된 노드들의 최대 ID를 기반으로
    /// 다음 ID를 설정합니다.
    pub fn set_next_id(&self, next_id: NodeId) {
        self.next_node_id.store(next_id, Ordering::SeqCst);
    }

    /// 현재 다음 노드 ID 반환
    pub fn get_next_id(&self) -> NodeId {
        self.next_node_id.load(Ordering::SeqCst)
    }

    /// 최대 노드 ID 찾기 (복구 시 사용)
    pub async fn find_max_node_id(&self) -> Option<NodeId> {
        self.disk
            .read()
            .await
            .range_scan(0, u64::MAX)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|node| node.id())
            .max()
    }

    /// BufferPoolManager 접근 (Page-level WAL 통합용)
    pub async fn get_buffer_pool(
        &self,
    ) -> Option<std::sync::Arc<crate::storage::cache::BufferPoolManager>> {
        Some(self.disk.read().await.buffer_pool())
    }

    /// Label BufferPoolManager 접근 (WAL 복구용)
    pub async fn get_label_buffer_pool(
        &self,
    ) -> std::sync::Arc<crate::storage::cache::BufferPoolManager> {
        self.disk.read().await.label_buffer_pool()
    }

    /// PageWAL 설정
    pub async fn set_page_wal(&self, page_wal: Arc<PageWAL>) -> Result<()> {
        self.disk.write().await.set_page_wal(page_wal).await
    }

    /// 항상 Disk 모드 (v1.9.1: Memory 백엔드 제거)
    pub fn is_disk_mode(&self) -> bool {
        true
    }

    pub fn compact(&self) {
        // 디스크 모드에서는 별도 compaction 필요 없음
    }

    /// 배치 노드 삽입 (최적화)
    ///
    /// Lock을 한 번만 획득하여 여러 노드를 효율적으로 삽입합니다.
    pub async fn batch_insert(&self, nodes: Vec<Node>) -> Result<()> {
        if nodes.is_empty() {
            return Ok(());
        }

        let (count, total_size) = {
            let mut disk = self.disk.write().await;
            let mut count = 0usize;
            let mut total_size = 0usize;
            for node in nodes {
                let node_id = node.id();
                if disk.get(node_id).await?.is_some() {
                    return Err(GraphError::Validation {
                        message: format!("Node ID {} already exists", node_id),
                    });
                }
                total_size += node.memory_size();
                disk.insert(&node).await?;
                count += 1;
            }
            (count, total_size)
        };

        let mut stats = self.stats.write().await;
        stats.total_elements += count;
        stats.memory_usage += total_size;
        stats.insert_count += count as u64;

        Ok(())
    }

    pub async fn batch_insert_design(&self, nodes: Vec<Node>) -> Result<()> {
        self.batch_insert(nodes).await
    }

    pub async fn create_design_snapshot(&self) -> StorageSnapshot {
        let nodes = self
            .disk
            .read()
            .await
            .range_scan(0, u64::MAX)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|node| (node.id(), node))
            .collect();

        StorageSnapshot {
            nodes,
            timestamp: Instant::now(),
        }
    }

    pub fn compact_design(&self) -> Result<DesignCompactionStats> {
        self.compact();
        Ok(DesignCompactionStats {
            before: 0,
            after: 0,
            reclaimed: 0,
        })
    }
}

// Stats & Snapshots Structs (Copied from original)
#[derive(Debug, Clone)]
pub struct NodeStorageStats {
    pub total_nodes: usize,
    pub memory_usage: usize,
    pub insert_count: u64,
    pub lookup_count: u64,
    pub update_count: u64,
    pub delete_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStorageSnapshot {
    pub nodes: HashMap<NodeId, Node>,
    pub timestamp_nanos: u64,
}

impl NodeStorageSnapshot {
    pub fn new(nodes: HashMap<NodeId, Node>) -> Self {
        Self {
            nodes,
            timestamp_nanos: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        }
    }
}

/// 설계 문서 호환 타입들
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSnapshot {
    pub nodes: HashMap<NodeId, Node>,
    #[serde(skip)]
    #[serde(default = "std::time::Instant::now")]
    pub timestamp: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct DesignCompactionStats {
    pub before: usize,
    pub after: usize,
    pub reclaimed: usize,
}
