//! 디스크 기반 엣지 스토리지
//!
//! v1.9.1: Memory 백엔드 제거 - Disk 백엔드만 사용하여 동작 일관성 확보
//!
//! 이 모듈은 엣지 데이터를 저장하고 관리합니다.
//! - B+Tree + Heap + WAL 기반 영속화
//!
//! 서브모듈:
//! - `types`: 타입 정의 (스냅샷, 통계, 패턴 등)
//! - `index`: 인덱스 조회 메서드
//! - `batch`: 배치 연산
//! - `snapshot`: 스냅샷/체크포인트
//! - `pattern`: 패턴 매칭/순회

mod batch;
mod index;
mod pattern;
mod snapshot;
pub mod types;

use crate::core::{Edge, EdgeId, NodeId};
use crate::error::{GraphError, Result};
use crate::storage::disk::edge_storage::DiskEdgeStorage;
use crate::transaction::PageWAL;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

use crate::storage::graph::common::InternalElementStats;
pub use types::{
    EdgeCompactionStats, EdgeStorageSnapshot, EdgeStorageStats, EdgeTransactionSnapshot,
    RelationshipPattern,
};

/// 디스크 기반 엣지 스토리지
///
/// v1.9.1: Memory 백엔드 제거 - Disk 백엔드만 사용
#[derive(Debug)]
pub struct EdgeStorage {
    /// 디스크 스토리지 (내부 Lock으로 세분화된 동시성)
    disk: RwLock<DiskEdgeStorage>,

    /// 다음 엣지 ID 생성기
    pub next_edge_id: AtomicU64,

    /// 스토리지 통계
    pub(crate) stats: RwLock<InternalElementStats>,
}

impl EdgeStorage {
    /// 디스크 기반 엣지 스토리지 생성
    ///
    /// v1.9.1: Memory 백엔드 제거로 new_disk()가 유일한 생성자
    pub async fn new_disk(path: impl AsRef<Path>) -> Result<Self> {
        let disk_storage = DiskEdgeStorage::new(path).await?;

        // 기존 엣지에서 max ID 찾기
        let max_id = disk_storage
            .scan_all()
            .await?
            .iter()
            .map(|e| e.id())
            .max()
            .unwrap_or(0);

        Ok(Self {
            disk: RwLock::new(disk_storage),
            next_edge_id: AtomicU64::new(max_id + 1),
            stats: RwLock::new(InternalElementStats::default()),
        })
    }

    /// 새로운 엣지 생성 및 저장
    pub async fn create<S: Into<String>>(
        &self,
        relation_type: S,
        from_node: NodeId,
        to_node: NodeId,
    ) -> Result<EdgeId> {
        let edge_id = self.next_edge_id.fetch_add(1, Ordering::SeqCst);
        let edge = Edge::new(edge_id, relation_type, from_node, to_node);

        self.insert(edge).await?;
        Ok(edge_id)
    }

    /// 엣지 삽입
    pub async fn insert(&self, edge: Edge) -> Result<()> {
        let memory_size = edge.memory_size();

        {
            let mut disk = self.disk.write().await;
            disk.insert(&edge).await?;
        }

        // 통계 업데이트
        let mut stats = self.stats.write().await;
        stats.total_elements += 1;
        stats.memory_usage += memory_size;
        stats.insert_count += 1;

        Ok(())
    }

    /// 트랜잭션 컨텍스트에서 엣지 삽입 (WAL 로깅 포함)
    ///
    /// SlotInsert 레코드를 WAL에 기록하여 롤백/복구를 지원합니다.
    pub async fn insert_with_tx(
        &self,
        edge: Edge,
        tx_id: crate::transaction::TransactionId,
    ) -> Result<()> {
        let memory_size = edge.memory_size();

        {
            let mut disk = self.disk.write().await;
            disk.insert_with_tx(&edge, tx_id).await?;
        }

        // 통계 업데이트
        let mut stats = self.stats.write().await;
        stats.total_elements += 1;
        stats.memory_usage += memory_size;
        stats.insert_count += 1;

        Ok(())
    }

