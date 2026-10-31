// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! B+Tree Slotted Page 래퍼
//!
//! SlottedPage 위에 B+Tree 특화 기능을 제공합니다.
//! - B+Tree 메타데이터 관리 (slot 0에 저장)
//! - 정렬된 엔트리 관리
//! - Slot-level WAL 로깅 지원

use crate::error::Result;
use crate::storage::disk::page::Page;
use crate::storage::disk::page::{read_u16_le, read_u32_le, read_u64_le};
use crate::storage::disk::slotted_page::{SlotId, SlottedPage};
use crate::transaction::lsn::LSN;
use crate::transaction::{PageStorageType, PageWAL, TransactionId};

use super::types::{INVALID_PAGE_ID, IndexPageType, RecordId};

/// B+Tree 메타데이터 크기 (slot 0에 저장)
const BTREE_META_SIZE: usize = 20;

/// B+Tree 메타데이터 오프셋
const META_PAGE_TYPE: usize = 0; // 4 bytes
const META_IS_ROOT: usize = 4; // 4 bytes
const META_PARENT_ID: usize = 8; // 4 bytes
const META_NEXT_PAGE_ID: usize = 12; // 4 bytes (Leaf only)
const META_LEVEL: usize = 16; // 4 bytes

/// B+Tree 전용 SlottedPage 래퍼
///
/// 구조:
/// - Slot 0: B+Tree 메타데이터 (20 bytes)
/// - Slot 1+: 실제 키-값 엔트리
pub struct BTreeSlottedPage<'a> {
    page: &'a mut Page,
}

impl<'a> BTreeSlottedPage<'a> {
    /// 새 래퍼 생성
    pub fn new(page: &'a mut Page) -> Self {
        Self { page }
    }

    /// B+Tree 페이지로 초기화
    pub fn init(&mut self, page_id: u32, page_type: IndexPageType, parent_id: u32, level: u32) {
        // SlottedPage 초기화
        {
            let mut slotted = SlottedPage::new(self.page);
            slotted.init(page_id);
        }

        // 메타데이터 슬롯 생성 (slot 0)
        let meta = self.create_metadata(page_type, false, parent_id, INVALID_PAGE_ID, level);
        {
            let mut slotted = SlottedPage::new(self.page);
            let _ = slotted.insert_record(&meta);
        }
    }

    /// 메타데이터 바이트 배열 생성
    fn create_metadata(
        &self,
        page_type: IndexPageType,
        is_root: bool,
        parent_id: u32,
        next_page_id: u32,
        level: u32,
    ) -> [u8; BTREE_META_SIZE] {
        let mut meta = [0u8; BTREE_META_SIZE];

        let type_val: u32 = match page_type {
            IndexPageType::LeafPage => 1,
            IndexPageType::InternalPage => 2,
        };
        meta[META_PAGE_TYPE..META_PAGE_TYPE + 4].copy_from_slice(&type_val.to_le_bytes());
        meta[META_IS_ROOT..META_IS_ROOT + 4]
            .copy_from_slice(&(if is_root { 1u32 } else { 0u32 }).to_le_bytes());
        meta[META_PARENT_ID..META_PARENT_ID + 4].copy_from_slice(&parent_id.to_le_bytes());
        meta[META_NEXT_PAGE_ID..META_NEXT_PAGE_ID + 4].copy_from_slice(&next_page_id.to_le_bytes());
        meta[META_LEVEL..META_LEVEL + 4].copy_from_slice(&level.to_le_bytes());

        meta
    }

    // ==================== 메타데이터 접근자 ====================

    /// 메타데이터 slot(slot 0)이 존재하는지 확인
    fn has_metadata_slot(&self) -> bool {
        const SLOT_COUNT_OFFSET: usize = 12;
        let slot_count =
            read_u16_le(&self.page.data[SLOT_COUNT_OFFSET..SLOT_COUNT_OFFSET + size_of::<u16>()]);
        slot_count > 0
    }

