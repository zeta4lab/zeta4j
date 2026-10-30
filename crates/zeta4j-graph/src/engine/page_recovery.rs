//! Page-level ARIES Recovery Manager
//!
//! ARIES 알고리즘 기반의 크래시 복구를 수행합니다.
//!
//! ## ARIES 3단계 복구
//!
//! 1. **Analysis Phase**: WAL을 스캔하여 트랜잭션 테이블과 Dirty Page 테이블 구축
//! 2. **REDO Phase**: Dirty 페이지에 WAL 변경 재실행 (Repeating History)
//! 3. **UNDO Phase**: 미커밋 트랜잭션 롤백 (CLR 기록)
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::engine::page_recovery::PageRecoveryManager;
//!
//! let mut recovery = PageRecoveryManager::new(page_wal, buffer_pool);
//! let stats = recovery.recover().await?;
//!
//! println!("Recovered: {} redo, {} undo", stats.redo_count, stats.undo_count);
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::error::{GraphError, Result};
use crate::storage::cache::BufferPoolManager;
use crate::storage::disk::page::PageId;
use crate::storage::disk::slotted_page::{SlotId, SlottedPage};
use crate::transaction::TransactionId;
use crate::transaction::lsn::{INVALID_LSN, LSN};
use crate::transaction::page_wal::PageWAL;
use crate::transaction::wal::{LogRecord, PageStorageType, SlotUndoOp};

/// 트랜잭션 상태
#[derive(Debug, Clone, PartialEq)]
pub enum TxStatus {
    /// 활성 (커밋 전)
    Active,
    /// 커밋됨
    Committed,
    /// 롤백됨
    Aborted,
}

/// 트랜잭션 테이블 엔트리
#[derive(Debug, Clone)]
pub struct TxTableEntry {
    /// 트랜잭션 상태
    pub status: TxStatus,
    /// 마지막 WAL 레코드의 LSN
    pub last_lsn: LSN,
    /// 다음 UNDO 대상 LSN
    pub undo_next_lsn: LSN,
}

impl TxTableEntry {
    pub fn new_active(lsn: LSN) -> Self {
        Self {
            status: TxStatus::Active,
            last_lsn: lsn,
            undo_next_lsn: lsn,
        }
    }
}

/// Dirty Page 테이블 엔트리
#[derive(Debug, Clone)]
pub struct DirtyPageEntry {
    /// 페이지가 dirty 상태가 된 첫 번째 LSN
    pub rec_lsn: LSN,
}

/// WAL 레코드와 LSN 쌍
#[derive(Debug)]
struct WalRecordWithLSN {
    lsn: LSN,
    record: LogRecord,
}

/// 복구 통계
#[derive(Debug, Default)]
pub struct RecoveryStats {
    /// 분석된 WAL 레코드 수
    pub records_analyzed: usize,
    /// REDO 수행 횟수
    pub redo_count: usize,
    /// UNDO 수행 횟수
    pub undo_count: usize,
    /// CLR 생성 횟수
    pub clr_count: usize,
    /// 복구된 활성 트랜잭션 수
    pub active_tx_count: usize,
}

/// Page-level ARIES Recovery Manager
///
/// ARIES 알고리즘 기반의 크래시 복구를 수행합니다.
pub struct PageRecoveryManager {
    /// Page-level WAL
    page_wal: Arc<PageWAL>,
    /// 버퍼 풀 관리자
    buffer_pool: Arc<BufferPoolManager>,
    /// 트랜잭션 테이블
    tx_table: HashMap<TransactionId, TxTableEntry>,
    /// Dirty Page 테이블
    dirty_page_table: HashMap<PageId, DirtyPageEntry>,
    /// 마지막으로 확인한 체크포인트 LSN
    last_checkpoint_lsn: Option<LSN>,
    /// Storage type 필터 (통합 WAL에서 해당 storage type만 복구)
    storage_type_filter: PageStorageType,
}

