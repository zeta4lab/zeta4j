//! B+Tree Internal Page (SlottedPage 기반)
//!
//! SlottedPage를 사용하여 가변 길이 키와 Slot-level WAL을 지원합니다.
//!
//! 레이아웃:
//! - Slot 0: B+Tree 메타데이터 (page_type, is_root, parent_id, next_page_id, level)
//! - Slot 1+: 정렬된 (key, child_page_id) 엔트리

use super::slotted::{
    BTreeSlottedPage, INTERNAL_ENTRY_SIZE, deserialize_internal_entry, serialize_internal_entry,
};
use super::types::IndexPageType;
use crate::error::Result;
use crate::storage::disk::page::{PAGE_SIZE, Page};
use crate::storage::disk::slotted_page::SlotId;
use crate::transaction::lsn::LSN;
use crate::transaction::{PageStorageType, PageWAL, TransactionId};

/// B+ Tree Internal Page
pub struct InternalPage<'a> {
    btree_page: BTreeSlottedPage<'a>,
}

impl<'a> InternalPage<'a> {
    pub fn new(page: &'a mut Page) -> Self {
        Self {
            btree_page: BTreeSlottedPage::new(page),
        }
    }

    pub fn init(&mut self, parent_id: u32) {
        if let Some(page_id) = self.btree_page.page().id {
            self.btree_page
                .init(page_id, IndexPageType::InternalPage, parent_id, 1);
        } else {
            self.btree_page
                .init(0, IndexPageType::InternalPage, parent_id, 1);
        }
    }

    // --- Header Accessors ---

    pub fn set_page_type(&mut self, _page_type: IndexPageType) {
        // BTreeSlottedPage의 메타데이터에 저장됨 (init 시 설정)
    }

    pub fn get_page_type(&self) -> IndexPageType {
        self.btree_page.page_type()
    }

    pub fn get_parent_page_id(&self) -> u32 {
        self.btree_page.parent_page_id()
    }

    pub fn set_parent_page_id(&mut self, parent_id: u32) {
        self.btree_page.set_parent_page_id(parent_id);
    }

    pub fn is_root_page(&self) -> bool {
        self.btree_page.is_root()
    }

    pub fn set_root_page(&mut self, root: bool) {
        self.btree_page.set_root(root);
    }

    pub fn get_size(&self) -> u32 {
        self.btree_page.entry_count() as u32
    }

    pub fn set_size(&mut self, _size: u32) {
        // SlottedPage는 자동으로 크기 관리
    }

    // --- Key/Value Accessors ---

    /// 최대 엔트리 개수 (페이지 크기 기반)
    pub fn get_max_size(&self) -> usize {
        // SlottedPage 오버헤드: 헤더 16 + 메타데이터 슬롯 (4 + 20) + 각 엔트리 슬롯 (4)
        // 각 엔트리: 12 bytes + 4 bytes slot = 16 bytes
        (PAGE_SIZE - 40) / (INTERNAL_ENTRY_SIZE + 4)
    }

    /// 최소 엔트리 개수 (언더플로우 임계값)
    pub fn get_min_size(&self) -> usize {
        self.get_max_size() / 2
    }

    /// 인덱스 위치의 키 반환
    pub fn key_at(&self, index: usize) -> u64 {
        if let Some(data) = self.btree_page.get_entry(index) {
            let (key, _) = deserialize_internal_entry(data);
            key
        } else {
            0
        }
    }

    /// 인덱스 위치의 자식 페이지 ID 반환
    pub fn value_at(&self, index: usize) -> u32 {
        if let Some(data) = self.btree_page.get_entry(index) {
            let (_, page_id) = deserialize_internal_entry(data);
            page_id
        } else {
            0
        }
    }