    /// 엣지 조회
    pub async fn get(&self, edge_id: EdgeId) -> Result<Option<Edge>> {
        self.stats.write().await.lookup_count += 1;
        self.disk.read().await.get(edge_id).await
    }

    /// 엣지 조회 (참조)
    pub async fn get_ref<F, R>(&self, edge_id: EdgeId, f: F) -> Result<Option<R>>
    where
        F: FnOnce(&Edge) -> R,
    {
        self.stats.write().await.lookup_count += 1;
        Ok(self.disk.read().await.get(edge_id).await?.map(|e| f(&e)))
    }

    /// 엣지 업데이트
    pub async fn update<F>(&self, edge_id: EdgeId, f: F) -> Result<()>
    where
        F: FnOnce(&mut Edge),
    {
        let mut disk = self.disk.write().await;
        let mut stats = self.stats.write().await;

        // Disk에서 읽어서 업데이트 후 다시 저장
        match disk.get(edge_id).await? {
            Some(mut edge) => {
                let old_size = edge.memory_size();
                f(&mut edge);
                let new_size = edge.memory_size();

                disk.update(&edge).await?;

                // 메모리 사용량 업데이트
                if new_size > old_size {
                    stats.memory_usage += new_size - old_size;
                } else {
                    stats.memory_usage = stats.memory_usage.saturating_sub(old_size - new_size);
                }

                stats.update_count += 1;
                Ok(())
            }
            None => Err(GraphError::EdgeNotFound { id: edge_id }),
        }
    }

    /// 트랜잭션 컨텍스트에서 엣지 업데이트 (WAL 로깅 포함)
    ///
    /// SlotDelete + SlotInsert를 WAL에 기록하여 롤백/복구를 지원합니다.
    pub async fn update_with_tx<F>(
        &self,
        edge_id: EdgeId,
        f: F,
        tx_id: crate::transaction::TransactionId,
    ) -> Result<()>
    where
        F: FnOnce(&mut Edge),
    {
        let mut disk = self.disk.write().await;
        let mut stats = self.stats.write().await;

        // Disk에서 읽어서 업데이트 후 다시 저장 (WAL 로깅 포함)
        match disk.get(edge_id).await? {
            Some(mut edge) => {
                let old_size = edge.memory_size();
                f(&mut edge);
                let new_size = edge.memory_size();

                disk.update_with_tx(&edge, tx_id).await?;

                // 메모리 사용량 업데이트
                if new_size > old_size {
                    stats.memory_usage += new_size - old_size;
                } else {
                    stats.memory_usage = stats.memory_usage.saturating_sub(old_size - new_size);
                }

                stats.update_count += 1;
                Ok(())
            }
            None => Err(GraphError::EdgeNotFound { id: edge_id }),
        }
    }

    /// 엣지 삭제
    pub async fn remove(&self, edge_id: EdgeId) -> Result<Edge> {
        let edge = {
            let mut disk = self.disk.write().await;
            disk.delete(edge_id)
                .await?
                .ok_or(GraphError::EdgeNotFound { id: edge_id })?
        };

        // 통계 업데이트
        let mut stats = self.stats.write().await;
        stats.total_elements = stats.total_elements.saturating_sub(1);
        stats.memory_usage = stats.memory_usage.saturating_sub(edge.memory_size());
        stats.delete_count += 1;

        Ok(edge)
    }

    /// 트랜잭션 컨텍스트에서 엣지 삭제 (WAL 로깅 포함)
    ///
    /// SlotDelete 레코드를 WAL에 기록하여 롤백/복구를 지원합니다.
    pub async fn remove_with_tx(
        &self,
        edge_id: EdgeId,
        tx_id: crate::transaction::TransactionId,
    ) -> Result<Edge> {
        let edge = {
            let mut disk = self.disk.write().await;
            disk.delete_with_tx(edge_id, tx_id)
                .await?
                .ok_or(GraphError::EdgeNotFound { id: edge_id })?
        };

        // 통계 업데이트
        let mut stats = self.stats.write().await;
        stats.total_elements = stats.total_elements.saturating_sub(1);
        stats.memory_usage = stats.memory_usage.saturating_sub(edge.memory_size());
        stats.delete_count += 1;

        Ok(edge)
    }