impl PageRecoveryManager {
    /// 새 Recovery Manager 생성
    ///
    /// BufferPoolManager의 storage_type을 자동으로 필터로 사용합니다.
    /// 통합 WAL에서 해당 storage type의 레코드만 복구합니다.
    pub fn new(page_wal: Arc<PageWAL>, buffer_pool: Arc<BufferPoolManager>) -> Self {
        // BufferPoolManager의 storage_type을 필터로 사용
        let storage_type_filter = buffer_pool.storage_type();

        Self {
            page_wal,
            buffer_pool,
            storage_type_filter,
            tx_table: HashMap::new(),
            dirty_page_table: HashMap::new(),
            last_checkpoint_lsn: None,
        }
    }

    /// ARIES 3단계 복구 수행
    ///
    /// 1. Analysis: 트랜잭션/Dirty Page 테이블 구축
    /// 2. REDO: 모든 페이지 변경 재실행
    /// 3. UNDO: 미커밋 트랜잭션 롤백
    pub async fn recover(&mut self) -> Result<RecoveryStats> {
        let mut stats = RecoveryStats::default();

        // WAL 레코드 읽기
        let records = self.read_all_records()?;
        stats.records_analyzed = records.len();

        if records.is_empty() {
            return Ok(stats);
        }

        info!(record_count = records.len(), "Starting ARIES recovery");

        // 1. Analysis Phase
        self.analysis_phase(&records)?;
        stats.active_tx_count = self
            .tx_table
            .values()
            .filter(|e| e.status == TxStatus::Active)
            .count();

        debug!(
            active_tx = stats.active_tx_count,
            dirty_pages = self.dirty_page_table.len(),
            "Analysis phase completed"
        );

        // 2. REDO Phase
        stats.redo_count = self.redo_phase(&records).await?;

        debug!(redo_count = stats.redo_count, "REDO phase completed");

        // 3. UNDO Phase
        let (undo_count, clr_count) = self.undo_phase(&records).await?;
        stats.undo_count = undo_count;
        stats.clr_count = clr_count;

        debug!(
            undo_count = stats.undo_count,
            clr_count = stats.clr_count,
            "UNDO phase completed"
        );

        // WAL flush
        self.page_wal.flush()?;

        // BufferPool의 dirty page들을 디스크에 기록
        if stats.redo_count > 0 || stats.undo_count > 0 {
            self.buffer_pool.flush_all_pages().await?;

            info!(
                redo_count = stats.redo_count,
                undo_count = stats.undo_count,
                clr_count = stats.clr_count,
                "Page-level WAL recovery completed"
            );
        }

        Ok(stats)
    }

    /// WAL에서 모든 레코드 읽기
    fn read_all_records(&self) -> Result<Vec<WalRecordWithLSN>> {
        let mut records = Vec::new();
        for (lsn, result) in (1_u64..).zip(self.page_wal.iter()?) {
            let record = result.map_err(|e| GraphError::Storage {
                message: format!("Failed to read WAL record: {}", e),
            })?;
            records.push(WalRecordWithLSN {
                lsn,
                record: record.record,
            });
        }

        Ok(records)
    }

    /// 레코드가 현재 storage_type_filter와 일치하는지 확인
    ///
    /// - PageWrite, CLR, Slot 레코드: storage_type 필드와 비교
    /// - Begin, Commit, Abort: 모든 storage type에서 처리 (트랜잭션 메타데이터)
    /// - Checkpoint: 모든 storage type에서 처리 (체크포인트 메타데이터)
    fn matches_storage_type(&self, record: &LogRecord) -> bool {
        match record {
            LogRecord::PageWrite { storage_type, .. }
            | LogRecord::CLR { storage_type, .. }
            | LogRecord::SlotInsert { storage_type, .. }
            | LogRecord::SlotDelete { storage_type, .. }
            | LogRecord::SlotUpdate { storage_type, .. }
            | LogRecord::SlotCLR { storage_type, .. } => *storage_type == self.storage_type_filter,
            // 트랜잭션/체크포인트 메타데이터는 모든 storage에서 처리
            LogRecord::Begin(_)
            | LogRecord::Commit(_)
            | LogRecord::Abort(_)
            | LogRecord::BeginCheckpoint
            | LogRecord::EndCheckpoint { .. } => true,
        }
    }

    // ==================== Analysis Phase ====================

