// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

use super::page::{PAGE_SIZE, Page, PageId, read_u16_le, read_u32_le};
use crate::error::{GraphError, Result};
use crate::transaction::lsn::LSN;
use crate::transaction::{PageStorageType, PageWAL, TransactionId};

// 타입 정의
pub type SlotId = u16;

// 상수 정의 (헤더 오프셋)
const OFFSET_PAGE_ID: usize = 0;
const OFFSET_LSN: usize = 4;
const OFFSET_SLOT_COUNT: usize = 12;
const OFFSET_FREE_SPACE_PTR: usize = 14;
const HEADER_SIZE: usize = 16;
const SLOT_SIZE: usize = 4; // Offset(2) + Length(2)

/// Slotted Page 래퍼 (Page 데이터를 조작하는 도구)
pub struct SlottedPage<'a> {
    page: &'a mut Page,
}

impl<'a> SlottedPage<'a> {
    pub fn new(page: &'a mut Page) -> Self {
        Self { page }
    }

    /// 페이지 초기화 (헤더 설정)
    pub fn init(&mut self, page_id: PageId) {
        self.set_page_id(page_id);
        self.set_lsn(0);
        self.set_slot_count(0);
        self.set_free_space_ptr(PAGE_SIZE as u16); // 맨 끝에서 시작
    }

    // --- Header Accessors ---

    pub fn get_page_id(&self) -> PageId {
        let bytes = &self.page.data[OFFSET_PAGE_ID..OFFSET_PAGE_ID + 4];
        read_u32_le(bytes)
    }

    fn set_page_id(&mut self, page_id: PageId) {
        let bytes = page_id.to_le_bytes();
        self.page.data[OFFSET_PAGE_ID..OFFSET_PAGE_ID + 4].copy_from_slice(&bytes);
    }

    pub fn get_slot_count(&self) -> u16 {
        let bytes = &self.page.data[OFFSET_SLOT_COUNT..OFFSET_SLOT_COUNT + 2];
        read_u16_le(bytes)
    }

    fn set_slot_count(&mut self, count: u16) {
        let bytes = count.to_le_bytes();
        self.page.data[OFFSET_SLOT_COUNT..OFFSET_SLOT_COUNT + 2].copy_from_slice(&bytes);
    }

    pub fn get_free_space_ptr(&self) -> u16 {
        let bytes = &self.page.data[OFFSET_FREE_SPACE_PTR..OFFSET_FREE_SPACE_PTR + 2];
        read_u16_le(bytes)
    }

    fn set_free_space_ptr(&mut self, ptr: u16) {
        let bytes = ptr.to_le_bytes();
        self.page.data[OFFSET_FREE_SPACE_PTR..OFFSET_FREE_SPACE_PTR + 2].copy_from_slice(&bytes);
    }

    fn set_lsn(&mut self, lsn: u64) {
        let bytes = lsn.to_le_bytes();
        self.page.data[OFFSET_LSN..OFFSET_LSN + 8].copy_from_slice(&bytes);
    }

    // --- Slot Operations ---

    /// 레코드 삽입
    pub fn insert_record(&mut self, data: &[u8]) -> Result<SlotId> {
        let len = data.len() as u16;
        let slot_count = self.get_slot_count();
        let free_ptr = self.get_free_space_ptr();

        // 필요 공간 계산: 데이터 크기 + 새 슬롯 엔트리 크기
        let required_space = len as usize + SLOT_SIZE;
        let used_header_space = HEADER_SIZE + (slot_count as usize * SLOT_SIZE);

        // 오버플로 방지: free_ptr가 used_header_space보다 작으면 공간 없음
        if (free_ptr as usize) <= used_header_space {
            return Err(GraphError::Storage {
                message: "Page full (no free space)".to_string(),
            });
        }
        let available_space = (free_ptr as usize) - used_header_space;

        if available_space < required_space {
            return Err(GraphError::Storage {
                message: "Page full".to_string(),
            });
        }

        // 오버플로 방지: free_ptr가 len보다 작으면 에러
        if free_ptr < len {
            return Err(GraphError::Storage {
                message: "Page full (cannot fit record)".to_string(),
            });
        }

        // 1. 데이터 쓰기 (뒤에서 앞으로)
        let new_free_ptr = free_ptr - len;
        self.page.data[new_free_ptr as usize..(new_free_ptr + len) as usize].copy_from_slice(data);
        self.set_free_space_ptr(new_free_ptr);

        // 2. 슬롯 추가 (앞에서 뒤로)
        self.set_slot(slot_count, new_free_ptr, len);
        self.set_slot_count(slot_count + 1);

        Ok(slot_count)
    }