    /// 메타데이터에서 u32 읽기 (slot 0의 데이터 직접 접근)
    fn read_meta_u32(&self, offset: usize) -> u32 {
        use crate::storage::disk::page::PAGE_SIZE;

        // SlottedPage 헤더 상수
        const HEADER_SIZE: usize = 16;
        const SLOT_COUNT_OFFSET: usize = 12;

        // slot_count 확인 (slot 0이 존재하는지)
        let slot_count =
            read_u16_le(&self.page.data[SLOT_COUNT_OFFSET..SLOT_COUNT_OFFSET + size_of::<u16>()]);

        if slot_count == 0 {
            tracing::warn!("BTreeSlottedPage::read_meta_u32: no slots, returning 0");
            return 0;
        }

        // slot 0의 offset 읽기
        let slot_offset_pos = HEADER_SIZE;
        let data_offset =
            read_u16_le(&self.page.data[slot_offset_pos..slot_offset_pos + size_of::<u16>()]);

        // 오프셋 유효성 검사
        let meta_pos = data_offset as usize + offset;
        if meta_pos + 4 > PAGE_SIZE {
            tracing::warn!(
                "BTreeSlottedPage::read_meta_u32: invalid offset {} (data_offset={}, offset={}), returning 0",
                meta_pos,
                data_offset,
                offset
            );
            return 0;
        }

        // 메타데이터에서 값 읽기
        read_u32_le(&self.page.data[meta_pos..meta_pos + size_of::<u32>()])
    }