    /// 엣지 존재 여부 확인
    pub async fn contains_edge(&self, edge_id: EdgeId) -> bool {
        self.disk.read().await.contains(edge_id).await
    }

    /// 모든 엣지 ID 반복자
    pub async fn edge_ids(&self) -> Vec<EdgeId> {
        self.disk
            .read()
            .await
            .scan_all()
            .await
            .unwrap_or_default()
            .iter()
            .map(|e| e.id())
            .collect()
    }

    /// 조건부 업데이트 (Compare-and-Swap)
    pub async fn update_if_match<P, U>(
        &self,
        edge_id: EdgeId,
        predicate: P,
        update: U,
    ) -> Result<bool>
    where
        P: FnOnce(&Edge) -> bool,
        U: FnOnce(&mut Edge),
    {
        let mut disk = self.disk.write().await;

        match disk.get(edge_id).await? {
            Some(mut edge) => {
                if predicate(&edge) {
                    let mut stats = self.stats.write().await;
                    let old_size = edge.memory_size();
                    update(&mut edge);
                    let new_size = edge.memory_size();

                    disk.update(&edge).await?;

                    // 메모리 사용량 업데이트
                    if new_size > old_size {
                        stats.memory_usage += new_size - old_size;
                    } else {
                        stats.memory_usage = stats.memory_usage.saturating_sub(old_size - new_size);
                    }

                    stats.update_count += 1;
                    return Ok(true);
                }
                Ok(false)
            }
            None => Ok(false),
        }
    }

    /// 총 엣지 개수
    pub async fn edge_count(&self) -> usize {
        self.stats.read().await.total_elements
    }

    /// 모든 엣지 ID 반환
    pub async fn all_ids(&self) -> Vec<EdgeId> {
        self.disk
            .read()
            .await
            .scan_all()
            .await
            .unwrap_or_default()
            .iter()
            .map(|e| e.id())
            .collect()
    }

    /// 모든 엣지 스캔 (복구 시 GraphIndex 재구축용)
    pub async fn scan_all_edges(&self) -> Result<Vec<Edge>> {
        self.disk.read().await.scan_all().await
    }

    /// 메모리 사용량 (바이트)
    pub async fn memory_usage(&self) -> usize {
        self.stats.read().await.memory_usage
    }

    /// 스토리지 통계 조회
    pub async fn stats(&self) -> EdgeStorageStats {
        let stats = self.stats.read().await;
        EdgeStorageStats {
            total_edges: stats.total_elements,
            memory_usage: stats.memory_usage,
            insert_count: stats.insert_count,
            lookup_count: stats.lookup_count,
            update_count: stats.update_count,
            delete_count: stats.delete_count,
        }
    }

    /// 모든 엣지 삭제 (메모리 정리)
    pub async fn clear(&self) {
        {
            let mut disk = self.disk.write().await;
            let _ = disk.clear().await;
        }

        *self.stats.write().await = InternalElementStats::default();
        self.next_edge_id.store(1, Ordering::SeqCst);
    }

    /// 메모리 압축
    pub async fn compact(&self) {
        // 디스크 모드에서는 별도 compaction 필요 없음
    }

    /// 다음 엣지 ID 생성 (내부용)
    pub fn generate_id(&self) -> EdgeId {
        self.next_edge_id.fetch_add(1, Ordering::SeqCst)
    }

    /// 다음 엣지 ID 설정 (복구 시 사용)
    pub fn set_next_id(&self, next_id: EdgeId) {
        self.next_edge_id.store(next_id, Ordering::SeqCst);
    }

    /// 현재 다음 엣지 ID 반환
    pub fn get_next_id(&self) -> EdgeId {
        self.next_edge_id.load(Ordering::SeqCst)
    }