    /// 레코드 읽기
    pub fn get_record(&self, slot_id: SlotId) -> Option<&[u8]> {
        if slot_id >= self.get_slot_count() {
            return None;
        }

        let (offset, len) = self.get_slot(slot_id);
        if len == 0 {
            return None; // Deleted record (if we use len=0 for deleted)
        }

        let start = offset as usize;
        let end = start + len as usize;

        // 범위 검사: 페이지 범위를 벗어나면 None 반환
        if end > PAGE_SIZE || start >= PAGE_SIZE {
            return None;
        }

        Some(&self.page.data[start..end])
    }

    /// 슬롯 정보 읽기
    fn get_slot(&self, slot_id: SlotId) -> (u16, u16) {
        let offset = HEADER_SIZE + (slot_id as usize * SLOT_SIZE);
        let slot_bytes = &self.page.data[offset..offset + SLOT_SIZE];

        let record_offset = read_u16_le(&slot_bytes[0..2]);
        let record_len = read_u16_le(&slot_bytes[2..4]);

        (record_offset, record_len)
    }

    /// 슬롯 정보 쓰기
    fn set_slot(&mut self, slot_id: SlotId, offset: u16, len: u16) {
        let header_offset = HEADER_SIZE + (slot_id as usize * SLOT_SIZE);

        self.page.data[header_offset..header_offset + 2].copy_from_slice(&offset.to_le_bytes());
        self.page.data[header_offset + 2..header_offset + 4].copy_from_slice(&len.to_le_bytes());
    }

    /// 레코드 삭제 (논리적 삭제 - len을 0으로 설정)
    ///
    /// 실제 데이터는 페이지에 남아있지만, 슬롯의 length를 0으로 설정하여
    /// 삭제된 것으로 표시합니다. 공간을 회수하려면 `compact()`를 호출하세요.
    pub fn delete_record(&mut self, slot_id: SlotId) -> Result<()> {
        if slot_id >= self.get_slot_count() {
            return Err(GraphError::Storage {
                message: format!("Invalid slot id: {}", slot_id),
            });
        }

        let (offset, len) = self.get_slot(slot_id);
        if len == 0 {
            // 이미 삭제됨 - idempotent 동작
            return Ok(());
        }

        // 슬롯의 length를 0으로 설정하여 삭제 표시
        // offset은 유지 (compact 시 필요할 수 있음)
        self.set_slot(slot_id, offset, 0);
        self.page.is_dirty = true;

        Ok(())
    }

    /// 사용 가능한 여유 공간 반환 (바이트)
    ///
    /// 삭제된 레코드 공간은 포함되지 않습니다.
    /// 삭제된 공간까지 포함하려면 `compact()` 후 호출하세요.
    pub fn free_space(&self) -> usize {
        let slot_count = self.get_slot_count() as usize;
        let free_ptr = self.get_free_space_ptr() as usize;
        let used_header_space = HEADER_SIZE + (slot_count * SLOT_SIZE);

        free_ptr.saturating_sub(used_header_space)
    }

    /// 단편화율 계산 (0.0 ~ 1.0)
    ///
    /// 삭제된 레코드가 차지하는 공간의 비율을 반환합니다.
    /// 0.0 = 단편화 없음, 1.0 = 모든 레코드가 삭제됨
    ///
    /// 계산 방법: 삭제된_레코드_크기 / (삭제된_레코드_크기 + 유효한_레코드_크기)
    pub fn fragmentation_ratio(&self) -> f64 {
        let slot_count = self.get_slot_count();
        if slot_count == 0 {
            return 0.0;
        }

        let mut valid_bytes: usize = 0;

        for slot_id in 0..slot_count {
            let (_offset, len) = self.get_slot(slot_id);
            if len != 0 {
                valid_bytes += len as usize;
            }
        }

        // 더 정확한 단편화 계산: 데이터 영역의 "구멍" 크기
        // 데이터 영역 = free_space_ptr ~ PAGE_SIZE
        let free_ptr = self.get_free_space_ptr() as usize;
        let data_region_size = PAGE_SIZE - free_ptr;

        if data_region_size == 0 {
            return 0.0;
        }

        // 실제 유효 데이터가 차지하는 비율
        let valid_data_ratio = valid_bytes as f64 / data_region_size as f64;

        // 단편화율 = 1 - 유효 데이터 비율
        (1.0 - valid_data_ratio).clamp(0.0, 1.0)
    }