    /// Analysis Phase: 트랜잭션/Dirty Page 테이블 구축
    ///
    /// WAL을 처음부터 끝까지 스캔하여:
    /// - 트랜잭션 테이블: 각 트랜잭션의 상태와 마지막 LSN
    /// - Dirty Page 테이블: 각 dirty 페이지의 rec_lsn
    fn analysis_phase(&mut self, records: &[WalRecordWithLSN]) -> Result<()> {
        // 체크포인트 찾기 (있으면 그 지점의 테이블 복원)
        self.find_and_restore_checkpoint(records);

        // 체크포인트 이후 레코드부터 분석
        let start_lsn = self.last_checkpoint_lsn.unwrap_or(INVALID_LSN);

        for entry in records {
            if entry.lsn <= start_lsn {
                continue;
            }

            match &entry.record {
                LogRecord::Begin(tx_id) => {
                    self.tx_table
                        .insert(*tx_id, TxTableEntry::new_active(entry.lsn));
                }

                LogRecord::Commit(tx_id) => {
                    if let Some(tx_entry) = self.tx_table.get_mut(tx_id) {
                        tx_entry.status = TxStatus::Committed;
                        tx_entry.last_lsn = entry.lsn;
                    }
                }

                LogRecord::Abort(tx_id) => {
                    if let Some(tx_entry) = self.tx_table.get_mut(tx_id) {
                        tx_entry.status = TxStatus::Aborted;
                        tx_entry.last_lsn = entry.lsn;
                    }
                }

                LogRecord::PageWrite {
                    tx_id,
                    storage_type,
                    page_id,
                    ..
                } => {
                    // 트랜잭션 테이블 업데이트 (모든 storage type)
                    if let Some(tx_entry) = self.tx_table.get_mut(tx_id) {
                        tx_entry.last_lsn = entry.lsn;
                        tx_entry.undo_next_lsn = entry.lsn;
                    } else {
                        // Begin 레코드 없이 PageWrite가 먼저 올 수도 있음 (체크포인트 후)
                        self.tx_table
                            .insert(*tx_id, TxTableEntry::new_active(entry.lsn));
                    }

                    // Dirty Page 테이블 업데이트 (해당 storage type만)
                    if *storage_type == self.storage_type_filter {
                        self.dirty_page_table
                            .entry(*page_id)
                            .or_insert(DirtyPageEntry { rec_lsn: entry.lsn });
                    }
                }

                LogRecord::CLR {
                    tx_id,
                    storage_type,
                    undo_next_lsn,
                    page_id,
                    ..
                } => {
                    // 트랜잭션 테이블 업데이트 (모든 storage type)
                    if let Some(tx_entry) = self.tx_table.get_mut(tx_id) {
                        tx_entry.last_lsn = entry.lsn;
                        tx_entry.undo_next_lsn = *undo_next_lsn;
                    }

                    // Dirty Page 테이블 업데이트 (해당 storage type만)
                    if *storage_type == self.storage_type_filter {
                        self.dirty_page_table
                            .entry(*page_id)
                            .or_insert(DirtyPageEntry { rec_lsn: entry.lsn });
                    }
                }

                // Slot-level WAL 레코드 처리
                LogRecord::SlotInsert {
                    tx_id,
                    storage_type,
                    page_id,
                    ..
                }
                | LogRecord::SlotDelete {
                    tx_id,
                    storage_type,
                    page_id,
                    ..
                }
                | LogRecord::SlotUpdate {
                    tx_id,
                    storage_type,
                    page_id,
                    ..
                } => {
                    // 트랜잭션 테이블 업데이트 (모든 storage type)
                    if let Some(tx_entry) = self.tx_table.get_mut(tx_id) {
                        tx_entry.last_lsn = entry.lsn;
                        tx_entry.undo_next_lsn = entry.lsn;
                    } else {
                        self.tx_table
                            .insert(*tx_id, TxTableEntry::new_active(entry.lsn));
                    }

                    // Dirty Page 테이블 업데이트 (해당 storage type만)
                    if *storage_type == self.storage_type_filter {
                        self.dirty_page_table
                            .entry(*page_id)
                            .or_insert(DirtyPageEntry { rec_lsn: entry.lsn });
                    }
                }

                LogRecord::SlotCLR {
                    tx_id,
                    storage_type,
                    undo_next_lsn,
                    page_id,
                    ..
                } => {
                    // 트랜잭션 테이블 업데이트 (모든 storage type)
                    if let Some(tx_entry) = self.tx_table.get_mut(tx_id) {
                        tx_entry.last_lsn = entry.lsn;
                        tx_entry.undo_next_lsn = *undo_next_lsn;
                    }

                    // Dirty Page 테이블 업데이트 (해당 storage type만)
                    if *storage_type == self.storage_type_filter {
                        self.dirty_page_table
                            .entry(*page_id)
                            .or_insert(DirtyPageEntry { rec_lsn: entry.lsn });
                    }
                }

                LogRecord::EndCheckpoint {
                    active_transactions,
                    dirty_page_table,
                } => {
                    // 체크포인트 정보로 테이블 초기화
                    for (tx_id, last_lsn) in active_transactions {
                        self.tx_table.entry(*tx_id).or_insert(TxTableEntry {
                            status: TxStatus::Active,
                            last_lsn: *last_lsn,
                            undo_next_lsn: *last_lsn,
                        });
                    }

                    for (_storage_type, page_id, rec_lsn) in dirty_page_table {
                        self.dirty_page_table
                            .entry(*page_id)
                            .or_insert(DirtyPageEntry { rec_lsn: *rec_lsn });
                    }

                    self.last_checkpoint_lsn = Some(entry.lsn);
                }

                _ => {
                    // BeginCheckpoint, Legacy Checkpoint 등 무시
                }
            }
        }

        Ok(())
    }

