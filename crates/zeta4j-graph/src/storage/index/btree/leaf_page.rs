//! B+Tree Leaf Page (SlottedPage 기반)
//!
//! SlottedPage를 사용하여 가변 길이 키와 Slot-level WAL을 지원합니다.
//!
//! 레이아웃:
//! - Slot 0: B+Tree 메타데이터 (page_type, is_root, parent_id, next_page_id, level)
//! - Slot 1+: 정렬된 키-값 엔트리

use super::slotted::{
    BTreeSlottedPage, LEAF_ENTRY_SIZE, deserialize_leaf_entry, serialize_leaf_entry,
};
use super::types::{INVALID_PAGE_ID, IndexPageType, RecordId};
use crate::error::Result;
use crate::storage::disk::page::{PAGE_SIZE, Page};
use crate::storage::disk::slotted_page::SlotId;
use crate::transaction::lsn::LSN;
use crate::transaction::{PageStorageType, PageWAL, TransactionId};

/// B+ Tree Leaf Page
pub struct LeafPage<'a> {
    btree_page: BTreeSlottedPage<'a>,
}

impl<'a> LeafPage<'a> {
    pub fn new(page: &'a mut Page) -> Self {
        Self {
            btree_page: BTreeSlottedPage::new(page),
        }
    }

    pub fn init(&mut self, parent_id: u32) {
        if let Some(page_id) = self.btree_page.page().id {
            self.btree_page
                .init(page_id, IndexPageType::LeafPage, parent_id, 0);
        } else {
            self.btree_page
                .init(0, IndexPageType::LeafPage, parent_id, 0);
        }
    }

    // --- Header Accessors ---

    pub fn set_page_type(&mut self, page_type: IndexPageType) {
        // BTreeSlottedPage의 메타데이터에 저장됨 (init 시 설정)
        // 이 메서드는 호환성을 위해 유지하지만 실제로는 init에서 처리
        let _ = page_type;
    }

    pub fn get_page_type(&self) -> IndexPageType {
        self.btree_page.page_type()
    }

    pub fn get_size(&self) -> u32 {
        self.btree_page.entry_count() as u32
    }

    pub fn set_size(&mut self, _size: u32) {
        // SlottedPage는 자동으로 크기 관리
        // 이 메서드는 호환성을 위해 유지
    }

    pub fn set_next_page_id(&mut self, next_page_id: u32) {
        self.btree_page.set_next_page_id(next_page_id);
    }

    pub fn get_next_page_id(&self) -> u32 {
        self.btree_page.next_page_id()
    }

    pub fn set_parent_page_id(&mut self, parent_id: u32) {
        self.btree_page.set_parent_page_id(parent_id);
    }

    pub fn get_parent_page_id(&self) -> u32 {
        self.btree_page.parent_page_id()
    }

    pub fn set_root_page(&mut self, root: bool) {
        self.btree_page.set_root(root);
    }

    pub fn is_root_page(&self) -> bool {
        self.btree_page.is_root()
    }

    // --- Key/Value Accessors ---

    /// 최대 엔트리 개수 (페이지 크기 기반)
    pub fn get_max_size(&self) -> usize {
        // SlottedPage 오버헤드: 헤더 16 + 메타데이터 슬롯 (4 + 20) + 각 엔트리 슬롯 (4)
        // 각 엔트리: 14 bytes + 4 bytes slot = 18 bytes
        (PAGE_SIZE - 40) / (LEAF_ENTRY_SIZE + 4)
    }

    /// 인덱스 위치의 키 반환
    pub fn key_at(&self, index: usize) -> u64 {
        if let Some(data) = self.btree_page.get_entry(index) {
            let (key, _) = deserialize_leaf_entry(data);
            key
        } else {
            0 // 잘못된 인덱스
        }
    }

    /// 인덱스 위치의 값 반환
    pub fn value_at(&self, index: usize) -> RecordId {
        if let Some(data) = self.btree_page.get_entry(index) {
            let (_, value) = deserialize_leaf_entry(data);
            value
        } else {
            RecordId {
                page_id: INVALID_PAGE_ID,
                slot_id: 0,
            }
        }
    }

