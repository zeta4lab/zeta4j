//! B+ Tree 삽입 연산
//!
//! 키-값 삽입 및 페이지 분할 기능을 제공합니다.

use super::index::BPlusTree;
use super::internal_page::InternalPage;
use super::leaf_page::LeafPage;
use super::types::{INVALID_PAGE_ID, IndexPageType, RecordId};
use crate::error::{GraphError, Result};
use crate::storage::disk::page::PageId;
use async_recursion::async_recursion;

impl BPlusTree {
    /// 값 삽입 (분할 지원)
    pub async fn insert(&mut self, key: u64, value: RecordId) -> Result<bool> {
        let leaf_page_guard = self.find_leaf_page(key, true).await?;
        let mut page = leaf_page_guard.lock().await;
        let leaf_page_id = page.id.ok_or_else(|| GraphError::Storage {
            message: "Page has no ID".to_string(),
        })?;

        {
            let mut leaf = LeafPage::new(&mut page);

            // 공간이 있으면 바로 삽입
            if (leaf.get_size() as usize) < leaf.get_max_size() {
                let success = leaf.insert(key, value);
                drop(page);
                self.buffer_pool_manager
                    .unpin_page(leaf_page_id, true)
                    .await?;
                return Ok(success);
            }
        }

        // 페이지가 꽉 찼으므로 분할 필요
        drop(page);
        self.buffer_pool_manager
            .unpin_page(leaf_page_id, false)
            .await?;

        self.insert_with_split(key, value, leaf_page_id).await
    }

    /// 값 삽입 또는 업데이트 (upsert)
    pub async fn upsert(&mut self, key: u64, value: RecordId) -> Result<bool> {
        let leaf_page_guard = self.find_leaf_page(key, true).await?;
        let mut page = leaf_page_guard.lock().await;
        let leaf_page_id = page.id.ok_or_else(|| GraphError::Storage {
            message: "Page has no ID".to_string(),
        })?;

        {
            let mut leaf = LeafPage::new(&mut page);

            // insert_or_update 사용 (기존 키면 업데이트, 새 키면 삽입)
            if (leaf.get_size() as usize) < leaf.get_max_size() || leaf.binary_search(key).is_some()
            {
                let success = leaf.insert_or_update(key, value);
                drop(page);
                self.buffer_pool_manager
                    .unpin_page(leaf_page_id, true)
                    .await?;
                return Ok(success);
            }
        }

        // 페이지가 꽉 찼고 새 키이므로 분할 필요
        drop(page);
        self.buffer_pool_manager
            .unpin_page(leaf_page_id, false)
            .await?;

        self.insert_with_split(key, value, leaf_page_id).await
    }

    /// 분할을 수행하면서 삽입
    async fn insert_with_split(
        &mut self,
        key: u64,
        value: RecordId,
        leaf_page_id: PageId,
    ) -> Result<bool> {
        // 1. 새 페이지 할당
        let new_page_guard = self.buffer_pool_manager.new_page().await?;
        let new_page_id = new_page_guard
            .lock()
            .await
            .id
            .ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

        // 2. 기존 Leaf 페이지 다시 fetch
        let old_page_guard = self.buffer_pool_manager.fetch_page(leaf_page_id).await?;

        let mut old_page = old_page_guard.lock().await;
        let mut new_page = new_page_guard.lock().await;

        // 3. 새 페이지 초기화 및 분할
        {
            let mut old_leaf = LeafPage::new(&mut old_page);
            let mut new_leaf = LeafPage::new(&mut new_page);
            new_leaf.init(old_leaf.get_parent_page_id());

            old_leaf.split(&mut new_leaf);

            // 4. 키 위치에 따라 적절한 페이지에 삽입
            let split_key = new_leaf.key_at(0);
            if key < split_key {
                old_leaf.insert(key, value);
            } else {
                new_leaf.insert(key, value);
            }
        }

        // 5. 부모에 새 키 삽입 (루트 분할 처리 포함)
        let old_leaf = LeafPage::new(&mut old_page);
        let new_leaf = LeafPage::new(&mut new_page);
        let split_key = new_leaf.key_at(0);
        let parent_page_id = old_leaf.get_parent_page_id();
        let is_root = old_leaf.is_root_page();

        drop(old_page);
        drop(new_page);

        if is_root {
            // 루트 분할: 새 루트 생성
            self.create_new_root(leaf_page_id, split_key, new_page_id)
                .await?;
        } else {
            // 부모에 새 키 삽입
            self.insert_into_parent(parent_page_id, leaf_page_id, split_key, new_page_id)
                .await?;
        }

        // Unpin pages
        self.buffer_pool_manager
            .unpin_page(leaf_page_id, true)
            .await?;
        self.buffer_pool_manager
            .unpin_page(new_page_id, true)
            .await?;

        Ok(true)
    }