    /// 압축이 필요한지 여부 확인
    ///
    /// 단편화율이 threshold를 초과하면 true 반환
    /// 일반적으로 threshold는 0.3 ~ 0.5 사이 권장
    pub fn needs_compaction(&self, threshold: f64) -> bool {
        self.fragmentation_ratio() > threshold
    }

    /// 회수 가능한 공간 계산 (바이트)
    ///
    /// compact() 호출 시 회수할 수 있는 예상 공간을 반환합니다.
    pub fn reclaimable_space(&self) -> usize {
        let slot_count = self.get_slot_count();
        if slot_count == 0 {
            return 0;
        }

        // 유효한 레코드의 총 크기 계산
        let mut valid_bytes: usize = 0;
        for slot_id in 0..slot_count {
            let (_, len) = self.get_slot(slot_id);
            if len > 0 {
                valid_bytes += len as usize;
            }
        }

        // 현재 데이터 영역 크기
        let free_ptr = self.get_free_space_ptr() as usize;
        let data_region_size = PAGE_SIZE - free_ptr;

        // 회수 가능한 공간 = 현재 데이터 영역 - 유효 데이터
        data_region_size.saturating_sub(valid_bytes)
    }

    /// 삭제된 레코드 공간을 회수하여 페이지를 압축
    ///
    /// 모든 유효한 레코드를 페이지 끝으로 재배치하고,
    /// 삭제된 레코드가 차지하던 공간을 회수합니다.
    ///
    /// **주의**: 이 메서드는 슬롯 ID를 유지합니다 (슬롯 테이블은 변경하지 않음).
    /// 삭제된 슬롯을 완전히 제거하려면 B+Tree의 compact_entries()를 사용하세요.
    ///
    /// # Returns
    ///
    /// 회수된 공간의 크기 (바이트)
    pub fn compact(&mut self) -> usize {
        let slot_count = self.get_slot_count();
        if slot_count == 0 {
            return 0;
        }

        // 압축 전 free_space_ptr 저장
        let old_free_ptr = self.get_free_space_ptr() as usize;

        // 1. 유효한 레코드들을 임시 버퍼에 수집
        let mut records: Vec<(SlotId, Vec<u8>)> = Vec::new();
        for slot_id in 0..slot_count {
            let (offset, len) = self.get_slot(slot_id);
            if len > 0 {
                let start = offset as usize;
                let end = start + len as usize;
                // 범위 검사: 유효하지 않은 범위는 스킵
                if end > PAGE_SIZE || start >= PAGE_SIZE {
                    continue;
                }
                let data = self.page.data[start..end].to_vec();
                records.push((slot_id, data));
            }
        }

        // 2. free_space_ptr을 페이지 끝으로 리셋
        let mut new_free_ptr = PAGE_SIZE as u16;

        // 3. 레코드들을 뒤에서부터 다시 배치 (슬롯 ID 유지)
        for (slot_id, data) in records {
            let len = data.len() as u16;
            new_free_ptr -= len;

            // 데이터 복사
            self.page.data[new_free_ptr as usize..(new_free_ptr + len) as usize]
                .copy_from_slice(&data);

            // 슬롯 업데이트
            self.set_slot(slot_id, new_free_ptr, len);
        }

        // 4. free_space_ptr 업데이트
        self.set_free_space_ptr(new_free_ptr);
        self.page.is_dirty = true;

        // 회수된 공간 = 새 free_ptr - 이전 free_ptr
        (new_free_ptr as usize).saturating_sub(old_free_ptr)
    }

