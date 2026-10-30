//! B+ Tree 재조정 연산
//!
//! Underflow 시 형제로부터 빌려오기, 병합, 루트 축소 등을 처리합니다.

use super::index::BPlusTree;
use super::internal_page::InternalPage;
use super::leaf_page::LeafPage;
use super::types::{INVALID_PAGE_ID, IndexPageType};
use crate::error::{GraphError, Result};
use crate::storage::disk::page::PageId;
use async_recursion::async_recursion;

impl BPlusTree {
    /// Leaf 페이지 재조정 (Underflow 처리)
    pub(super) async fn rebalance_leaf(
        &mut self,
        leaf_page_id: PageId,
        parent_page_id: PageId,
    ) -> Result<()> {
        // 1. 부모 페이지에서 형제 찾기
        let (left_sibling_id, right_sibling_id, leaf_index_in_parent) = {
            let parent_guard = self.buffer_pool_manager.fetch_page(parent_page_id).await?;
            let mut parent_page = parent_guard.lock().await;
            let internal = InternalPage::new(&mut parent_page);

            let idx =
                internal
                    .find_child_index(leaf_page_id)
                    .ok_or_else(|| GraphError::Storage {
                        message: "Child not found in parent".to_string(),
                    })?;

            let left_id = if idx > 0 {
                Some(internal.value_at(idx - 1))
            } else {
                None
            };
            let right_id = if idx + 1 < internal.get_size() as usize {
                Some(internal.value_at(idx + 1))
            } else {
                None
            };

            drop(parent_page);
            self.buffer_pool_manager
                .unpin_page(parent_page_id, false)
                .await?;

            (left_id, right_id, idx)
        };

        // 2. 왼쪽 형제에서 빌려오기 시도
        if let Some(left_id) = left_sibling_id
            && self
                .try_borrow_from_left_leaf(
                    leaf_page_id,
                    left_id,
                    parent_page_id,
                    leaf_index_in_parent,
                )
                .await?
        {
            return Ok(());
        }

        // 3. 오른쪽 형제에서 빌려오기 시도
        if let Some(right_id) = right_sibling_id
            && self
                .try_borrow_from_right_leaf(
                    leaf_page_id,
                    right_id,
                    parent_page_id,
                    leaf_index_in_parent,
                )
                .await?
        {
            return Ok(());
        }

        // 4. 빌려올 수 없으면 병합
        if let Some(left_id) = left_sibling_id {
            // 왼쪽 형제와 병합 (왼쪽에 현재 페이지 내용을 합침)
            self.merge_leaves(left_id, leaf_page_id, parent_page_id, leaf_index_in_parent)
                .await?;
        } else if let Some(right_id) = right_sibling_id {
            // 오른쪽 형제와 병합 (현재 페이지에 오른쪽 내용을 합침)
            self.merge_leaves(
                leaf_page_id,
                right_id,
                parent_page_id,
                leaf_index_in_parent + 1,
            )
            .await?;
        }

        Ok(())
    }

    /// 왼쪽 형제 Leaf에서 빌려오기 시도
    async fn try_borrow_from_left_leaf(
        &self,
        leaf_page_id: PageId,
        left_sibling_id: PageId,
        parent_page_id: PageId,
        leaf_index: usize,
    ) -> Result<bool> {
        // Leaf 페이지와 왼쪽 형제 fetch
        let leaf_guard = self.buffer_pool_manager.fetch_page(leaf_page_id).await?;
        let left_guard = self.buffer_pool_manager.fetch_page(left_sibling_id).await?;

        let mut leaf_page = leaf_guard.lock().await;
        let mut left_page = left_guard.lock().await;

        let new_separator_key: Option<u64>;

        {
            let mut leaf = LeafPage::new(&mut leaf_page);
            let mut left_sibling = LeafPage::new(&mut left_page);

            new_separator_key = leaf.borrow_from_left(&mut left_sibling);
        }

        if let Some(new_key) = new_separator_key {
            // 부모의 separator 키 업데이트
            drop(leaf_page);
            drop(left_page);

            self.buffer_pool_manager
                .unpin_page(leaf_page_id, true)
                .await?;
            self.buffer_pool_manager
                .unpin_page(left_sibling_id, true)
                .await?;

            self.update_parent_key(parent_page_id, leaf_index, new_key)
                .await?;
            Ok(true)
        } else {
            drop(leaf_page);
            drop(left_page);
            self.buffer_pool_manager
                .unpin_page(leaf_page_id, false)
                .await?;
            self.buffer_pool_manager
                .unpin_page(left_sibling_id, false)
                .await?;
            Ok(false)
        }
    }