    /// 인덱스 위치에 키-값 설정
    pub fn set_at(&mut self, index: usize, key: u64, value: u32) {
        let size = self.get_size() as usize;
        if index >= size {
            return;
        }

        // 모든 엔트리 수집
        let mut entries: Vec<(u64, u32)> = Vec::with_capacity(size);
        for i in 0..size {
            if i == index {
                entries.push((key, value));
            } else if let Some(data) = self.btree_page.get_entry(i) {
                entries.push(deserialize_internal_entry(data));
            }
        }

        // 모든 엔트리 슬롯 제거 (메타데이터 유지, 슬롯 카운트 리셋)
        let _ = self.btree_page.reset_entries();

        for (k, v) in entries {
            let data = serialize_internal_entry(k, v);
            let _ = self.btree_page.insert_entry(&data);
        }
    }

    /// 주어진 키보다 작거나 같은 가장 큰 키의 자식 페이지 ID 반환
    pub fn lookup(&self, key: u64) -> u32 {
        let size = self.get_size() as usize;
        if size == 0 {
            return u32::MAX; // INVALID_PAGE_ID — 빈 내부 페이지는 유효한 자식 없음
        }
        if size == 1 {
            return self.value_at(0);
        }

        // 이진 탐색: key보다 큰 첫 번째 키의 위치 찾기 (upper_bound)
        let mut left = 1;
        let mut right = size;

        while left < right {
            let mid = left + (right - left) / 2;
            if self.key_at(mid) <= key {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        if left > 0 {
            self.value_at(left - 1)
        } else {
            self.value_at(0)
        }
    }

    /// 특정 인덱스의 엔트리 제거
    pub fn remove_at(&mut self, index: usize) {
        let _ = self.btree_page.delete_entry(index);
        // 삭제된 슬롯 정리 (슬롯 테이블 단편화 방지)
        let _ = self.btree_page.compact_entries();
    }

    /// 특정 PageId를 가진 엔트리의 인덱스 찾기
    pub fn find_child_index(&self, page_id: u32) -> Option<usize> {
        let size = self.get_size();
        (0..size as usize).find(|&i| self.value_at(i) == page_id)
    }

    /// 특정 PageId를 가진 엔트리 제거
    pub fn remove_child(&mut self, page_id: u32) -> bool {
        if let Some(idx) = self.find_child_index(page_id) {
            self.remove_at(idx);
            true
        } else {
            false
        }
    }

    /// 특정 인덱스의 키 업데이트
    pub fn update_key_at(&mut self, index: usize, new_key: u64) {
        let value = self.value_at(index);
        self.set_at(index, new_key, value);
    }

    /// 특정 PageId 값을 가진 엔트리 뒤에 새 키와 PageId 삽입
    pub fn insert_node_after(&mut self, old_page_id: u32, new_key: u64, new_page_id: u32) -> bool {
        let size = self.get_size() as usize;
        if size >= self.get_max_size() {
            return false;
        }

        // 모든 엔트리 수집
        let mut entries: Vec<(u64, u32)> = Vec::with_capacity(size + 1);
        for i in 0..size {
            if let Some(data) = self.btree_page.get_entry(i) {
                let (k, v) = deserialize_internal_entry(data);
                entries.push((k, v));
                if v == old_page_id {
                    entries.push((new_key, new_page_id));
                }
            }
        }

        // 모든 엔트리 슬롯 제거 (메타데이터 유지, 슬롯 카운트 리셋)
        let _ = self.btree_page.reset_entries();

        for (k, v) in entries {
            let data = serialize_internal_entry(k, v);
            if self.btree_page.insert_entry(&data).is_err() {
                return false;
            }
        }

        true
    }

    /// Internal 페이지가 꽉 찼는지 확인
    pub fn is_full(&self) -> bool {
        self.get_size() as usize >= self.get_max_size()
    }

    /// 언더플로우 상태인지 확인
    pub fn is_underflow(&self) -> bool {
        (self.get_size() as usize) < self.get_min_size()
    }

    /// Internal 페이지 분할
    pub fn split(&mut self, new_page: &mut InternalPage) -> u64 {
        let size = self.get_size() as usize;
        let mid = size / 2;

        // 중간 키가 부모로 승격됨
        let promoted_key = self.key_at(mid);

        // mid부터 끝까지 새 페이지로 복사
        for i in mid..size {
            let k = self.key_at(i);
            let v = self.value_at(i);
            let data = serialize_internal_entry(k, v);
            let _ = new_page.btree_page.insert_entry(&data);
        }

        // 원본 페이지에서 후반부 삭제 (역순)
        for i in (mid..size).rev() {
            let _ = self.btree_page.delete_entry(i);
        }
        // 삭제된 슬롯 정리 (슬롯 테이블 단편화 방지)
        let _ = self.btree_page.compact_entries();

        promoted_key
    }

    /// 특정 인덱스에 키와 페이지 삽입
    pub fn insert_at(&mut self, index: usize, key: u64, page_id: u32) {
        let size = self.get_size() as usize;

        // 모든 엔트리 수집
        let mut entries: Vec<(u64, u32)> = Vec::with_capacity(size + 1);
        for i in 0..size {
            if i == index {
                entries.push((key, page_id));
            }
            if let Some(data) = self.btree_page.get_entry(i) {
                entries.push(deserialize_internal_entry(data));
            }
        }
        if index >= size {
            entries.push((key, page_id));
        }

        // 모든 엔트리 슬롯 제거 (메타데이터 유지, 슬롯 카운트 리셋)
        let _ = self.btree_page.reset_entries();

        for (k, v) in entries {
            let data = serialize_internal_entry(k, v);
            let _ = self.btree_page.insert_entry(&data);
        }
    }

    /// 정렬된 위치에 키와 페이지 삽입
    pub fn insert_sorted(&mut self, key: u64, page_id: u32) -> bool {
        let size = self.get_size() as usize;
        if size >= self.get_max_size() {
            return false;
        }

        // 모든 엔트리 수집
        let mut entries: Vec<(u64, u32)> = Vec::with_capacity(size + 1);
        for i in 0..size {
            if let Some(data) = self.btree_page.get_entry(i) {
                entries.push(deserialize_internal_entry(data));
            }
        }

        // 삽입 위치 찾기 (upper_bound)
        let insert_idx = if size <= 1 {
            size
        } else {
            let mut left = 1;
            let mut right = size;
            while left < right {
                let mid = left + (right - left) / 2;
                if entries[mid].0 <= key {
                    left = mid + 1;
                } else {
                    right = mid;
                }
            }
            left
        };

        entries.insert(insert_idx, (key, page_id));

        // 모든 엔트리 슬롯 제거 (메타데이터 유지, 슬롯 카운트 리셋)
        let _ = self.btree_page.reset_entries();

        for (k, v) in entries {
            let data = serialize_internal_entry(k, v);
            if self.btree_page.insert_entry(&data).is_err() {
                return false;
            }
        }

        true
    }

    /// 왼쪽 형제에서 빌려오기
    pub fn borrow_from_left_internal(
        &mut self,
        left_sibling: &mut InternalPage,
        parent_separator: u64,
    ) -> Option<(u64, u32)> {
        let left_size = left_sibling.get_size();
        if left_size <= left_sibling.get_min_size() as u32 {
            return None;
        }

        let last_idx = (left_size - 1) as usize;
        let borrowed_key = left_sibling.key_at(last_idx);
        let borrowed_child = left_sibling.value_at(last_idx);

        // 현재 페이지 모든 엔트리 수집
        let my_size = self.get_size() as usize;
        let mut entries: Vec<(u64, u32)> = Vec::with_capacity(my_size + 1);

        // 빌려온 자식을 맨 앞에 추가 (키는 0으로 설정)
        entries.push((0, borrowed_child));

        // 기존 엔트리 추가 (첫 번째 키는 parent_separator로 설정)
        for i in 0..my_size {
            let k = if i == 0 {
                parent_separator
            } else {
                self.key_at(i)
            };
            let v = self.value_at(i);
            entries.push((k, v));
        }

        // 현재 페이지 재구성 (reset_entries로 슬롯 공간 완전 초기화)
        let _ = self.btree_page.reset_entries();
        for (k, v) in &entries {
            let data = serialize_internal_entry(*k, *v);
            let _ = self.btree_page.insert_entry(&data);
        }

        // 왼쪽 형제 재구성 (마지막 항목 제외)
        let mut left_entries: Vec<(u64, u32)> = Vec::with_capacity(last_idx);
        for i in 0..last_idx {
            left_entries.push((left_sibling.key_at(i), left_sibling.value_at(i)));
        }
        let _ = left_sibling.btree_page.reset_entries();
        for (k, v) in &left_entries {
            let data = serialize_internal_entry(*k, *v);
            let _ = left_sibling.btree_page.insert_entry(&data);
        }

        Some((borrowed_key, borrowed_child))
    }

    /// 오른쪽 형제에서 빌려오기
    pub fn borrow_from_right_internal(
        &mut self,
        right_sibling: &mut InternalPage,
        parent_separator: u64,
    ) -> Option<(u64, u32)> {
        let right_size = right_sibling.get_size();
        if right_size <= right_sibling.get_min_size() as u32 {
            return None;
        }

        let borrowed_child = right_sibling.value_at(0);

        // 현재 페이지의 모든 엔트리 수집
        let my_size = self.get_size() as usize;
        let mut entries: Vec<(u64, u32)> = Vec::with_capacity(my_size + 1);
        for i in 0..my_size {
            entries.push((self.key_at(i), self.value_at(i)));
        }
        entries.push((parent_separator, borrowed_child)); // 끝에 추가

        // 현재 페이지 재구성 (reset_entries로 슬롯 공간 완전 초기화)
        let _ = self.btree_page.reset_entries();
        for (k, v) in &entries {
            let data = serialize_internal_entry(*k, *v);
            let _ = self.btree_page.insert_entry(&data);
        }

        // 오른쪽 형제 재구성 (첫 번째 항목 제외)
        let mut right_entries: Vec<(u64, u32)> = Vec::with_capacity(right_size as usize - 1);
        for i in 1..right_size as usize {
            right_entries.push((right_sibling.key_at(i), right_sibling.value_at(i)));
        }

        let _ = right_sibling.btree_page.reset_entries();
        for (k, v) in &right_entries {
            let data = serialize_internal_entry(*k, *v);
            let _ = right_sibling.btree_page.insert_entry(&data);
        }

        let new_separator = right_entries.first().map(|(k, _)| *k).unwrap_or(0);
        Some((new_separator, borrowed_child))
    }

    /// 이진 탐색으로 key보다 큰 첫 번째 키의 인덱스 찾기 (upper_bound)
    #[expect(dead_code)]
    fn binary_search_upper_bound(&self, key: u64, start: usize, end: usize) -> usize {
        let mut left = start;
        let mut right = end;

        while left < right {
            let mid = left + (right - left) / 2;
            if self.key_at(mid) <= key {
                left = mid + 1;
            } else {
                right = mid;
            }
        }
        left
    }

    /// 키-값 삽입 + WAL 로깅
    pub fn insert_with_wal(
        &mut self,
        key: u64,
        page_id: u32,
        page_wal: &PageWAL,
        tx_id: TransactionId,
        storage_type: PageStorageType,
    ) -> Result<SlotId> {
        let data = serialize_internal_entry(key, page_id);
        let (slot_id, _lsn) =
            self.btree_page
                .insert_entry_with_wal(&data, page_wal, tx_id, storage_type)?;
        Ok(slot_id)
    }

    /// 엔트리 삭제 + WAL 로깅
    pub fn delete_with_wal(
        &mut self,
        index: usize,
        page_wal: &PageWAL,
        tx_id: TransactionId,
        storage_type: PageStorageType,
    ) -> Result<LSN> {
        let lsn = self
            .btree_page
            .delete_entry_with_wal(index, page_wal, tx_id, storage_type)?;
        // 삭제된 슬롯 정리 (슬롯 테이블 단편화 방지)
        let _ = self.btree_page.compact_entries();
        Ok(lsn)
    }

    /// 페이지 참조
    pub fn page(&self) -> &Page {
        self.btree_page.page()
    }

    /// 페이지 mutable 참조
    pub fn page_mut(&mut self) -> &mut Page {
        self.btree_page.page_mut()
    }

    /// 모든 엔트리 제거 (메타데이터 유지)
    pub fn reset_entries(&mut self) -> Result<()> {
        self.btree_page.reset_entries()
    }
}