    /// 최대 엣지 ID 찾기 (복구 시 사용)
    pub async fn find_max_edge_id(&self) -> Option<EdgeId> {
        self.disk
            .read()
            .await
            .scan_all()
            .await
            .ok()?
            .iter()
            .map(|e| e.id())
            .max()
    }

    /// PageWAL 설정
    pub async fn set_page_wal(&self, page_wal: Arc<PageWAL>) -> Result<()> {
        self.disk.write().await.set_page_wal(page_wal).await
    }

    /// BufferPoolManager 접근 (Page-level WAL 통합용)
    pub async fn get_buffer_pool(
        &self,
    ) -> Option<std::sync::Arc<crate::storage::cache::BufferPoolManager>> {
        Some(self.disk.read().await.buffer_pool())
    }

    /// Secondary Index BufferPoolManagers 접근 (WAL 복구용)
    pub async fn get_secondary_index_buffer_pools(
        &self,
    ) -> Vec<std::sync::Arc<crate::storage::cache::BufferPoolManager>> {
        self.disk.read().await.secondary_index_buffer_pools()
    }

    /// 항상 Disk 모드 (v1.9.1: Memory 백엔드 제거)
    pub fn is_disk_mode(&self) -> bool {
        true
    }

    /// last_page_id 재계산 (WAL 복구 후 호출)
    ///
    /// 디스크 파일 크기를 기반으로 last_page_id를 다시 계산합니다.
    pub async fn recalculate_last_page_id(&self) -> Result<()> {
        self.disk.read().await.recalculate_last_page_id().await
    }

    /// 스토리지 플러시
    pub async fn flush(&self) -> Result<()> {
        self.disk.read().await.flush().await
    }

    // ========== UNDO 지원 메서드 ==========

    /// UNDO 시 Primary Index 복구
    ///
    /// 트랜잭션 롤백 시 SlotDelete의 UNDO로 호출됩니다.
    /// B+Tree에 edge_id -> RecordId 매핑을 복원합니다.
    pub async fn restore_index(
        &self,
        edge_id: EdgeId,
        page_id: crate::storage::disk::page::PageId,
        slot_id: crate::storage::disk::slotted_page::SlotId,
    ) -> Result<()> {
        self.disk
            .write()
            .await
            .restore_index(edge_id, page_id, slot_id)
            .await
    }

    /// UNDO 시 Secondary Indexes 복구
    ///
    /// 트랜잭션 롤백 시 SlotDelete의 UNDO로 호출됩니다.
    /// outgoing, incoming, type 인덱스에 엣지 정보를 복원합니다.
    pub async fn restore_secondary_indexes(
        &self,
        edge_id: EdgeId,
        from_node: NodeId,
        to_node: NodeId,
        relation_type: &str,
    ) -> Result<()> {
        self.disk
            .write()
            .await
            .restore_secondary_indexes(edge_id, from_node, to_node, relation_type)
            .await
    }

    /// UNDO 시 Secondary Indexes에서 제거
    ///
    /// 트랜잭션 롤백 시 SlotInsert의 UNDO로 호출됩니다.
    /// outgoing, incoming, type 인덱스에서 엣지 정보를 제거합니다.
    pub async fn remove_from_secondary_indexes(
        &self,
        edge_id: EdgeId,
        from_node: NodeId,
        to_node: NodeId,
        relation_type: &str,
    ) -> Result<()> {
        self.disk
            .write()
            .await
            .remove_from_secondary_indexes(edge_id, from_node, to_node, relation_type)
            .await
    }

    /// UNDO 시 B+Tree 인덱스에서 제거 + 통계 갱신
    ///
    /// SlotInsert UNDO 시:
    /// 1. B+Tree 인덱스에서 엣지 엔트리 제거
    /// 2. 엣지 카운트 감소
    pub async fn undo_insert(&self, edge_id: EdgeId) -> Result<()> {
        self.disk.write().await.undo_insert(edge_id).await?;
        // 통계 갱신: read first, then write to avoid holding both locks
        let current_total = self.stats.read().await.total_elements;
        self.stats.write().await.total_elements = current_total.saturating_sub(1);
        Ok(())
    }
}