    /// 오른쪽 형제 Leaf에서 빌려오기 시도
    async fn try_borrow_from_right_leaf(
        &self,
        leaf_page_id: PageId,
        right_sibling_id: PageId,
        parent_page_id: PageId,
        leaf_index: usize,
    ) -> Result<bool> {
        let leaf_guard = self.buffer_pool_manager.fetch_page(leaf_page_id).await?;
        let right_guard = self
            .buffer_pool_manager
            .fetch_page(right_sibling_id)
            .await?;

        let mut leaf_page = leaf_guard.lock().await;
        let mut right_page = right_guard.lock().await;

        let new_separator_key: Option<u64>;

        {
            let mut leaf = LeafPage::new(&mut leaf_page);
            let mut right_sibling = LeafPage::new(&mut right_page);

            new_separator_key = leaf.borrow_from_right(&mut right_sibling);
        }

        if let Some(new_key) = new_separator_key {
            drop(leaf_page);
            drop(right_page);

            self.buffer_pool_manager
                .unpin_page(leaf_page_id, true)
                .await?;
            self.buffer_pool_manager
                .unpin_page(right_sibling_id, true)
                .await?;

            // 오른쪽 형제의 separator 키 업데이트 (leaf_index + 1)
            self.update_parent_key(parent_page_id, leaf_index + 1, new_key)
                .await?;
            Ok(true)
        } else {
            drop(leaf_page);
            drop(right_page);
            self.buffer_pool_manager
                .unpin_page(leaf_page_id, false)
                .await?;
            self.buffer_pool_manager
                .unpin_page(right_sibling_id, false)
                .await?;
            Ok(false)
        }
    }

    /// 두 Leaf 페이지 병합
    async fn merge_leaves(
        &mut self,
        left_page_id: PageId,
        right_page_id: PageId,
        parent_page_id: PageId,
        right_index_in_parent: usize,
    ) -> Result<()> {
        // 1. 왼쪽 페이지에 오른쪽 페이지 내용 병합
        {
            let left_guard = self.buffer_pool_manager.fetch_page(left_page_id).await?;
            let right_guard = self.buffer_pool_manager.fetch_page(right_page_id).await?;

            let mut left_page = left_guard.lock().await;
            let mut right_page = right_guard.lock().await;

            {
                let mut left_leaf = LeafPage::new(&mut left_page);
                let right_leaf = LeafPage::new(&mut right_page);
                left_leaf.merge_with_right(&right_leaf);
            }

            drop(left_page);
            drop(right_page);

            self.buffer_pool_manager
                .unpin_page(left_page_id, true)
                .await?;
            self.buffer_pool_manager
                .unpin_page(right_page_id, true)
                .await?;
        }

        // 2. 부모에서 오른쪽 페이지 엔트리 제거
        self.remove_entry_from_internal(parent_page_id, right_index_in_parent)
            .await?;

        Ok(())
    }

    /// 부모 Internal 페이지의 키 업데이트
    async fn update_parent_key(
        &self,
        parent_page_id: PageId,
        index: usize,
        new_key: u64,
    ) -> Result<()> {
        let parent_guard = self.buffer_pool_manager.fetch_page(parent_page_id).await?;

        let mut parent_page = parent_guard.lock().await;
        let mut internal = InternalPage::new(&mut parent_page);
        internal.update_key_at(index, new_key);

        drop(parent_page);
        self.buffer_pool_manager
            .unpin_page(parent_page_id, true)
            .await?;

        Ok(())
    }