    /// 인덱스 위치에 키-값 설정 (기존 슬롯 덮어쓰기)
    pub fn set_at(&mut self, index: usize, key: u64, value: RecordId) {
        // SlottedPage는 in-place 업데이트를 지원하지 않으므로
        // delete + insert로 처리해야 하지만,
        // 현재는 간단히 구현을 위해 기존 메서드 호환성만 제공
        // 실제로는 split/merge 등에서만 사용됨

        // 현재 구현: 모든 엔트리를 읽고, index 위치의 값을 변경하고, 다시 쓰기
        let size = self.get_size() as usize;
        if index >= size {
            return;
        }

        // 모든 엔트리 수집
        let mut entries: Vec<(u64, RecordId)> = Vec::with_capacity(size);
        for i in 0..size {
            if i == index {
                entries.push((key, value));
            } else if let Some(data) = self.btree_page.get_entry(i) {
                entries.push(deserialize_leaf_entry(data));
            }
        }

        // 모든 엔트리 슬롯 제거 (메타데이터 유지, 슬롯 카운트 리셋)
        let _ = self.btree_page.reset_entries();

        for (k, v) in entries {
            let data = serialize_leaf_entry(k, v);
            let _ = self.btree_page.insert_entry(&data);
        }
    }

    /// 키-값 삽입 (정렬 순서 유지)
    pub fn insert(&mut self, key: u64, value: RecordId) -> bool {
        let size = self.get_size() as usize;
        if size >= self.get_max_size() {
            return false;
        }

        // 1. 기존 엔트리 수집
        let mut entries: Vec<(u64, RecordId)> = Vec::with_capacity(size + 1);
        for i in 0..size {
            if let Some(data) = self.btree_page.get_entry(i) {
                entries.push(deserialize_leaf_entry(data));
            }
        }

        // 2. 삽입 위치 찾기 (이진 탐색)
        let pos = match entries.binary_search_by_key(&key, |&(k, _)| k) {
            Ok(_) => return false, // 중복 키
            Err(pos) => pos,
        };

        // 3. 새 엔트리 삽입
        entries.insert(pos, (key, value));

        // 4. 모든 엔트리 슬롯 제거 (메타데이터 유지, 슬롯 카운트 리셋)
        let _ = self.btree_page.reset_entries();

        // 5. 모든 엔트리 재삽입
        for (k, v) in entries {
            let data = serialize_leaf_entry(k, v);
            if self.btree_page.insert_entry(&data).is_err() {
                return false;
            }
        }

        true
    }

    /// 키-값 삽입 (정렬 순서 유지) + WAL 로깅
    pub fn insert_with_wal(
        &mut self,
        key: u64,
        value: RecordId,
        page_wal: &PageWAL,
        tx_id: TransactionId,
        storage_type: PageStorageType,
    ) -> Result<SlotId> {
        let size = self.get_size() as usize;

        // 1. 기존 엔트리 수집
        let mut entries: Vec<(u64, RecordId)> = Vec::with_capacity(size + 1);
        for i in 0..size {
            if let Some(data) = self.btree_page.get_entry(i) {
                entries.push(deserialize_leaf_entry(data));
            }
        }

        // 2. 삽입 위치 찾기
        let pos = match entries.binary_search_by_key(&key, |&(k, _)| k) {
            Ok(_) => {
                return Err(crate::error::GraphError::Storage {
                    message: "Duplicate key".to_string(),
                });
            }
            Err(pos) => pos,
        };

        // 3. 새 엔트리 삽입
        entries.insert(pos, (key, value));

        // 4. 기존 슬롯 삭제 (역순, WAL 포함)
        for i in (0..size).rev() {
            self.btree_page
                .delete_entry_with_wal(i, page_wal, tx_id, storage_type)?;
        }

        // 5. 모든 엔트리 재삽입 (WAL 포함)
        let mut last_slot_id = 0;
        for (k, v) in entries {
            let data = serialize_leaf_entry(k, v);
            let (slot_id, _lsn) =
                self.btree_page
                    .insert_entry_with_wal(&data, page_wal, tx_id, storage_type)?;
            last_slot_id = slot_id;
        }

        Ok(last_slot_id)
    }

    /// 키가 존재하면 값 업데이트, 없으면 삽입 (upsert)
    pub fn insert_or_update(&mut self, key: u64, value: RecordId) -> bool {
        // 기존 키 검색
        if let Some(index) = self.binary_search(key) {
            // 값 업데이트
            self.set_at(index, key, value);
            return true;
        }

        // 새 키 삽입
        self.insert(key, value)
    }