    /// 새 루트 페이지 생성 (루트 분할 시)
    pub(super) async fn create_new_root(
        &mut self,
        left_page_id: PageId,
        key: u64,
        right_page_id: PageId,
    ) -> Result<()> {
        let new_root_guard = self.buffer_pool_manager.new_page().await?;
        let new_root_id = new_root_guard
            .lock()
            .await
            .id
            .ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

        // 새 루트 초기화 (Internal Page)
        {
            let mut new_root_page = new_root_guard.lock().await;
            let mut internal = InternalPage::new(&mut new_root_page);
            internal.init(INVALID_PAGE_ID);
            internal.set_root_page(true);

            // 첫 번째 엔트리: 왼쪽 자식 (insert_at 사용)
            internal.insert_at(0, 0, left_page_id); // key=0은 dummy
            // 두 번째 엔트리: 분할 키와 오른쪽 자식
            internal.insert_at(1, key, right_page_id);
        }

        // 기존 루트(왼쪽 자식)의 루트 플래그 해제 및 부모 설정
        {
            let left_page_guard = self.buffer_pool_manager.fetch_page(left_page_id).await?;
            let mut left_page = left_page_guard.lock().await;
            let page_type = Self::get_page_type_from_page(&left_page);

            match page_type {
                IndexPageType::LeafPage => {
                    let mut leaf = LeafPage::new(&mut left_page);
                    leaf.set_root_page(false);
                    leaf.set_parent_page_id(new_root_id);
                }
                IndexPageType::InternalPage => {
                    let mut internal = InternalPage::new(&mut left_page);
                    internal.set_root_page(false);
                    internal.set_parent_page_id(new_root_id);
                }
            }
            drop(left_page);
            self.buffer_pool_manager
                .unpin_page(left_page_id, true)
                .await?;
        }

        // 오른쪽 자식의 부모 설정
        {
            let right_page_guard = self.buffer_pool_manager.fetch_page(right_page_id).await?;
            let mut right_page = right_page_guard.lock().await;
            let page_type = Self::get_page_type_from_page(&right_page);

            match page_type {
                IndexPageType::LeafPage => {
                    let mut leaf = LeafPage::new(&mut right_page);
                    leaf.set_parent_page_id(new_root_id);
                }
                IndexPageType::InternalPage => {
                    let mut internal = InternalPage::new(&mut right_page);
                    internal.set_parent_page_id(new_root_id);
                }
            }
            drop(right_page);
            self.buffer_pool_manager
                .unpin_page(right_page_id, true)
                .await?;
        }

        // 루트 ID 업데이트
        self.root_page_id = new_root_id;

        self.buffer_pool_manager
            .unpin_page(new_root_id, true)
            .await?;

        // 메타데이터에 새 루트 ID 저장
        self.persist_metadata().await?;

        Ok(())
    }

    /// 부모 Internal 페이지에 새 키 삽입
    #[async_recursion]
    async fn insert_into_parent(
        &mut self,
        parent_page_id: PageId,
        old_page_id: PageId,
        key: u64,
        new_page_id: PageId,
    ) -> Result<()> {
        let parent_guard = self.buffer_pool_manager.fetch_page(parent_page_id).await?;

        let mut parent_page = parent_guard.lock().await;
        let mut internal = InternalPage::new(&mut parent_page);

        let success = internal.insert_node_after(old_page_id, key, new_page_id);

        if success {
            drop(parent_page);
            self.buffer_pool_manager
                .unpin_page(parent_page_id, true)
                .await?;
            Ok(())
        } else {
            // Internal 페이지도 꽉 참 -> Internal 분할 필요
            let grandparent_page_id = internal.get_parent_page_id();
            let is_root = internal.is_root_page();

            drop(parent_page);
            self.buffer_pool_manager
                .unpin_page(parent_page_id, false)
                .await?;

            self.split_internal_page(
                parent_page_id,
                grandparent_page_id,
                is_root,
                old_page_id,
                key,
                new_page_id,
            )
            .await
        }
    }