    /// 마지막 체크포인트 찾기 및 복원
    fn find_and_restore_checkpoint(&mut self, records: &[WalRecordWithLSN]) {
        // 마지막 EndCheckpoint 찾기
        for entry in records.iter().rev() {
            if let LogRecord::EndCheckpoint {
                active_transactions,
                dirty_page_table,
            } = &entry.record
            {
                // 체크포인트 정보로 테이블 초기화
                for (tx_id, last_lsn) in active_transactions {
                    self.tx_table.insert(
                        *tx_id,
                        TxTableEntry {
                            status: TxStatus::Active,
                            last_lsn: *last_lsn,
                            undo_next_lsn: *last_lsn,
                        },
                    );
                }

                for (_storage_type, page_id, rec_lsn) in dirty_page_table {
                    self.dirty_page_table
                        .insert(*page_id, DirtyPageEntry { rec_lsn: *rec_lsn });
                }

                self.last_checkpoint_lsn = Some(entry.lsn);
                break;
            }
        }
    }

    // ==================== REDO Phase ====================

    /// REDO Phase: 모든 페이지 변경 재실행
    ///
    /// Dirty Page 테이블의 최소 rec_lsn부터 모든 PageWrite/CLR 재실행.
    /// 조건: `page_lsn < record.lsn` 이면 REDO 수행
    async fn redo_phase(&mut self, records: &[WalRecordWithLSN]) -> Result<usize> {
        let mut redo_count = 0;

        // REDO 시작 LSN 계산 (Dirty Page 테이블의 최소 rec_lsn)
        let redo_start_lsn = self
            .dirty_page_table
            .values()
            .map(|e| e.rec_lsn)
            .min()
            .unwrap_or(INVALID_LSN);

        debug!(
            dirty_pages = self.dirty_page_table.len(),
            redo_start_lsn = redo_start_lsn,
            storage_type = ?self.storage_type_filter,
            "REDO phase starting"
        );

        for entry in records {
            if entry.lsn < redo_start_lsn {
                continue;
            }

            // Storage type 필터링 (해당 storage type만 REDO)
            if !self.matches_storage_type(&entry.record) {
                continue;
            }

            match &entry.record {
                LogRecord::PageWrite {
                    page_id,
                    offset,
                    after_image,
                    ..
                } => {
                    // Dirty Page 테이블에 있는지 확인
                    if let Some(dirty_entry) = self.dirty_page_table.get(page_id)
                        && entry.lsn >= dirty_entry.rec_lsn
                        && self
                            .redo_page_write(*page_id, *offset, after_image, entry.lsn)
                            .await?
                    {
                        redo_count += 1;
                    }
                }

                LogRecord::CLR {
                    page_id,
                    offset,
                    after_image,
                    ..
                } => {
                    // CLR도 REDO 대상
                    if let Some(dirty_entry) = self.dirty_page_table.get(page_id)
                        && entry.lsn >= dirty_entry.rec_lsn
                        && self
                            .redo_page_write(*page_id, *offset, after_image, entry.lsn)
                            .await?
                    {
                        redo_count += 1;
                    }
                }

                // Slot-level REDO
                LogRecord::SlotInsert {
                    page_id,
                    slot_id,
                    data,
                    ..
                } => {
                    if let Some(dirty_entry) = self.dirty_page_table.get(page_id)
                        && entry.lsn >= dirty_entry.rec_lsn
                        && self
                            .redo_slot_insert(*page_id, *slot_id, data, entry.lsn)
                            .await?
                    {
                        redo_count += 1;
                    }
                }

                LogRecord::SlotDelete {
                    page_id, slot_id, ..
                } => {
                    if let Some(dirty_entry) = self.dirty_page_table.get(page_id)
                        && entry.lsn >= dirty_entry.rec_lsn
                        && self.redo_slot_delete(*page_id, *slot_id, entry.lsn).await?
                    {
                        redo_count += 1;
                    }
                }

                LogRecord::SlotUpdate {
                    page_id,
                    slot_id,
                    after_data,
                    ..
                } => {
                    if let Some(dirty_entry) = self.dirty_page_table.get(page_id)
                        && entry.lsn >= dirty_entry.rec_lsn
                        && self
                            .redo_slot_update(*page_id, *slot_id, after_data, entry.lsn)
                            .await?
                    {
                        redo_count += 1;
                    }
                }

                LogRecord::SlotCLR {
                    page_id,
                    slot_id,
                    undo_op,
                    ..
                } => {
                    if let Some(dirty_entry) = self.dirty_page_table.get(page_id)
                        && entry.lsn >= dirty_entry.rec_lsn
                        && self
                            .redo_slot_clr(*page_id, *slot_id, undo_op, entry.lsn)
                            .await?
                    {
                        redo_count += 1;
                    }
                }

                _ => {}
            }
        }

        Ok(redo_count)
    }