    /// 메타데이터에 u32 쓰기
    fn write_meta_u32(&mut self, offset: usize, value: u32) {
        use crate::storage::disk::page::PAGE_SIZE;

        // SlottedPage 헤더 상수
        const HEADER_SIZE: usize = 16;
        const SLOT_COUNT_OFFSET: usize = 12;

        // slot_count 확인 (slot 0이 존재하는지)
        let slot_count =
            read_u16_le(&self.page.data[SLOT_COUNT_OFFSET..SLOT_COUNT_OFFSET + size_of::<u16>()]);

        if slot_count == 0 {
            tracing::warn!("BTreeSlottedPage::write_meta_u32: no slots, skipping write");
            return;
        }

        // slot 0의 offset 읽기
        let slot_offset_pos = HEADER_SIZE;
        let data_offset =
            read_u16_le(&self.page.data[slot_offset_pos..slot_offset_pos + size_of::<u16>()]);

        // 오프셋 유효성 검사
        let meta_pos = data_offset as usize + offset;
        if meta_pos + 4 > PAGE_SIZE {
            tracing::warn!(
                "BTreeSlottedPage::write_meta_u32: invalid offset {} (data_offset={}, offset={}), skipping write",
                meta_pos,
                data_offset,
                offset
            );
            return;
        }

        // 메타데이터에 값 쓰기
        self.page.data[meta_pos..meta_pos + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// 슬롯 정보 읽기
    fn get_slot_info(&self, slot_id: SlotId) -> (u16, u16) {
        const HEADER_SIZE: usize = 16;
        const SLOT_SIZE: usize = 4;
        let offset = HEADER_SIZE + (slot_id as usize * SLOT_SIZE);
        let data_offset = read_u16_le(&self.page.data[offset..offset + size_of::<u16>()]);
        let data_len =
            read_u16_le(&self.page.data[offset + size_of::<u16>()..offset + size_of::<u32>()]);
        (data_offset, data_len)
    }

    /// 페이지 타입
    pub fn page_type(&self) -> IndexPageType {
        match self.read_meta_u32(META_PAGE_TYPE) {
            1 => IndexPageType::LeafPage,
            _ => IndexPageType::InternalPage,
        }
    }

    /// 루트 페이지 여부
    pub fn is_root(&self) -> bool {
        self.read_meta_u32(META_IS_ROOT) == 1
    }

    /// 루트 설정
    pub fn set_root(&mut self, is_root: bool) {
        self.write_meta_u32(META_IS_ROOT, if is_root { 1 } else { 0 });
    }

    /// 부모 페이지 ID
    pub fn parent_page_id(&self) -> u32 {
        self.read_meta_u32(META_PARENT_ID)
    }

    /// 부모 페이지 ID 설정
    pub fn set_parent_page_id(&mut self, parent_id: u32) {
        self.write_meta_u32(META_PARENT_ID, parent_id);
    }

    /// 다음 페이지 ID (Leaf용 linked list)
    ///
    /// 메타데이터가 손상되어 slot이 없으면 INVALID_PAGE_ID를 반환하여
    /// range scan 무한 루프를 방지합니다.
    pub fn next_page_id(&self) -> u32 {
        if !self.has_metadata_slot() {
            return INVALID_PAGE_ID;
        }
        self.read_meta_u32(META_NEXT_PAGE_ID)
    }

    /// 다음 페이지 ID 설정
    pub fn set_next_page_id(&mut self, next_id: u32) {
        self.write_meta_u32(META_NEXT_PAGE_ID, next_id);
    }

    /// 레벨 (0 = Leaf)
    pub fn level(&self) -> u32 {
        self.read_meta_u32(META_LEVEL)
    }

    // ==================== 엔트리 관리 ====================

    /// 슬롯 개수 읽기 (직접 접근)
    fn get_slot_count(&self) -> u16 {
        const OFFSET_SLOT_COUNT: usize = 12;
        read_u16_le(&self.page.data[OFFSET_SLOT_COUNT..OFFSET_SLOT_COUNT + size_of::<u16>()])
    }

    /// 엔트리 개수 (메타데이터 및 삭제된 슬롯 제외)
    pub fn entry_count(&self) -> usize {
        let total_slots = self.get_slot_count() as usize;
        if total_slots <= 1 {
            return 0; // 메타데이터 슬롯만 있거나 없음
        }

        let mut count = 0;
        for slot_id in 1..total_slots {
            let (_, len) = self.get_slot_info(slot_id as SlotId);
            if len > 0 {
                count += 1;
            }
        }
        count
    }

    /// 엔트리 삽입 (정렬 유지 안 함, 끝에 추가)
    pub fn insert_entry(&mut self, data: &[u8]) -> Result<SlotId> {
        let mut slotted = SlottedPage::new(self.page);
        slotted.insert_record(data)
    }

    /// 엔트리 삽입 + WAL 로깅
    pub fn insert_entry_with_wal(
        &mut self,
        data: &[u8],
        page_wal: &PageWAL,
        tx_id: TransactionId,
        storage_type: PageStorageType,
    ) -> Result<(SlotId, LSN)> {
        let mut slotted = SlottedPage::new(self.page);
        slotted.insert_record_with_wal(data, page_wal, tx_id, storage_type)
    }

    /// 엔트리 읽기 (entry_idx는 0부터 시작, 삭제되지 않은 슬롯만 카운트)
    pub fn get_entry(&self, entry_idx: usize) -> Option<&[u8]> {
        use crate::storage::disk::page::PAGE_SIZE;

        let total_slots = self.get_slot_count() as usize;
        if total_slots <= 1 {
            return None;
        }

        let mut found = 0;
        for slot_id in 1..total_slots {
            let (offset, len) = self.get_slot_info(slot_id as SlotId);
            if len > 0 {
                if found == entry_idx {
                    let start = offset as usize;
                    let end = start + len as usize;
                    // 범위 검사
                    if end > PAGE_SIZE {
                        tracing::warn!(
                            "BTreeSlottedPage::get_entry: invalid range {}..{} (PAGE_SIZE={})",
                            start,
                            end,
                            PAGE_SIZE
                        );
                        return None;
                    }
                    return Some(&self.page.data[start..end]);
                }
                found += 1;
            }
        }
        None
    }

    /// 슬롯 ID로 직접 엔트리 읽기 (내부 사용)
    #[expect(dead_code)]
    fn get_entry_by_slot(&self, slot_id: SlotId) -> Option<&[u8]> {
        use crate::storage::disk::page::PAGE_SIZE;

        if slot_id == 0 || slot_id >= self.get_slot_count() {
            return None;
        }

        let (offset, len) = self.get_slot_info(slot_id);
        if len == 0 {
            return None;
        }

        let start = offset as usize;
        let end = start + len as usize;
        // 범위 검사
        if end > PAGE_SIZE {
            tracing::warn!(
                "BTreeSlottedPage::get_entry_by_slot: invalid range {}..{} (PAGE_SIZE={})",
                start,
                end,
                PAGE_SIZE
            );
            return None;
        }
        Some(&self.page.data[start..end])
    }

    /// entry_idx에 해당하는 실제 slot_id 찾기
    fn find_slot_id_for_entry(&self, entry_idx: usize) -> Option<SlotId> {
        let total_slots = self.get_slot_count() as usize;
        if total_slots <= 1 {
            return None;
        }

        let mut found = 0;
        for slot_id in 1..total_slots {
            let (_, len) = self.get_slot_info(slot_id as SlotId);
            if len > 0 {
                if found == entry_idx {
                    return Some(slot_id as SlotId);
                }
                found += 1;
            }
        }
        None
    }

    /// 엔트리 삭제
    pub fn delete_entry(&mut self, entry_idx: usize) -> Result<()> {
        let slot_id = self.find_slot_id_for_entry(entry_idx).ok_or_else(|| {
            crate::error::GraphError::Storage {
                message: format!("Entry {} not found", entry_idx),
            }
        })?;
        let mut slotted = SlottedPage::new(self.page);
        slotted.delete_record(slot_id)
    }

    /// 삭제된 슬롯 공간 회수 (메타데이터 슬롯 0은 유지)
    ///
    /// **주의**: SlottedPage::compact()는 슬롯 ID를 재할당합니다.
    /// B+Tree에서는 compact_entries()를 사용하세요.
    ///
    /// Returns: 회수된 바이트 수
    pub fn compact(&mut self) -> Result<usize> {
        let mut slotted = SlottedPage::new(self.page);
        Ok(slotted.compact())
    }

    /// 엔트리 압축 (메타데이터 보존, 삭제된 슬롯 제거)
    ///
    /// 삭제된 슬롯을 완전히 제거하고 slot_count를 감소시킵니다.
    /// 메타데이터(slot 0)는 보존됩니다.
    pub fn compact_entries(&mut self) -> Result<()> {
        // 1. 현재 메타데이터 저장
        let page_type = self.page_type();
        let is_root = self.is_root();
        let parent_id = self.parent_page_id();
        let next_page_id = self.next_page_id();
        let level = self.level();
        let page_id = self.page.id.unwrap_or(0);

        // 2. 유효한 엔트리 수집 (slot 1부터 전체 슬롯 순회)
        let total_slots = {
            let slotted = SlottedPage::new(self.page);
            slotted.get_slot_count() as usize
        };
        let mut entries: Vec<Vec<u8>> = Vec::new();
        for slot_id in 1..total_slots {
            let slotted = SlottedPage::new(self.page);
            // 삭제된 슬롯(len=0)은 건너뜀
            if let Some(data) = slotted.get_record(slot_id as u16) {
                entries.push(data.to_vec());
            }
        }

        // 3. 페이지 초기화
        {
            let mut slotted = SlottedPage::new(self.page);
            slotted.init(page_id);
        }

        // 4. 메타데이터 재삽입 (slot 0)
        let meta = self.create_metadata(page_type, is_root, parent_id, next_page_id, level);
        {
            let mut slotted = SlottedPage::new(self.page);
            let _ = slotted.insert_record(&meta);
        }

        // 5. 엔트리 재삽입
        for entry_data in entries {
            let mut slotted = SlottedPage::new(self.page);
            let _ = slotted.insert_record(&entry_data);
        }

        Ok(())
    }

    /// 모든 엔트리 슬롯 제거 (메타데이터 slot 0만 유지)
    ///
    /// delete + compact 대신 이 메서드를 사용하면 슬롯 카운트도 리셋됩니다.
    /// delete_record()는 슬롯을 삭제하지 않고 len=0으로만 표시하므로
    /// 슬롯 공간이 계속 증가하는 문제가 있습니다.
    pub fn reset_entries(&mut self) -> Result<()> {
        // 1. 현재 메타데이터 읽기
        let page_type = self.page_type();
        let is_root = self.is_root();
        let parent_id = self.parent_page_id();
        let next_page_id = self.next_page_id();
        let level = self.level();
        let page_id = self.page.id.unwrap_or(0);

        // 2. 페이지 완전 초기화 (모든 슬롯 제거)
        {
            let mut slotted = SlottedPage::new(self.page);
            slotted.init(page_id);
        }

        // 3. 메타데이터 다시 삽입 (slot 0)
        let meta = self.create_metadata(page_type, is_root, parent_id, next_page_id, level);
        {
            let mut slotted = SlottedPage::new(self.page);
            let _ = slotted.insert_record(&meta);
        }

        Ok(())
    }

    /// 엔트리 삭제 + WAL 로깅
    pub fn delete_entry_with_wal(
        &mut self,
        entry_idx: usize,
        page_wal: &PageWAL,
        tx_id: TransactionId,
        storage_type: PageStorageType,
    ) -> Result<LSN> {
        let slot_id = self.find_slot_id_for_entry(entry_idx).ok_or_else(|| {
            crate::error::GraphError::Storage {
                message: format!("Entry {} not found", entry_idx),
            }
        })?;
        let mut slotted = SlottedPage::new(self.page);
        slotted.delete_record_with_wal(slot_id, page_wal, tx_id, storage_type)
    }

    /// free_space_ptr 읽기 (직접 접근)
    fn get_free_space_ptr(&self) -> u16 {
        const OFFSET_FREE_SPACE_PTR: usize = 14;
        read_u16_le(
            &self.page.data[OFFSET_FREE_SPACE_PTR..OFFSET_FREE_SPACE_PTR + size_of::<u16>()],
        )
    }

    /// 사용 가능한 공간 계산
    pub fn free_space(&self) -> usize {
        const HEADER_SIZE: usize = 16;
        const SLOT_SIZE: usize = 4;
        let slot_count = self.get_slot_count() as usize;
        let free_ptr = self.get_free_space_ptr() as usize;
        let used_header_space = HEADER_SIZE + (slot_count * SLOT_SIZE);
        free_ptr.saturating_sub(used_header_space)
    }

    /// 페이지가 가득 찼는지 확인
    pub fn is_full(&self) -> bool {
        // 최소 14바이트 엔트리 (key 8 + value 6) 공간이 없으면 full
        self.free_space() < 20 // 14 bytes + slot entry 4 bytes + margin
    }

    /// 기본 Page 참조
    pub fn page(&self) -> &Page {
        self.page
    }

    /// 기본 Page mutable 참조
    pub fn page_mut(&mut self) -> &mut Page {
        self.page
    }
}

// ==================== Leaf 엔트리 (Key-RecordId) ====================

/// Leaf 엔트리 직렬화 형식
/// [key: u64 (8 bytes)][page_id: u32 (4 bytes)][slot_id: u16 (2 bytes)]
pub const LEAF_ENTRY_SIZE: usize = 14;

/// Leaf 엔트리 직렬화
pub fn serialize_leaf_entry(key: u64, value: RecordId) -> [u8; LEAF_ENTRY_SIZE] {
    let mut data = [0u8; LEAF_ENTRY_SIZE];
    data[0..8].copy_from_slice(&key.to_le_bytes());
    data[8..12].copy_from_slice(&value.page_id.to_le_bytes());
    data[12..14].copy_from_slice(&value.slot_id.to_le_bytes());
    data
}

/// Leaf 엔트리 역직렬화
pub fn deserialize_leaf_entry(data: &[u8]) -> (u64, RecordId) {
    let key = read_u64_le(&data[0..8]);
    let page_id = read_u32_le(&data[8..12]);
    let slot_id = read_u16_le(&data[12..14]);
    (key, RecordId { page_id, slot_id })
}

// ==================== Internal 엔트리 (Key-PageId) ====================

/// Internal 엔트리 직렬화 형식
/// [key: u64 (8 bytes)][child_page_id: u32 (4 bytes)]
pub const INTERNAL_ENTRY_SIZE: usize = 12;

/// Internal 엔트리 직렬화
pub fn serialize_internal_entry(key: u64, child_page_id: u32) -> [u8; INTERNAL_ENTRY_SIZE] {
    let mut data = [0u8; INTERNAL_ENTRY_SIZE];
    data[0..8].copy_from_slice(&key.to_le_bytes());
    data[8..12].copy_from_slice(&child_page_id.to_le_bytes());
    data
}

/// Internal 엔트리 역직렬화
pub fn deserialize_internal_entry(data: &[u8]) -> (u64, u32) {
    let key = read_u64_le(&data[0..8]);
    let child_page_id = read_u32_le(&data[8..12]);
    (key, child_page_id)
}