    /// Internal 페이지에서 엔트리 제거 (재조정 포함)
    #[async_recursion]
    pub(super) async fn remove_entry_from_internal(
        &mut self,
        page_id: PageId,
        index: usize,
    ) -> Result<()> {
        let (need_rebalance, is_root, parent_page_id, remaining_size) = {
            let page_guard = self.buffer_pool_manager.fetch_page(page_id).await?;

            let mut page = page_guard.lock().await;
            let mut internal = InternalPage::new(&mut page);

            internal.remove_at(index);

            let is_root = internal.is_root_page();
            let parent_id = internal.get_parent_page_id();
            let size = internal.get_size() as usize;
            let min_size = internal.get_min_size();
            let need_rebalance = !is_root && size < min_size;

            drop(page);
            self.buffer_pool_manager.unpin_page(page_id, true).await?;

            (need_rebalance, is_root, parent_id, size)
        };

        // 루트가 비어있으면 처리
        if is_root && remaining_size == 1 {
            // 루트에 자식이 하나만 남으면, 그 자식을 새 루트로
            self.collapse_root(page_id).await?;
            return Ok(());
        }

        // Underflow 처리
        if need_rebalance {
            self.rebalance_internal(page_id, parent_page_id).await?;
        }

        Ok(())
    }

    /// 루트가 자식 하나만 가지면 축소
    async fn collapse_root(&mut self, root_page_id: PageId) -> Result<()> {
        let new_root_id = {
            let root_guard = self.buffer_pool_manager.fetch_page(root_page_id).await?;

            let mut root_page = root_guard.lock().await;
            let internal = InternalPage::new(&mut root_page);

            // 유일한 자식 가져오기
            let child_id = internal.value_at(0);

            drop(root_page);
            self.buffer_pool_manager
                .unpin_page(root_page_id, false)
                .await?;

            child_id
        };

        // 새 루트 설정
        {
            let new_root_guard = self.buffer_pool_manager.fetch_page(new_root_id).await?;

            let mut new_root_page = new_root_guard.lock().await;
            let page_type = Self::get_page_type_from_page(&new_root_page);

            match page_type {
                IndexPageType::LeafPage => {
                    let mut leaf = LeafPage::new(&mut new_root_page);
                    leaf.set_root_page(true);
                    leaf.set_parent_page_id(INVALID_PAGE_ID);
                }
                IndexPageType::InternalPage => {
                    let mut internal = InternalPage::new(&mut new_root_page);
                    internal.set_root_page(true);
                    internal.set_parent_page_id(INVALID_PAGE_ID);
                }
            }

            drop(new_root_page);
            self.buffer_pool_manager
                .unpin_page(new_root_id, true)
                .await?;
        }

        self.root_page_id = new_root_id;
        self.persist_metadata().await?;

        Ok(())
    }

    /// Internal 페이지 재조정
    #[async_recursion]
    async fn rebalance_internal(&mut self, page_id: PageId, parent_page_id: PageId) -> Result<()> {
        // 부모에서 형제 찾기
        let (left_sibling_id, right_sibling_id, index_in_parent) = {
            let parent_guard = self.buffer_pool_manager.fetch_page(parent_page_id).await?;
            let mut parent_page = parent_guard.lock().await;
            let internal = InternalPage::new(&mut parent_page);

            let idx = internal
                .find_child_index(page_id)
                .ok_or_else(|| GraphError::Storage {
                    message: "Child not found in parent".to_string(),
                })?;

            let left_id = if idx > 0 {
                Some(internal.value_at(idx - 1))
            } else {
                None
            };
            let right_id = if idx + 1 < internal.get_size() as usize {
                Some(internal.value_at(idx + 1))
            } else {
                None
            };

            drop(parent_page);
            self.buffer_pool_manager
                .unpin_page(parent_page_id, false)
                .await?;

            (left_id, right_id, idx)
        };

        // 1. 왼쪽 형제에서 빌려오기 시도
        if let Some(left_id) = left_sibling_id
            && self
                .try_borrow_from_left_internal(page_id, left_id, parent_page_id, index_in_parent)
                .await?
        {
            return Ok(());
        }

        // 2. 오른쪽 형제에서 빌려오기 시도
        if let Some(right_id) = right_sibling_id
            && self
                .try_borrow_from_right_internal(page_id, right_id, parent_page_id, index_in_parent)
                .await?
        {
            return Ok(());
        }

        // 3. 빌려올 수 없으면 병합
        if let Some(left_id) = left_sibling_id {
            self.merge_internal_pages(left_id, page_id, parent_page_id, index_in_parent)
                .await?;
        } else if let Some(right_id) = right_sibling_id {
            self.merge_internal_pages(page_id, right_id, parent_page_id, index_in_parent + 1)
                .await?;
        }

        Ok(())
    }