    /// 단일 PageWrite REDO 수행
    ///
    /// 조건: `page_lsn < record.lsn` 이면 REDO 수행
    async fn redo_page_write(
        &mut self,
        page_id: PageId,
        offset: u16,
        after_image: &[u8],
        record_lsn: LSN,
    ) -> Result<bool> {
        let page_ref = self.buffer_pool.fetch_page(page_id).await?;
        let mut page = page_ref.lock().await;

        // REDO 조건 확인: page_lsn < record_lsn
        if page.needs_redo(record_lsn) {
            let start = offset as usize;
            let end = start + after_image.len();

            if end <= page.data.len() {
                page.data[start..end].copy_from_slice(after_image);
                page.mark_dirty_with_lsn(record_lsn);

                debug!(
                    page_id = page_id,
                    offset = offset,
                    lsn = record_lsn,
                    "REDO applied"
                );

                return Ok(true);
            } else {
                warn!(
                    page_id = page_id,
                    offset = offset,
                    length = after_image.len(),
                    "REDO skipped: offset out of bounds"
                );
            }
        }

        Ok(false)
    }

    // ==================== Slot-level REDO ====================

    /// SlotInsert REDO: 슬롯에 레코드 삽입
    async fn redo_slot_insert(
        &mut self,
        page_id: PageId,
        slot_id: SlotId,
        data: &[u8],
        record_lsn: LSN,
    ) -> Result<bool> {
        let page_ref = self.buffer_pool.fetch_page(page_id).await?;
        let mut page = page_ref.lock().await;

        if page.needs_redo(record_lsn) {
            let mut slotted = SlottedPage::new(&mut page);
            slotted.redo_slot_insert(slot_id, data)?;
            page.mark_dirty_with_lsn(record_lsn);

            debug!(
                page_id = page_id,
                slot_id = slot_id,
                lsn = record_lsn,
                "SlotInsert REDO applied"
            );

            return Ok(true);
        }

        Ok(false)
    }