    /// 이진 탐색으로 키 찾기 (존재하면 인덱스, 없으면 None)
    pub fn binary_search(&self, key: u64) -> Option<usize> {
        let size = self.get_size() as usize;
        if size == 0 {
            return None;
        }

        let mut left = 0;
        let mut right = size;

        while left < right {
            let mid = left + (right - left) / 2;
            let mid_key = self.key_at(mid);

            if mid_key == key {
                return Some(mid);
            } else if mid_key < key {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        None
    }

    /// 이진 탐색으로 삽입 위치 찾기 (key보다 크거나 같은 첫 위치)
    #[expect(dead_code)]
    fn binary_search_insert_pos(&self, key: u64, size: usize) -> usize {
        if size == 0 {
            return 0;
        }

        let mut left = 0;
        let mut right = size;

        while left < right {
            let mid = left + (right - left) / 2;
            let mid_key = self.key_at(mid);

            if mid_key < key {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        left
    }

    /// 최소 키 개수 (언더플로우 임계값)
    pub fn get_min_size(&self) -> usize {
        self.get_max_size() / 2
    }

    /// 언더플로우 상태인지 확인
    pub fn is_underflow(&self) -> bool {
        (self.get_size() as usize) < self.get_min_size()
    }

    /// 형제 페이지에서 키 빌려오기 (왼쪽 형제에서)
    pub fn borrow_from_left(&mut self, left_sibling: &mut LeafPage) -> Option<u64> {
        let left_size = left_sibling.get_size();
        if left_size <= left_sibling.get_min_size() as u32 {
            return None;
        }

        // 왼쪽 형제의 마지막 키-값을 가져옴
        let last_idx = (left_size - 1) as usize;
        let borrowed_key = left_sibling.key_at(last_idx);
        let borrowed_val = left_sibling.value_at(last_idx);

        // 현재 페이지의 모든 항목을 수집
        let my_size = self.get_size() as usize;
        let mut entries: Vec<(u64, RecordId)> = Vec::with_capacity(my_size + 1);
        entries.push((borrowed_key, borrowed_val)); // 맨 앞에 추가
        for i in 0..my_size {
            entries.push((self.key_at(i), self.value_at(i)));
        }

        // 현재 페이지 재구성 (reset_entries로 슬롯 공간 완전 초기화)
        let _ = self.btree_page.reset_entries();
        for (k, v) in &entries {
            let data = serialize_leaf_entry(*k, *v);
            let _ = self.btree_page.insert_entry(&data);
        }

        // 왼쪽 형제 재구성 (마지막 항목 제외)
        let mut left_entries: Vec<(u64, RecordId)> = Vec::with_capacity(last_idx);
        for i in 0..last_idx {
            left_entries.push((left_sibling.key_at(i), left_sibling.value_at(i)));
        }
        let _ = left_sibling.btree_page.reset_entries();
        for (k, v) in &left_entries {
            let data = serialize_leaf_entry(*k, *v);
            let _ = left_sibling.btree_page.insert_entry(&data);
        }

        Some(entries[0].0) // 새로운 분리 키 반환
    }

    /// 형제 페이지에서 키 빌려오기 (오른쪽 형제에서)
    pub fn borrow_from_right(&mut self, right_sibling: &mut LeafPage) -> Option<u64> {
        let right_size = right_sibling.get_size();
        if right_size <= right_sibling.get_min_size() as u32 {
            return None;
        }

        // 오른쪽 형제의 첫 번째 키-값을 가져옴
        let borrowed_key = right_sibling.key_at(0);
        let borrowed_val = right_sibling.value_at(0);

        // 현재 페이지의 모든 항목 수집 후 빌려온 항목 추가
        let my_size = self.get_size() as usize;
        let mut entries: Vec<(u64, RecordId)> = Vec::with_capacity(my_size + 1);
        for i in 0..my_size {
            entries.push((self.key_at(i), self.value_at(i)));
        }
        entries.push((borrowed_key, borrowed_val)); // 끝에 추가

        // 현재 페이지 재구성 (reset_entries로 슬롯 공간 완전 초기화)
        let _ = self.btree_page.reset_entries();
        for (k, v) in &entries {
            let data = serialize_leaf_entry(*k, *v);
            let _ = self.btree_page.insert_entry(&data);
        }

        // 오른쪽 형제 재구성 (첫 번째 항목 제거)
        let mut right_entries: Vec<(u64, RecordId)> = Vec::with_capacity(right_size as usize - 1);
        for i in 1..right_size as usize {
            right_entries.push((right_sibling.key_at(i), right_sibling.value_at(i)));
        }

        let _ = right_sibling.btree_page.reset_entries();
        for (k, v) in &right_entries {
            let data = serialize_leaf_entry(*k, *v);
            let _ = right_sibling.btree_page.insert_entry(&data);
        }

        Some(right_entries.first().map(|(k, _)| *k).unwrap_or(0)) // 새로운 분리 키
    }

    /// 오른쪽 형제와 병합 (현재 페이지에 오른쪽 형제의 모든 항목 추가)
    pub fn merge_with_right(&mut self, right_sibling: &LeafPage) {
        let my_size = self.get_size() as usize;
        let right_size = right_sibling.get_size() as usize;

        // 모든 항목 수집 (현재 페이지 + 오른쪽 형제)
        let mut entries: Vec<(u64, RecordId)> = Vec::with_capacity(my_size + right_size);
        for i in 0..my_size {
            entries.push((self.key_at(i), self.value_at(i)));
        }
        for i in 0..right_size {
            entries.push((right_sibling.key_at(i), right_sibling.value_at(i)));
        }

        // 현재 페이지 재구성 (reset_entries로 슬롯 공간 완전 초기화)
        let _ = self.btree_page.reset_entries();
        for (k, v) in &entries {
            let data = serialize_leaf_entry(*k, *v);
            let _ = self.btree_page.insert_entry(&data);
        }

        // 링크 업데이트: 오른쪽 형제의 next를 가져옴
        self.set_next_page_id(right_sibling.get_next_page_id());
    }

    /// 페이지 분할: 데이터의 절반을 recipient로 이동
    pub fn split(&mut self, recipient: &mut LeafPage) {
        let size = self.get_size() as usize;
        let split_idx = size / 2;

        // 1. recipient에 후반부 복사
        for i in split_idx..size {
            let k = self.key_at(i);
            let v = self.value_at(i);
            let data = serialize_leaf_entry(k, v);
            let _ = recipient.btree_page.insert_entry(&data);
        }

        // 2. 현재 페이지에서 후반부 삭제 (역순)
        for i in (split_idx..size).rev() {
            let _ = self.btree_page.delete_entry(i);
        }
        // 삭제된 슬롯 정리 (슬롯 테이블 단편화 방지)
        let _ = self.btree_page.compact_entries();

        // 3. 링크 연결 (Linked List)
        recipient.set_next_page_id(self.get_next_page_id());
        if let Some(recipient_id) = recipient.btree_page.page().id {
            self.set_next_page_id(recipient_id);
        }

        // 4. 부모 ID 복사
        recipient.set_parent_page_id(self.get_parent_page_id());
    }

    /// 키 삭제
    pub fn delete(&mut self, key: u64) -> bool {
        if let Some(index) = self.binary_search(key) {
            // 해당 인덱스의 엔트리 삭제
            if self.btree_page.delete_entry(index).is_ok() {
                // 삭제된 슬롯 정리 (슬롯 테이블 단편화 방지)
                let _ = self.btree_page.compact_entries();
                return true;
            }
        }
        false
    }

    /// 키 삭제 + WAL 로깅
    pub fn delete_with_wal(
        &mut self,
        key: u64,
        page_wal: &PageWAL,
        tx_id: TransactionId,
        storage_type: PageStorageType,
    ) -> Result<LSN> {
        if let Some(index) = self.binary_search(key) {
            let lsn =
                self.btree_page
                    .delete_entry_with_wal(index, page_wal, tx_id, storage_type)?;
            // 삭제된 슬롯 정리 (슬롯 테이블 단편화 방지)
            let _ = self.btree_page.compact_entries();
            return Ok(lsn);
        }
        Err(crate::error::GraphError::Storage {
            message: format!("Key {} not found", key),
        })
    }

    /// 페이지 참조
    pub fn page(&self) -> &Page {
        self.btree_page.page()
    }

    /// 페이지 mutable 참조
    pub fn page_mut(&mut self) -> &mut Page {
        self.btree_page.page_mut()
    }
}