    /// 자동 압축 수행 (단편화율 기반)
    ///
    /// 단편화율이 threshold를 초과하면 자동으로 compact()를 실행합니다.
    ///
    /// # Arguments
    ///
    /// * `threshold` - 압축 트리거 단편화율 (0.0 ~ 1.0, 기본 권장값: 0.4)
    ///
    /// # Returns
    ///
    /// 회수된 공간의 크기 (압축하지 않았으면 0)
    pub fn auto_compact(&mut self, threshold: f64) -> usize {
        if self.needs_compaction(threshold) {
            self.compact()
        } else {
            0
        }
    }

    /// 레코드 업데이트 (in-place 또는 재배치)
    ///
    /// 새 데이터가 기존 공간에 맞으면 in-place 업데이트,
    /// 그렇지 않으면 기존 레코드를 삭제하고 새로 삽입합니다.
    pub fn update_record(&mut self, slot_id: SlotId, data: &[u8]) -> Result<()> {
        if slot_id >= self.get_slot_count() {
            return Err(GraphError::Storage {
                message: format!("Invalid slot id: {}", slot_id),
            });
        }

        let (offset, old_len) = self.get_slot(slot_id);
        if old_len == 0 {
            return Err(GraphError::Storage {
                message: format!("Slot {} is deleted", slot_id),
            });
        }

        let new_len = data.len() as u16;

        if new_len <= old_len {
            // In-place 업데이트 가능 (새 데이터가 더 작거나 같음)
            self.page.data[offset as usize..(offset + new_len) as usize].copy_from_slice(data);
            // 길이가 줄어든 경우 슬롯 업데이트
            if new_len < old_len {
                self.set_slot(slot_id, offset, new_len);
            }
        } else {
            // 더 큰 데이터: 새 위치에 삽입 필요
            // 기존 슬롯을 삭제로 마킹하면 compact에서 공간 회수 가능

            // 헬퍼: 새 레코드를 위한 공간이 있는지 확인
            let can_allocate = |this: &Self, len: u16| -> bool {
                let free_ptr = this.get_free_space_ptr();
                let used_header_space = HEADER_SIZE + (this.get_slot_count() as usize * SLOT_SIZE);
                if free_ptr < len {
                    return false;
                }
                let new_offset = (free_ptr - len) as usize;
                new_offset >= used_header_space
            };

            // 기존 슬롯을 삭제로 마킹 (compact에서 공간 회수 가능)
            self.set_slot(slot_id, offset, 0);

            // 첫 번째 시도
            if !can_allocate(self, new_len) {
                // compact 후 재시도
                self.compact();
                if !can_allocate(self, new_len) {
                    return Err(GraphError::Storage {
                        message: "Not enough space for update".to_string(),
                    });
                }
            }

            // 이제 안전하게 새 레코드 쓰기
            let free_ptr = self.get_free_space_ptr();
            let new_offset = free_ptr - new_len;
            self.page.data[new_offset as usize..(new_offset + new_len) as usize]
                .copy_from_slice(data);
            self.set_free_space_ptr(new_offset);
            self.set_slot(slot_id, new_offset, new_len);
        }

        self.page.is_dirty = true;
        Ok(())
    }

    /// 삭제된 슬롯 수 반환
    pub fn deleted_count(&self) -> u16 {
        let slot_count = self.get_slot_count();
        let mut deleted = 0;
        for slot_id in 0..slot_count {
            let (_, len) = self.get_slot(slot_id);
            if len == 0 {
                deleted += 1;
            }
        }
        deleted
    }

    /// 유효한 레코드 수 반환
    pub fn valid_record_count(&self) -> u16 {
        self.get_slot_count() - self.deleted_count()
    }

    /// 모든 유효한 레코드 순회
    pub fn iter_records(&self) -> impl Iterator<Item = (SlotId, &[u8])> {
        let slot_count = self.get_slot_count();
        (0..slot_count)
            .filter_map(move |slot_id| self.get_record(slot_id).map(|data| (slot_id, data)))
    }

    // ==================== WAL Integration Methods ====================