    /// SlotDelete REDO: 슬롯 삭제
    async fn redo_slot_delete(
        &mut self,
        page_id: PageId,
        slot_id: SlotId,
        record_lsn: LSN,
    ) -> Result<bool> {
        let page_ref = self.buffer_pool.fetch_page(page_id).await?;
        let mut page = page_ref.lock().await;

        if page.needs_redo(record_lsn) {
            let mut slotted = SlottedPage::new(&mut page);
            slotted.redo_slot_delete(slot_id)?;
            page.mark_dirty_with_lsn(record_lsn);

            debug!(
                page_id = page_id,
                slot_id = slot_id,
                lsn = record_lsn,
                "SlotDelete REDO applied"
            );

            return Ok(true);
        }

        Ok(false)
    }

    /// SlotUpdate REDO: 슬롯 데이터 업데이트
    async fn redo_slot_update(
        &mut self,
        page_id: PageId,
        slot_id: SlotId,
        after_data: &[u8],
        record_lsn: LSN,
    ) -> Result<bool> {
        let page_ref = self.buffer_pool.fetch_page(page_id).await?;
        let mut page = page_ref.lock().await;

        if page.needs_redo(record_lsn) {
            let mut slotted = SlottedPage::new(&mut page);
            slotted.redo_slot_update(slot_id, after_data)?;
            page.mark_dirty_with_lsn(record_lsn);

            debug!(
                page_id = page_id,
                slot_id = slot_id,
                lsn = record_lsn,
                "SlotUpdate REDO applied"
            );

            return Ok(true);
        }

        Ok(false)
    }

    /// SlotCLR REDO: UNDO 연산의 REDO (물리적 복구)
    async fn redo_slot_clr(
        &mut self,
        page_id: PageId,
        slot_id: SlotId,
        undo_op: &SlotUndoOp,
        record_lsn: LSN,
    ) -> Result<bool> {
        let page_ref = self.buffer_pool.fetch_page(page_id).await?;
        let mut page = page_ref.lock().await;

        if page.needs_redo(record_lsn) {
            let mut slotted = SlottedPage::new(&mut page);

            match undo_op {
                SlotUndoOp::DeleteSlot => {
                    // Insert를 UNDO한 결과: 슬롯 삭제
                    slotted.redo_slot_delete(slot_id)?;
                }
                SlotUndoOp::RestoreSlot { data } => {
                    // Delete를 UNDO한 결과: 슬롯 복원
                    slotted.redo_slot_insert(slot_id, data)?;
                }
                SlotUndoOp::RestoreData { data } => {
                    // Update를 UNDO한 결과: 이전 데이터 복원
                    slotted.redo_slot_update(slot_id, data)?;
                }
            }

            page.mark_dirty_with_lsn(record_lsn);

            debug!(
                page_id = page_id,
                slot_id = slot_id,
                lsn = record_lsn,
                "SlotCLR REDO applied"
            );

            return Ok(true);
        }

        Ok(false)
    }

    // ==================== UNDO Phase ====================

    /// UNDO Phase: 미커밋 트랜잭션 롤백
    ///
    /// 활성 트랜잭션의 PageWrite/Slot 레코드를 역순으로 UNDO하고 CLR 기록.
    async fn undo_phase(&mut self, records: &[WalRecordWithLSN]) -> Result<(usize, usize)> {
        let mut undo_count = 0;
        let mut clr_count = 0;

        // 활성 트랜잭션 목록
        let active_txs: Vec<(TransactionId, TxTableEntry)> = self
            .tx_table
            .iter()
            .filter(|(_, entry)| entry.status == TxStatus::Active)
            .map(|(tx_id, entry)| (*tx_id, entry.clone()))
            .collect();

        if active_txs.is_empty() {
            return Ok((0, 0));
        }

        // 트랜잭션별 레코드 인덱스 구축
        let mut tx_records: HashMap<TransactionId, Vec<&WalRecordWithLSN>> = HashMap::new();
        for entry in records {
            let tx_id = entry.record.transaction_id();
            if active_txs.iter().any(|(id, _)| *id == tx_id) {
                tx_records.entry(tx_id).or_default().push(entry);
            }
        }

        // 각 활성 트랜잭션 UNDO
        for (tx_id, tx_entry) in active_txs {
            let tx_undo_count = self
                .undo_transaction(
                    tx_id,
                    tx_entry.undo_next_lsn,
                    tx_records.get(&tx_id).map(|v| v.as_slice()).unwrap_or(&[]),
                )
                .await?;

            undo_count += tx_undo_count;
            clr_count += tx_undo_count; // 각 UNDO마다 CLR 생성

            // Abort 로그 기록
            self.page_wal.log_abort(tx_id)?;
        }

        Ok((undo_count, clr_count))
    }