    /// 왼쪽 형제 Internal에서 빌려오기 시도
    async fn try_borrow_from_left_internal(
        &self,
        page_id: PageId,
        left_sibling_id: PageId,
        parent_page_id: PageId,
        index_in_parent: usize,
    ) -> Result<bool> {
        // 부모에서 separator 키 가져오기
        let parent_separator = {
            let parent_guard = self.buffer_pool_manager.fetch_page(parent_page_id).await?;
            let mut parent_page = parent_guard.lock().await;
            let internal = InternalPage::new(&mut parent_page);
            let sep = internal.key_at(index_in_parent);
            drop(parent_page);
            self.buffer_pool_manager
                .unpin_page(parent_page_id, false)
                .await?;
            sep
        };

        // Current 페이지와 왼쪽 형제 fetch
        let page_guard = self.buffer_pool_manager.fetch_page(page_id).await?;
        let left_guard = self.buffer_pool_manager.fetch_page(left_sibling_id).await?;

        let mut current_page = page_guard.lock().await;
        let mut left_page = left_guard.lock().await;

        let borrow_result: Option<(u64, u32)>;

        {
            let mut current_internal = InternalPage::new(&mut current_page);
            let mut left_internal = InternalPage::new(&mut left_page);

            borrow_result =
                current_internal.borrow_from_left_internal(&mut left_internal, parent_separator);
        }

        if let Some((new_separator_key, moved_child_id)) = borrow_result {
            drop(current_page);
            drop(left_page);

            self.buffer_pool_manager.unpin_page(page_id, true).await?;
            self.buffer_pool_manager
                .unpin_page(left_sibling_id, true)
                .await?;

            // 부모의 separator 키 업데이트
            self.update_parent_key(parent_page_id, index_in_parent, new_separator_key)
                .await?;

            // 이동된 자식의 parent_id 업데이트
            self.update_child_parent(moved_child_id, page_id).await?;

            Ok(true)
        } else {
            drop(current_page);
            drop(left_page);
            self.buffer_pool_manager.unpin_page(page_id, false).await?;
            self.buffer_pool_manager
                .unpin_page(left_sibling_id, false)
                .await?;
            Ok(false)
        }
    }

    /// 오른쪽 형제 Internal에서 빌려오기 시도
    async fn try_borrow_from_right_internal(
        &self,
        page_id: PageId,
        right_sibling_id: PageId,
        parent_page_id: PageId,
        index_in_parent: usize,
    ) -> Result<bool> {
        // 부모에서 separator 키 가져오기 (오른쪽 형제의 인덱스)
        let parent_separator = {
            let parent_guard = self.buffer_pool_manager.fetch_page(parent_page_id).await?;
            let mut parent_page = parent_guard.lock().await;
            let internal = InternalPage::new(&mut parent_page);
            let sep = internal.key_at(index_in_parent + 1);
            drop(parent_page);
            self.buffer_pool_manager
                .unpin_page(parent_page_id, false)
                .await?;
            sep
        };

        // Current 페이지와 오른쪽 형제 fetch
        let page_guard = self.buffer_pool_manager.fetch_page(page_id).await?;
        let right_guard = self
            .buffer_pool_manager
            .fetch_page(right_sibling_id)
            .await?;

        let mut current_page = page_guard.lock().await;
        let mut right_page = right_guard.lock().await;

        let borrow_result: Option<(u64, u32)>;

        {
            let mut current_internal = InternalPage::new(&mut current_page);
            let mut right_internal = InternalPage::new(&mut right_page);

            borrow_result =
                current_internal.borrow_from_right_internal(&mut right_internal, parent_separator);
        }

        if let Some((new_separator_key, moved_child_id)) = borrow_result {
            drop(current_page);
            drop(right_page);

            self.buffer_pool_manager.unpin_page(page_id, true).await?;
            self.buffer_pool_manager
                .unpin_page(right_sibling_id, true)
                .await?;

            // 오른쪽 형제의 separator 키 업데이트
            self.update_parent_key(parent_page_id, index_in_parent + 1, new_separator_key)
                .await?;

            // 이동된 자식의 parent_id 업데이트
            self.update_child_parent(moved_child_id, page_id).await?;

            Ok(true)
        } else {
            drop(current_page);
            drop(right_page);
            self.buffer_pool_manager.unpin_page(page_id, false).await?;
            self.buffer_pool_manager
                .unpin_page(right_sibling_id, false)
                .await?;
            Ok(false)
        }
    }