    /// 레코드 삽입 + WAL 로깅
    ///
    /// 레코드를 삽입하고 SlotInsert 레코드를 WAL에 기록합니다.
    /// 반환된 LSN은 페이지의 page_lsn 업데이트에 사용됩니다.
    ///
    /// # Arguments
    ///
    /// * `data` - 삽입할 레코드 데이터
    /// * `page_wal` - PageWAL 참조
    /// * `tx_id` - 트랜잭션 ID
    /// * `storage_type` - 스토리지 유형
    ///
    /// # Returns
    ///
    /// (SlotId, LSN) 튜플
    pub fn insert_record_with_wal(
        &mut self,
        data: &[u8],
        page_wal: &PageWAL,
        tx_id: TransactionId,
        storage_type: PageStorageType,
    ) -> Result<(SlotId, LSN)> {
        let page_id = self.get_page_id();

        // 1. 레코드 삽입 수행
        let slot_id = self.insert_record(data)?;

        // 2. Slot-level WAL 로깅 (단일 레코드)
        let lsn = page_wal
            .log_slot_insert(tx_id, storage_type, page_id, slot_id, data)
            .map_err(|e| GraphError::Storage {
                message: format!("WAL logging failed: {}", e),
            })?;

        // 3. 페이지 dirty 마킹 + LSN 업데이트
        self.page.mark_dirty_with_lsn(lsn);

        Ok((slot_id, lsn))
    }

    /// 레코드 삭제 + WAL 로깅
    ///
    /// 레코드를 논리적으로 삭제하고 SlotDelete 레코드를 WAL에 기록합니다.
    pub fn delete_record_with_wal(
        &mut self,
        slot_id: SlotId,
        page_wal: &PageWAL,
        tx_id: TransactionId,
        storage_type: PageStorageType,
    ) -> Result<LSN> {
        if slot_id >= self.get_slot_count() {
            return Err(GraphError::Storage {
                message: format!("Invalid slot id: {}", slot_id),
            });
        }

        let (_, len) = self.get_slot(slot_id);
        if len == 0 {
            // 이미 삭제됨 - idempotent 동작 (WAL 로깅 필요 없음)
            return Ok(0);
        }

        let page_id = self.get_page_id();

        // 1. 삭제 전 데이터 복사 (UNDO용)
        let data = self
            .get_record(slot_id)
            .ok_or_else(|| GraphError::Storage {
                message: format!("Slot {} has no data", slot_id),
            })?
            .to_vec();

        // 2. 레코드 삭제 수행
        self.delete_record(slot_id)?;

        // 3. Slot-level WAL 로깅
        let lsn = page_wal
            .log_slot_delete(tx_id, storage_type, page_id, slot_id, &data)
            .map_err(|e| GraphError::Storage {
                message: format!("WAL logging failed: {}", e),
            })?;

        // 4. 페이지 dirty 마킹 + LSN 업데이트
        self.page.mark_dirty_with_lsn(lsn);

        Ok(lsn)
    }

    /// 레코드 업데이트 + WAL 로깅 (in-place만 지원)
    ///
    /// 새 데이터가 기존 공간에 맞는 경우에만 in-place 업데이트를 수행합니다.
    /// 공간이 부족하면 에러를 반환합니다.
    pub fn update_record_with_wal(
        &mut self,
        slot_id: SlotId,
        data: &[u8],
        page_wal: &PageWAL,
        tx_id: TransactionId,
        storage_type: PageStorageType,
    ) -> Result<LSN> {
        if slot_id >= self.get_slot_count() {
            return Err(GraphError::Storage {
                message: format!("Invalid slot id: {}", slot_id),
            });
        }

        let (_, old_len) = self.get_slot(slot_id);
        if old_len == 0 {
            return Err(GraphError::Storage {
                message: format!("Slot {} is deleted", slot_id),
            });
        }

        let new_len = data.len() as u16;
        if new_len > old_len {
            // 재배치가 필요한 경우는 복잡하므로, 현재는 에러 반환
            // 실제로는 delete + insert를 별도로 수행해야 함
            return Err(GraphError::Storage {
                message: "Update requires more space than available. Use delete + insert."
                    .to_string(),
            });
        }

        let page_id = self.get_page_id();

        // 1. before data 캡처 (UNDO용)
        let before_data = self
            .get_record(slot_id)
            .ok_or_else(|| GraphError::Storage {
                message: format!("Slot {} has no data", slot_id),
            })?
            .to_vec();

        // 2. 레코드 업데이트 수행
        self.update_record(slot_id, data)?;

        // 3. Slot-level WAL 로깅
        let lsn = page_wal
            .log_slot_update(tx_id, storage_type, page_id, slot_id, &before_data, data)
            .map_err(|e| GraphError::Storage {
                message: format!("WAL logging failed: {}", e),
            })?;

        // 4. 페이지 dirty 마킹 + LSN 업데이트
        self.page.mark_dirty_with_lsn(lsn);

        Ok(lsn)
    }