    /// 단일 트랜잭션 UNDO
    ///
    /// Slot-level 레코드는 실제 UNDO 수행, PageWrite는 MVCC로 처리
    async fn undo_transaction(
        &mut self,
        tx_id: TransactionId,
        undo_next_lsn: LSN,
        records: &[&WalRecordWithLSN],
    ) -> Result<usize> {
        let mut undo_count = 0;
        let mut current_undo_lsn = undo_next_lsn;

        // LSN 역순으로 UNDO 대상 찾기 (Slot 레코드 포함, storage_type 필터링)
        let mut undo_targets: Vec<&WalRecordWithLSN> = records
            .iter()
            .filter(|entry| {
                entry.lsn <= current_undo_lsn
                    && self.matches_storage_type(&entry.record)
                    && (entry.record.is_slot_modification()
                        || matches!(entry.record, LogRecord::PageWrite { .. }))
            })
            .copied()
            .collect();

        undo_targets.sort_by_key(|record| std::cmp::Reverse(record.lsn)); // LSN 역순

        for entry in undo_targets {
            if entry.lsn > current_undo_lsn {
                continue;
            }

            let prev_lsn = self.find_prev_lsn_any(tx_id, entry.lsn, records);

            match &entry.record {
                // Slot-level UNDO: 실제 UNDO 수행
                LogRecord::SlotInsert {
                    storage_type,
                    page_id,
                    slot_id,
                    ..
                } => {
                    // Insert를 UNDO: 슬롯 삭제
                    self.undo_slot_insert(*page_id, *slot_id).await?;

                    // SlotCLR 기록
                    self.page_wal.log_slot_clr(
                        tx_id,
                        *storage_type,
                        prev_lsn,
                        *page_id,
                        *slot_id,
                        SlotUndoOp::DeleteSlot,
                    )?;

                    debug!(
                        tx_id = tx_id,
                        page_id = page_id,
                        slot_id = slot_id,
                        "SlotInsert UNDO: slot deleted"
                    );

                    current_undo_lsn = prev_lsn;
                    undo_count += 1;
                }

                LogRecord::SlotDelete {
                    storage_type,
                    page_id,
                    slot_id,
                    data,
                    ..
                } => {
                    // Delete를 UNDO: 슬롯 복원
                    self.undo_slot_delete(*page_id, data).await?;

                    // SlotCLR 기록
                    self.page_wal.log_slot_clr(
                        tx_id,
                        *storage_type,
                        prev_lsn,
                        *page_id,
                        *slot_id,
                        SlotUndoOp::RestoreSlot { data: data.clone() },
                    )?;

                    debug!(
                        tx_id = tx_id,
                        page_id = page_id,
                        slot_id = slot_id,
                        "SlotDelete UNDO: slot restored"
                    );

                    current_undo_lsn = prev_lsn;
                    undo_count += 1;
                }

                LogRecord::SlotUpdate {
                    storage_type,
                    page_id,
                    slot_id,
                    before_data,
                    ..
                } => {
                    // Update를 UNDO: 이전 데이터로 복원
                    self.undo_slot_update(*page_id, *slot_id, before_data)
                        .await?;

                    // SlotCLR 기록
                    self.page_wal.log_slot_clr(
                        tx_id,
                        *storage_type,
                        prev_lsn,
                        *page_id,
                        *slot_id,
                        SlotUndoOp::RestoreData {
                            data: before_data.clone(),
                        },
                    )?;

                    debug!(
                        tx_id = tx_id,
                        page_id = page_id,
                        slot_id = slot_id,
                        "SlotUpdate UNDO: data restored"
                    );

                    current_undo_lsn = prev_lsn;
                    undo_count += 1;
                }

                // PageWrite: MVCC로 처리 (skip physical UNDO)
                LogRecord::PageWrite {
                    storage_type,
                    page_id,
                    offset,
                    before_image,
                    ..
                } => {
                    debug!(
                        tx_id = tx_id,
                        page_id = page_id,
                        offset = offset,
                        "Skipping physical UNDO (using MVCC visibility instead)"
                    );

                    // CLR만 기록 (물리적 UNDO는 수행하지 않음)
                    self.page_wal.log_clr(
                        tx_id,
                        *storage_type,
                        prev_lsn,
                        *page_id,
                        *offset,
                        before_image,
                    )?;

                    current_undo_lsn = prev_lsn;
                    undo_count += 1;
                }

                _ => {}
            }
        }

        Ok(undo_count)
    }