    /// Internal 페이지 분할
    #[async_recursion]
    async fn split_internal_page(
        &mut self,
        page_id: PageId,
        grandparent_page_id: PageId,
        is_root: bool,
        old_child_id: PageId,
        key: u64,
        new_child_id: PageId,
    ) -> Result<()> {
        // 1. 새 페이지 할당
        let new_page_guard = self.buffer_pool_manager.new_page().await?;
        let new_page_id = new_page_guard
            .lock()
            .await
            .id
            .ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

        // 2. 기존 페이지 fetch
        let old_page_guard = self.buffer_pool_manager.fetch_page(page_id).await?;

        let promoted_key: u64;
        let entries_to_move: Vec<(u64, u32)>;

        {
            let mut old_page = old_page_guard.lock().await;
            let mut new_page = new_page_guard.lock().await;

            let mut old_internal = InternalPage::new(&mut old_page);
            let mut new_internal = InternalPage::new(&mut new_page);

            // 3. 새 페이지 초기화
            new_internal.init(grandparent_page_id);

            // 4. 먼저 새 키를 삽입할 위치를 찾아서 임시 버퍼에 모든 엔트리 수집
            let size = old_internal.get_size() as usize;
            let mut all_entries: Vec<(u64, u32)> = Vec::with_capacity(size + 1);

            // 기존 엔트리 수집 및 새 엔트리 삽입 위치 결정
            let mut inserted = false;
            for i in 0..size {
                let k = old_internal.key_at(i);
                let v = old_internal.value_at(i);

                // old_child_id 뒤에 새 엔트리 삽입
                if !inserted && v == old_child_id {
                    all_entries.push((k, v));
                    all_entries.push((key, new_child_id));
                    inserted = true;
                } else {
                    all_entries.push((k, v));
                }
            }
            // 끝에 삽입해야 하는 경우
            if !inserted {
                all_entries.push((key, new_child_id));
            }

            // 5. 분할 지점 계산
            let total = all_entries.len();
            let mid = total / 2;

            // 중간 키가 부모로 승격됨
            promoted_key = all_entries[mid].0;

            // 6. 기존 페이지의 모든 엔트리 삭제 (역순으로)
            let old_size = old_internal.get_size() as usize;
            for i in (0..old_size).rev() {
                old_internal.remove_at(i);
            }

            // 7. 왼쪽 절반은 기존 페이지에 (insert_at 사용)
            for (i, (k, v)) in all_entries[0..mid].iter().enumerate() {
                old_internal.insert_at(i, *k, *v);
            }

            // 8. 오른쪽 절반은 새 페이지에 (mid부터 - 승격 키의 자식 포인터 포함)
            entries_to_move = all_entries[mid..].to_vec();
            for (i, (k, v)) in entries_to_move.iter().enumerate() {
                new_internal.insert_at(i, *k, *v);
            }
        }

        // 8. 새 페이지로 이동한 자식들의 parent 포인터 업데이트
        for (_k, child_id) in &entries_to_move {
            self.update_child_parent(*child_id, new_page_id).await?;
        }

        // 9. unpin 페이지들
        self.buffer_pool_manager.unpin_page(page_id, true).await?;
        self.buffer_pool_manager
            .unpin_page(new_page_id, true)
            .await?;

        // 10. 부모에 승격된 키 삽입
        if is_root {
            // 루트 분할: 새 루트 생성
            self.create_new_root(page_id, promoted_key, new_page_id)
                .await?;
        } else {
            // 부모에 재귀적으로 삽입
            self.insert_into_parent(grandparent_page_id, page_id, promoted_key, new_page_id)
                .await?;
        }

        Ok(())
    }

    /// 자식 페이지의 parent 포인터 업데이트
    pub(super) async fn update_child_parent(
        &self,
        child_page_id: PageId,
        new_parent_id: PageId,
    ) -> Result<()> {
        let child_guard = self.buffer_pool_manager.fetch_page(child_page_id).await?;

        let mut child_page = child_guard.lock().await;
        let page_type = Self::get_page_type_from_page(&child_page);

        match page_type {
            IndexPageType::LeafPage => {
                let mut leaf = LeafPage::new(&mut child_page);
                leaf.set_parent_page_id(new_parent_id);
            }
            IndexPageType::InternalPage => {
                let mut internal = InternalPage::new(&mut child_page);
                internal.set_parent_page_id(new_parent_id);
            }
        }

        drop(child_page);
        self.buffer_pool_manager
            .unpin_page(child_page_id, true)
            .await?;

        Ok(())
    }
}