    /// 두 Internal 페이지 병합
    #[async_recursion]
    async fn merge_internal_pages(
        &mut self,
        left_page_id: PageId,
        right_page_id: PageId,
        parent_page_id: PageId,
        right_index_in_parent: usize,
    ) -> Result<()> {
        // 부모에서 separator 키 가져오기
        let separator_key = {
            let parent_guard = self.buffer_pool_manager.fetch_page(parent_page_id).await?;
            let mut parent_page = parent_guard.lock().await;
            let internal = InternalPage::new(&mut parent_page);
            let key = internal.key_at(right_index_in_parent);
            drop(parent_page);
            self.buffer_pool_manager
                .unpin_page(parent_page_id, false)
                .await?;
            key
        };

        // 왼쪽에 separator와 오른쪽 내용 병합
        let children: Vec<u32>;
        {
            let left_guard = self.buffer_pool_manager.fetch_page(left_page_id).await?;
            let right_guard = self.buffer_pool_manager.fetch_page(right_page_id).await?;

            let mut left_page = left_guard.lock().await;
            let mut right_page = right_guard.lock().await;

            // 병합 전 크기 검사
            let (left_size, right_size, max_size) = {
                let left_internal = InternalPage::new(&mut left_page);
                let right_internal = InternalPage::new(&mut right_page);
                (
                    left_internal.get_size() as usize,
                    right_internal.get_size() as usize,
                    left_internal.get_max_size(),
                )
            };

            if left_size + right_size > max_size {
                // 병합 불가능: 페이지 unpin 후 리턴 (underflow 상태 유지)
                drop(left_page);
                drop(right_page);
                self.buffer_pool_manager
                    .unpin_page(left_page_id, false)
                    .await?;
                self.buffer_pool_manager
                    .unpin_page(right_page_id, false)
                    .await?;
                return Ok(());
            }

            // 모든 엔트리 수집 (왼쪽 + separator + 오른쪽)
            let mut entries: Vec<(u64, u32)> = Vec::with_capacity(left_size + right_size);
            let mut children_vec: Vec<u32> = Vec::with_capacity(right_size);

            {
                let left_internal = InternalPage::new(&mut left_page);
                let right_internal = InternalPage::new(&mut right_page);

                // 왼쪽 엔트리
                for i in 0..left_size {
                    entries.push((left_internal.key_at(i), left_internal.value_at(i)));
                }

                // separator 키와 오른쪽의 첫 번째 자식
                entries.push((separator_key, right_internal.value_at(0)));

                // 오른쪽의 나머지 엔트리
                for i in 1..right_size {
                    entries.push((right_internal.key_at(i), right_internal.value_at(i)));
                }

                // 이동된 자식들 수집
                for i in 0..right_size {
                    children_vec.push(right_internal.value_at(i));
                }
            }

            children = children_vec;

            // 왼쪽 페이지 재구성 (reset_entries로 슬롯 공간 완전 초기화)
            {
                use super::slotted::{BTreeSlottedPage, serialize_internal_entry};
                let mut btree_page = BTreeSlottedPage::new(&mut left_page);
                let _ = btree_page.reset_entries();
                for (k, v) in &entries {
                    let data = serialize_internal_entry(*k, *v);
                    let _ = btree_page.insert_entry(&data);
                }
            }

            drop(left_page);
            drop(right_page);

            self.buffer_pool_manager
                .unpin_page(left_page_id, true)
                .await?;
            self.buffer_pool_manager
                .unpin_page(right_page_id, true)
                .await?;
        }

        // 이동된 자식들의 parent 업데이트
        for child_id in children {
            self.update_child_parent(child_id, left_page_id).await?;
        }

        // 부모에서 오른쪽 엔트리 제거
        self.remove_entry_from_internal(parent_page_id, right_index_in_parent)
            .await?;

        Ok(())
    }
}