    /// 단일 PageWrite UNDO 수행
    #[expect(dead_code)]
    async fn undo_page_write(
        &mut self,
        page_id: PageId,
        offset: u16,
        before_image: &[u8],
    ) -> Result<()> {
        let page_ref = self.buffer_pool.fetch_page(page_id).await?;
        let mut page = page_ref.lock().await;

        let start = offset as usize;
        let end = start + before_image.len();

        if end <= page.data.len() {
            page.data[start..end].copy_from_slice(before_image);
            page.is_dirty = true;
        }

        Ok(())
    }

    /// 이전 LSN 찾기 (UNDO 체인용 - PageWrite only)
    #[expect(dead_code)]
    fn find_prev_lsn(
        &self,
        tx_id: TransactionId,
        current_lsn: LSN,
        records: &[&WalRecordWithLSN],
    ) -> LSN {
        records
            .iter()
            .filter(|entry| {
                entry.lsn < current_lsn
                    && entry.record.transaction_id() == tx_id
                    && matches!(entry.record, LogRecord::PageWrite { .. })
            })
            .map(|entry| entry.lsn)
            .max()
            .unwrap_or(INVALID_LSN)
    }

    /// 이전 LSN 찾기 (PageWrite + Slot 레코드 모두)
    fn find_prev_lsn_any(
        &self,
        tx_id: TransactionId,
        current_lsn: LSN,
        records: &[&WalRecordWithLSN],
    ) -> LSN {
        records
            .iter()
            .filter(|entry| {
                entry.lsn < current_lsn
                    && entry.record.transaction_id() == tx_id
                    && (entry.record.is_slot_modification()
                        || matches!(entry.record, LogRecord::PageWrite { .. }))
            })
            .map(|entry| entry.lsn)
            .max()
            .unwrap_or(INVALID_LSN)
    }

    // ==================== Slot-level UNDO ====================

    /// SlotInsert UNDO: 슬롯 삭제
    async fn undo_slot_insert(&mut self, page_id: PageId, slot_id: SlotId) -> Result<()> {
        let page_ref = self.buffer_pool.fetch_page(page_id).await?;
        let mut page = page_ref.lock().await;

        let mut slotted = SlottedPage::new(&mut page);
        slotted.undo_slot_insert(slot_id)?;
        page.is_dirty = true;

        Ok(())
    }

    /// SlotDelete UNDO: 슬롯 복원
    async fn undo_slot_delete(&mut self, page_id: PageId, data: &[u8]) -> Result<()> {
        let page_ref = self.buffer_pool.fetch_page(page_id).await?;
        let mut page = page_ref.lock().await;

        let mut slotted = SlottedPage::new(&mut page);
        let _slot_id = slotted.undo_slot_delete(data)?;
        page.is_dirty = true;

        Ok(())
    }

    /// SlotUpdate UNDO: 이전 데이터로 복원
    async fn undo_slot_update(
        &mut self,
        page_id: PageId,
        slot_id: SlotId,
        before_data: &[u8],
    ) -> Result<()> {
        let page_ref = self.buffer_pool.fetch_page(page_id).await?;
        let mut page = page_ref.lock().await;

        let mut slotted = SlottedPage::new(&mut page);
        slotted.undo_slot_update(slot_id, before_data)?;
        page.is_dirty = true;

        Ok(())
    }

    // ==================== Public Accessors ====================

    /// 트랜잭션 테이블 조회
    pub fn tx_table(&self) -> &HashMap<TransactionId, TxTableEntry> {
        &self.tx_table
    }

    /// Dirty Page 테이블 조회
    pub fn dirty_page_table(&self) -> &HashMap<PageId, DirtyPageEntry> {
        &self.dirty_page_table
    }

    /// 마지막 체크포인트 LSN 조회
    pub fn last_checkpoint_lsn(&self) -> Option<LSN> {
        self.last_checkpoint_lsn
    }
}