    // ==================== Slot-level REDO/UNDO Methods ====================

    /// 페이지가 초기화되었는지 확인
    pub fn is_initialized(&self) -> bool {
        let free_ptr = self.get_free_space_ptr();
        // free_space_ptr이 유효한 범위(HEADER_SIZE ~ PAGE_SIZE)인지 확인
        free_ptr > 0 && free_ptr <= PAGE_SIZE as u16
    }

    /// 슬롯 삽입 REDO
    ///
    /// Recovery 시 SlotInsert 레코드를 재실행합니다.
    pub fn redo_slot_insert(&mut self, slot_id: SlotId, data: &[u8]) -> Result<()> {
        // 페이지가 초기화되지 않았으면 먼저 초기화
        if !self.is_initialized() {
            let page_id = self.page.id.unwrap_or(0);
            self.init(page_id);
        }

        // 슬롯이 이미 존재하면 스킵 (이미 REDO됨)
        if slot_id < self.get_slot_count() {
            let (_, len) = self.get_slot(slot_id);
            if len > 0 {
                return Ok(()); // 이미 존재
            }
        }

        // 새 레코드 삽입
        let _ = self.insert_record(data)?;
        Ok(())
    }

    /// 슬롯 삭제 REDO
    ///
    /// Recovery 시 SlotDelete 레코드를 재실행합니다.
    pub fn redo_slot_delete(&mut self, slot_id: SlotId) -> Result<()> {
        if slot_id >= self.get_slot_count() {
            return Ok(()); // 슬롯 없음
        }

        let (_, len) = self.get_slot(slot_id);
        if len == 0 {
            return Ok(()); // 이미 삭제됨
        }

        self.delete_record(slot_id)?;
        Ok(())
    }

    /// 슬롯 업데이트 REDO
    ///
    /// Recovery 시 SlotUpdate 레코드를 재실행합니다.
    pub fn redo_slot_update(&mut self, slot_id: SlotId, after_data: &[u8]) -> Result<()> {
        if slot_id >= self.get_slot_count() {
            return Ok(()); // 슬롯 없음
        }

        let (_, len) = self.get_slot(slot_id);
        if len == 0 {
            return Ok(()); // 삭제된 슬롯
        }

        self.update_record(slot_id, after_data)?;
        Ok(())
    }

    /// 슬롯 삽입 UNDO (삽입된 슬롯 삭제)
    pub fn undo_slot_insert(&mut self, slot_id: SlotId) -> Result<()> {
        if slot_id >= self.get_slot_count() {
            return Ok(()); // 슬롯 없음
        }

        let (_, len) = self.get_slot(slot_id);
        if len == 0 {
            return Ok(()); // 이미 삭제됨
        }

        self.delete_record(slot_id)?;
        Ok(())
    }

    /// 슬롯 삭제 UNDO (삭제된 슬롯 복원)
    ///
    /// 삭제된 슬롯의 데이터를 복원합니다.
    /// 현재 구현: 새 슬롯에 삽입 (원래 slot_id와 다를 수 있음)
    pub fn undo_slot_delete(&mut self, data: &[u8]) -> Result<SlotId> {
        // 데이터 복원 (새 슬롯에 삽입)
        let slot_id = self.insert_record(data)?;
        Ok(slot_id)
    }

    /// 슬롯 업데이트 UNDO (이전 데이터로 복원)
    pub fn undo_slot_update(&mut self, slot_id: SlotId, before_data: &[u8]) -> Result<()> {
        if slot_id >= self.get_slot_count() {
            return Ok(()); // 슬롯 없음
        }

        let (_, len) = self.get_slot(slot_id);
        if len == 0 {
            return Ok(()); // 삭제된 슬롯
        }

        self.update_record(slot_id, before_data)?;
        Ok(())
    }
}
