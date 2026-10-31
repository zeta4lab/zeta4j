// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Concurrent B+ Tree (Crabbing Protocol 적용)
//!
//! Lock Coupling (Crabbing) 기법을 사용하여 동시성을 극대화합니다.
//!
//! # 동작 방식
//!
//! ## 읽기 연산
//! 1. 루트에서 읽기 래치 획득
//! 2. 자식으로 내려가며 자식 읽기 래치 획득
//! 3. 부모 래치 즉시 해제 (읽기는 항상 safe)
//!
//! ## 쓰기 연산 (삽입/삭제)
//! 1. 루트에서 쓰기 래치 획득
//! 2. 자식으로 내려가며 자식 쓰기 래치 획득
//! 3. 자식이 "safe"하면 모든 조상 래치 해제
//! 4. safe = 분할/병합이 발생하지 않을 조건
//!
//! # 성능 특성
//!
//! - 읽기-읽기: 완전 동시 (래치 없이 진행)
//! - 읽기-쓰기: 다른 서브트리면 동시 가능
//! - 쓰기-쓰기: 다른 리프면 동시 가능

use super::internal_page::InternalPage;
use super::latch::{CrabbingContext, LatchManager, LatchMode, SafetyChecker};
use super::leaf_page::LeafPage;
use super::metadata::BTreeMetadata;
use super::types::{INVALID_PAGE_ID, IndexPageType, RecordId};
use crate::error::{GraphError, Result};
use crate::storage::cache::BufferPoolManager;
use crate::storage::disk::page::{PageId, read_u16_le, read_u32_le};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 형제 노드에서 빌려오는 방향
#[derive(Debug, Clone, Copy)]
enum BorrowDirection {
    Left,
    Right,
}

/// Concurrent B+ Tree
///
/// Crabbing Protocol을 사용하는 동시성 B+Tree
///
/// BufferPoolManager는 내부 가변성 패턴을 사용하므로
/// 외부에서 RwLock으로 감싸지 않아도 됨
pub struct ConcurrentBPlusTree {
    /// Buffer Pool Manager (내부 가변성 사용)
    buffer_pool_manager: Arc<BufferPoolManager>,
    /// 루트 페이지 ID
    root_page_id: RwLock<PageId>,
    /// 래치 관리자
    latch_manager: Arc<LatchManager>,
    /// 메타데이터 존재 여부
    has_metadata: bool,
}

impl ConcurrentBPlusTree {
    /// 새 Concurrent B+Tree 생성
    pub fn new(bpm: Arc<BufferPoolManager>, root_page_id: PageId, has_metadata: bool) -> Self {
        Self {
            buffer_pool_manager: bpm,
            root_page_id: RwLock::new(root_page_id),
            latch_manager: Arc::new(LatchManager::new()),
            has_metadata,
        }
    }

    /// 새 트리 생성 (테스트용, 메타데이터 없음)
    pub async fn create_new_tree(bpm: &Arc<BufferPoolManager>) -> Result<Self> {
        let root_id = {
            let root_page = bpm.new_page().await?;
            let root_id = root_page
                .lock()
                .await
                .id
                .ok_or_else(|| GraphError::Storage {
                    message: "Page has no ID".to_string(),
                })?;

            // Root는 처음에 Leaf
            {
                let mut page_guard = root_page.lock().await;
                let mut leaf = LeafPage::new(&mut page_guard);
                leaf.init(INVALID_PAGE_ID);
                leaf.set_root_page(true);
            }

            bpm.unpin_page(root_id, true).await?;
            root_id
        };

        Ok(Self {
            buffer_pool_manager: bpm.clone(),
            root_page_id: RwLock::new(root_id),
            latch_manager: Arc::new(LatchManager::new()),
            has_metadata: false,
        })
    }

    /// 메타데이터 포함 새 트리 생성
    pub async fn create_new_tree_with_metadata(bpm: &Arc<BufferPoolManager>) -> Result<Self> {
        // 1. 메타데이터 페이지 생성 (Page 0)
        let meta_page_guard = bpm.new_page().await?;
        let meta_page_id = meta_page_guard
            .lock()
            .await
            .id
            .ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

        if meta_page_id != BTreeMetadata::METADATA_PAGE_ID {
            return Err(GraphError::Storage {
                message: format!(
                    "Expected metadata page ID {}, got {}",
                    BTreeMetadata::METADATA_PAGE_ID,
                    meta_page_id
                ),
            });
        }

        // 2. 루트 Leaf 페이지 생성 (Page 1)
        let root_page_guard = bpm.new_page().await?;
        let root_page_id = root_page_guard
            .lock()
            .await
            .id
            .ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

        // 3. 루트 페이지 초기화
        {
            let mut root_page = root_page_guard.lock().await;
            let mut leaf = LeafPage::new(&mut root_page);
            leaf.init(INVALID_PAGE_ID);
            leaf.set_root_page(true);
        }

        // 4. 메타데이터 초기화
        {
            let mut meta_page = meta_page_guard.lock().await;
            let metadata = BTreeMetadata::new(root_page_id);
            metadata.to_page(&mut meta_page);
        }

        // 5. 페이지 unpin
        bpm.unpin_page(meta_page_id, true).await?;
        bpm.unpin_page(root_page_id, true).await?;

        Ok(Self {
            buffer_pool_manager: bpm.clone(),
            root_page_id: RwLock::new(root_page_id),
            latch_manager: Arc::new(LatchManager::new()),
            has_metadata: true,
        })
    }

    /// 디스크에서 복구
    pub async fn recover_from_disk(bpm: Arc<BufferPoolManager>) -> Result<Self> {
        // 메타데이터 페이지 읽기 (Page 0)
        let meta_page_guard = bpm.fetch_page(BTreeMetadata::METADATA_PAGE_ID).await?;
        let meta_page = meta_page_guard.lock().await;

        let metadata = BTreeMetadata::from_page(&meta_page)?;
        metadata.validate()?;

        let root_page_id = metadata.root_page_id;

        drop(meta_page);
        bpm.unpin_page(BTreeMetadata::METADATA_PAGE_ID, false)
            .await?;

        Ok(Self {
            buffer_pool_manager: bpm,
            root_page_id: RwLock::new(root_page_id),
            latch_manager: Arc::new(LatchManager::new()),
            has_metadata: true,
        })
    }

    /// 루트 페이지 ID 반환
    pub async fn root_page_id(&self) -> PageId {
        *self.root_page_id.read().await
    }

    /// 새 페이지 할당 및 ID 반환
    async fn allocate_new_page(&self) -> Result<PageId> {
        let page_guard = self.buffer_pool_manager.new_page().await?;
        let id = page_guard
            .lock()
            .await
            .id
            .ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;
        self.buffer_pool_manager.unpin_page(id, false).await?;
        Ok(id)
    }

    /// 트리가 비어있는지 확인
    pub async fn is_empty(&self) -> Result<bool> {
        let root_id = self.root_page_id().await;
        let mut ctx = CrabbingContext::new(self.latch_manager.clone(), LatchMode::Read);
        ctx.acquire(root_id).await?;

        let is_empty = {
            let root_guard = self.buffer_pool_manager.fetch_page(root_id).await?;
            let mut root_page = root_guard.lock().await;

            let page_type = Self::get_page_type(&root_page.data);
            let result = match page_type {
                IndexPageType::LeafPage => {
                    let leaf = LeafPage::new(&mut root_page);
                    leaf.get_size() == 0
                }
                IndexPageType::InternalPage => false,
            };

            drop(root_page);
            self.buffer_pool_manager.unpin_page(root_id, false).await?;
            result
        };

        Ok(is_empty)
    }

    /// 페이지 타입 확인 (SlottedPage 구조에서 읽기)
    fn get_page_type(data: &[u8]) -> IndexPageType {
        const HEADER_SIZE: usize = 16;

        let slot0_offset = read_u16_le(&data[HEADER_SIZE..HEADER_SIZE + size_of::<u16>()]);

        let page_type_val =
            read_u32_le(&data[slot0_offset as usize..slot0_offset as usize + size_of::<u32>()]);

        if page_type_val == 1 {
            IndexPageType::LeafPage
        } else {
            IndexPageType::InternalPage
        }
    }

    /// 값 조회 (Crabbing 읽기 모드)
    pub async fn get_value(&self, key: u64) -> Result<Option<RecordId>> {
        let root_id = self.root_page_id().await;
        let mut ctx = CrabbingContext::new(self.latch_manager.clone(), LatchMode::Read);

        let result = self.find_and_read_leaf(&mut ctx, root_id, key).await?;

        Ok(result)
    }

    /// Crabbing으로 리프 페이지 찾기 및 값 읽기
    async fn find_and_read_leaf(
        &self,
        ctx: &mut CrabbingContext,
        root_id: PageId,
        key: u64,
    ) -> Result<Option<RecordId>> {
        let mut current_page_id = root_id;
        ctx.acquire(current_page_id).await?;

        loop {
            let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;
            let mut page = page_guard.lock().await;

            let page_type = Self::get_page_type(&page.data);

            match page_type {
                IndexPageType::LeafPage => {
                    let leaf = LeafPage::new(&mut page);
                    let result = leaf.binary_search(key).map(|idx| leaf.value_at(idx));

                    drop(page);
                    self.buffer_pool_manager
                        .unpin_page(current_page_id, false)
                        .await?;

                    return Ok(result);
                }
                IndexPageType::InternalPage => {
                    let internal = InternalPage::new(&mut page);
                    let next_page_id = internal.lookup(key);

                    drop(page);
                    self.buffer_pool_manager
                        .unpin_page(current_page_id, false)
                        .await?;

                    if next_page_id == INVALID_PAGE_ID {
                        return Err(GraphError::Storage {
                            message: format!(
                                "B+Tree crabbing read hit empty internal page at page_id={}",
                                current_page_id
                            ),
                        });
                    }

                    // 읽기는 항상 safe → 부모 래치 즉시 해제
                    ctx.release_ancestors();

                    // 자식 래치 획득
                    ctx.acquire(next_page_id).await?;
                    current_page_id = next_page_id;
                }
            }
        }
    }

    /// 값 삽입 (Crabbing 쓰기 모드)
    pub async fn insert(&self, key: u64, value: RecordId) -> Result<bool> {
        let root_id = self.root_page_id().await;
        let mut ctx = CrabbingContext::new(self.latch_manager.clone(), LatchMode::Write);

        let (leaf_page_id, needs_split) = self.find_leaf_for_insert(&mut ctx, root_id, key).await?;

        if needs_split {
            self.insert_with_split_crabbing(&mut ctx, key, value, leaf_page_id)
                .await
        } else {
            self.insert_into_leaf(leaf_page_id, key, value).await
        }
    }

    /// Crabbing으로 리프 페이지 찾기 (공통 로직)
    async fn find_leaf_for_operation<F>(
        &self,
        ctx: &mut CrabbingContext,
        root_id: PageId,
        key: u64,
        is_safe: F,
    ) -> Result<(PageId, bool)>
    where
        F: Fn(usize, usize) -> bool,
    {
        let mut current_page_id = root_id;
        ctx.acquire(current_page_id).await?;

        loop {
            let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;
            let mut page = page_guard.lock().await;

            let page_type = Self::get_page_type(&page.data);

            match page_type {
                IndexPageType::LeafPage => {
                    let leaf = LeafPage::new(&mut page);
                    let current_size = leaf.get_size() as usize;
                    let max_size = leaf.get_max_size();
                    let safe = is_safe(current_size, max_size);

                    drop(page);
                    self.buffer_pool_manager
                        .unpin_page(current_page_id, false)
                        .await?;

                    if safe {
                        ctx.release_ancestors();
                    }

                    return Ok((current_page_id, !safe));
                }
                IndexPageType::InternalPage => {
                    let internal = InternalPage::new(&mut page);
                    let next_page_id = internal.lookup(key);
                    let current_size = internal.get_size() as usize;
                    let max_size = internal.get_max_size();
                    let safe = is_safe(current_size, max_size);

                    drop(page);
                    self.buffer_pool_manager
                        .unpin_page(current_page_id, false)
                        .await?;

                    if next_page_id == INVALID_PAGE_ID {
                        return Err(GraphError::Storage {
                            message: format!(
                                "B+Tree crabbing write hit empty internal page at page_id={}",
                                current_page_id
                            ),
                        });
                    }

                    if safe {
                        ctx.release_ancestors();
                    }

                    ctx.acquire(next_page_id).await?;
                    current_page_id = next_page_id;
                }
            }
        }
    }

    /// Crabbing으로 삽입용 리프 페이지 찾기
    #[inline]
    async fn find_leaf_for_insert(
        &self,
        ctx: &mut CrabbingContext,
        root_id: PageId,
        key: u64,
    ) -> Result<(PageId, bool)> {
        self.find_leaf_for_operation(ctx, root_id, key, SafetyChecker::is_safe_for_insert)
            .await
    }

    /// 리프 페이지에 단순 삽입
    async fn insert_into_leaf(
        &self,
        leaf_page_id: PageId,
        key: u64,
        value: RecordId,
    ) -> Result<bool> {
        let page_guard = self.buffer_pool_manager.fetch_page(leaf_page_id).await?;
        let mut page = page_guard.lock().await;

        let mut leaf = LeafPage::new(&mut page);
        let success = leaf.insert(key, value);

        drop(page);
        self.buffer_pool_manager
            .unpin_page(leaf_page_id, true)
            .await?;

        Ok(success)
    }

    /// 분할을 수반하는 삽입 (Crabbing)
    async fn insert_with_split_crabbing(
        &self,
        _ctx: &mut CrabbingContext,
        key: u64,
        value: RecordId,
        leaf_page_id: PageId,
    ) -> Result<bool> {
        let new_page_id = self.allocate_new_page().await?;

        let (split_key, parent_page_id, is_root) = {
            let old_page_guard = self.buffer_pool_manager.fetch_page(leaf_page_id).await?;
            let new_page_guard = self.buffer_pool_manager.fetch_page(new_page_id).await?;

            let mut old_page = old_page_guard.lock().await;
            let mut new_page = new_page_guard.lock().await;

            let mut old_leaf = LeafPage::new(&mut old_page);
            let mut new_leaf = LeafPage::new(&mut new_page);
            new_leaf.init(old_leaf.get_parent_page_id());

            old_leaf.split(&mut new_leaf);

            let split_key = new_leaf.key_at(0);
            if key < split_key {
                old_leaf.insert(key, value);
            } else {
                new_leaf.insert(key, value);
            }

            let parent_page_id = old_leaf.get_parent_page_id();
            let is_root = old_leaf.is_root_page();
            let split_key = new_leaf.key_at(0);

            drop(old_page);
            drop(new_page);
            self.buffer_pool_manager
                .unpin_page(leaf_page_id, true)
                .await?;
            self.buffer_pool_manager
                .unpin_page(new_page_id, true)
                .await?;

            (split_key, parent_page_id, is_root)
        };

        if is_root {
            self.create_new_root(leaf_page_id, split_key, new_page_id)
                .await?;
        } else {
            self.insert_into_parent(parent_page_id, leaf_page_id, split_key, new_page_id)
                .await?;
        }

        Ok(true)
    }

    /// 새 루트 페이지 생성
    async fn create_new_root(
        &self,
        left_page_id: PageId,
        key: u64,
        right_page_id: PageId,
    ) -> Result<()> {
        let new_root_id = self.allocate_new_page().await?;

        {
            let new_root_guard = self.buffer_pool_manager.fetch_page(new_root_id).await?;
            let mut new_root_page = new_root_guard.lock().await;

            let mut internal = InternalPage::new(&mut new_root_page);
            internal.init(INVALID_PAGE_ID);
            internal.set_root_page(true);
            internal.insert_at(0, 0, left_page_id);
            internal.insert_at(1, key, right_page_id);

            drop(new_root_page);
            self.buffer_pool_manager
                .unpin_page(new_root_id, true)
                .await?;
        }

        self.update_child_parent(left_page_id, new_root_id, true)
            .await?;
        self.update_child_parent(right_page_id, new_root_id, false)
            .await?;

        *self.root_page_id.write().await = new_root_id;

        self.persist_metadata().await?;

        Ok(())
    }

    /// 부모 페이지에 키 삽입
    #[async_recursion::async_recursion]
    async fn insert_into_parent(
        &self,
        parent_page_id: PageId,
        old_page_id: PageId,
        key: u64,
        new_page_id: PageId,
    ) -> Result<()> {
        let (success, grandparent_page_id, is_root) = {
            let parent_guard = self.buffer_pool_manager.fetch_page(parent_page_id).await?;
            let mut parent_page = parent_guard.lock().await;

            let mut internal = InternalPage::new(&mut parent_page);
            let success = internal.insert_node_after(old_page_id, key, new_page_id);
            let grandparent = internal.get_parent_page_id();
            let is_root = internal.is_root_page();

            drop(parent_page);
            self.buffer_pool_manager
                .unpin_page(parent_page_id, true)
                .await?;

            (success, grandparent, is_root)
        };

        if success {
            Ok(())
        } else {
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
    #[expect(clippy::needless_range_loop)]
    async fn split_internal_page(
        &self,
        page_id: PageId,
        grandparent_page_id: PageId,
        is_root: bool,
        old_child_id: PageId,
        key: u64,
        new_child_id: PageId,
    ) -> Result<()> {
        let new_page_id = self.allocate_new_page().await?;

        let (promoted_key, entries_to_move) = {
            let old_page_guard = self.buffer_pool_manager.fetch_page(page_id).await?;
            let new_page_guard = self.buffer_pool_manager.fetch_page(new_page_id).await?;

            let mut old_page = old_page_guard.lock().await;
            let mut new_page = new_page_guard.lock().await;

            let mut old_internal = InternalPage::new(&mut old_page);
            let mut new_internal = InternalPage::new(&mut new_page);

            new_internal.init(grandparent_page_id);

            let size = old_internal.get_size() as usize;
            let mut all_entries: Vec<(u64, u32)> = Vec::with_capacity(size + 1);

            let mut inserted = false;
            for i in 0..size {
                let k = old_internal.key_at(i);
                let v = old_internal.value_at(i);

                if !inserted && v == old_child_id {
                    all_entries.push((k, v));
                    all_entries.push((key, new_child_id));
                    inserted = true;
                } else {
                    all_entries.push((k, v));
                }
            }
            if !inserted {
                all_entries.push((key, new_child_id));
            }

            let total = all_entries.len();
            let mid = total / 2;
            let promoted_key = all_entries[mid].0;

            let _ = old_internal.reset_entries();
            for i in 0..mid {
                old_internal.insert_at(i, all_entries[i].0, all_entries[i].1);
            }

            let entries_to_move: Vec<(u64, u32)> = all_entries[mid..].to_vec();
            for (i, (k, v)) in entries_to_move.iter().enumerate() {
                new_internal.insert_at(i, *k, *v);
            }

            drop(old_page);
            drop(new_page);
            self.buffer_pool_manager.unpin_page(page_id, true).await?;
            self.buffer_pool_manager
                .unpin_page(new_page_id, true)
                .await?;

            (promoted_key, entries_to_move)
        };

        for (_k, child_id) in &entries_to_move {
            self.update_child_parent(*child_id, new_page_id, false)
                .await?;
        }

        if is_root {
            self.create_new_root(page_id, promoted_key, new_page_id)
                .await?;
        } else {
            self.insert_into_parent(grandparent_page_id, page_id, promoted_key, new_page_id)
                .await?;
        }

        Ok(())
    }

    /// 자식 페이지의 부모 포인터 업데이트
    async fn update_child_parent(
        &self,
        child_page_id: PageId,
        new_parent_id: PageId,
        clear_root: bool,
    ) -> Result<()> {
        let child_guard = self.buffer_pool_manager.fetch_page(child_page_id).await?;
        let mut child_page = child_guard.lock().await;

        let page_type = Self::get_page_type(&child_page.data);

        match page_type {
            IndexPageType::LeafPage => {
                let mut leaf = LeafPage::new(&mut child_page);
                leaf.set_parent_page_id(new_parent_id);
                if clear_root {
                    leaf.set_root_page(false);
                }
            }
            IndexPageType::InternalPage => {
                let mut internal = InternalPage::new(&mut child_page);
                internal.set_parent_page_id(new_parent_id);
                if clear_root {
                    internal.set_root_page(false);
                }
            }
        }

        drop(child_page);
        self.buffer_pool_manager
            .unpin_page(child_page_id, true)
            .await?;

        Ok(())
    }

    /// 메타데이터 저장
    async fn persist_metadata(&self) -> Result<()> {
        if !self.has_metadata {
            return Ok(());
        }

        let root_id = *self.root_page_id.read().await;

        let meta_page_guard = self
            .buffer_pool_manager
            .fetch_page(BTreeMetadata::METADATA_PAGE_ID)
            .await?;
        let mut meta_page = meta_page_guard.lock().await;

        let mut metadata = BTreeMetadata::from_page(&meta_page)?;
        metadata.root_page_id = root_id;
        metadata.touch();
        metadata.to_page(&mut meta_page);

        drop(meta_page);
        self.buffer_pool_manager
            .unpin_page(BTreeMetadata::METADATA_PAGE_ID, true)
            .await?;

        Ok(())
    }

    /// 범위 조회
    pub async fn range_scan(&self, start_key: u64, end_key: u64) -> Result<Vec<(u64, RecordId)>> {
        let mut results = Vec::new();
        let root_id = self.root_page_id().await;
        let mut ctx = CrabbingContext::new(self.latch_manager.clone(), LatchMode::Read);

        let start_leaf_id = self.find_leaf_page_id(&mut ctx, root_id, start_key).await?;

        ctx.release_all();

        let mut current_page_id = start_leaf_id;

        loop {
            let (entries, next_page_id, reached_end) = {
                let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;
                let mut page = page_guard.lock().await;

                let leaf = LeafPage::new(&mut page);
                let size = leaf.get_size();
                let next_id = leaf.get_next_page_id();
                let mut entries = Vec::new();
                let mut end_reached = false;

                for i in 0..size as usize {
                    let key = leaf.key_at(i);
                    if key >= end_key {
                        end_reached = true;
                        break;
                    }
                    if key >= start_key {
                        entries.push((key, leaf.value_at(i)));
                    }
                }

                drop(page);
                self.buffer_pool_manager
                    .unpin_page(current_page_id, false)
                    .await?;

                (entries, next_id, end_reached)
            };

            results.extend(entries);

            if reached_end || next_page_id == INVALID_PAGE_ID {
                break;
            }

            current_page_id = next_page_id;
        }

        Ok(results)
    }

    /// 리프 페이지 ID 찾기
    async fn find_leaf_page_id(
        &self,
        ctx: &mut CrabbingContext,
        root_id: PageId,
        key: u64,
    ) -> Result<PageId> {
        let mut current_page_id = root_id;
        ctx.acquire(current_page_id).await?;

        loop {
            let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;
            let mut page = page_guard.lock().await;

            let page_type = Self::get_page_type(&page.data);

            match page_type {
                IndexPageType::LeafPage => {
                    drop(page);
                    self.buffer_pool_manager
                        .unpin_page(current_page_id, false)
                        .await?;
                    return Ok(current_page_id);
                }
                IndexPageType::InternalPage => {
                    let internal = InternalPage::new(&mut page);
                    let next_page_id = internal.lookup(key);

                    drop(page);
                    self.buffer_pool_manager
                        .unpin_page(current_page_id, false)
                        .await?;

                    if next_page_id == INVALID_PAGE_ID {
                        return Err(GraphError::Storage {
                            message: format!(
                                "B+Tree crabbing delete hit empty internal page at page_id={}",
                                current_page_id
                            ),
                        });
                    }

                    ctx.release_ancestors();
                    ctx.acquire(next_page_id).await?;
                    current_page_id = next_page_id;
                }
            }
        }
    }

    /// 래치 관리자 참조 반환
    pub fn latch_manager(&self) -> &Arc<LatchManager> {
        &self.latch_manager
    }

    /// 값 삭제 (Crabbing 쓰기 모드)
    pub async fn delete(&self, key: u64) -> Result<Option<RecordId>> {
        let root_id = self.root_page_id().await;
        let mut ctx = CrabbingContext::new(self.latch_manager.clone(), LatchMode::Write);

        let (leaf_page_id, needs_rebalance) =
            self.find_leaf_for_delete(&mut ctx, root_id, key).await?;

        let deleted_value = self.delete_from_leaf(leaf_page_id, key).await?;

        if needs_rebalance && deleted_value.is_some() {
            self.rebalance_leaf_concurrent(&ctx, leaf_page_id).await?;
        }

        Ok(deleted_value)
    }

    /// Leaf 페이지 리밸런싱 (동시성 안전)
    async fn rebalance_leaf_concurrent(
        &self,
        ctx: &CrabbingContext,
        leaf_page_id: PageId,
    ) -> Result<()> {
        let held = ctx.held_latches();
        if held.len() < 2 {
            return Ok(());
        }

        let parent_page_id = held[held.len() - 2].page_id;

        let (left_sibling_id, right_sibling_id, leaf_index) = {
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

        if let Some(left_id) = left_sibling_id
            && self
                .try_borrow_leaf_concurrent(
                    leaf_page_id,
                    left_id,
                    parent_page_id,
                    leaf_index,
                    BorrowDirection::Left,
                )
                .await?
        {
            return Ok(());
        }

        if let Some(right_id) = right_sibling_id
            && self
                .try_borrow_leaf_concurrent(
                    leaf_page_id,
                    right_id,
                    parent_page_id,
                    leaf_index,
                    BorrowDirection::Right,
                )
                .await?
        {
            return Ok(());
        }

        if let Some(left_id) = left_sibling_id {
            self.merge_leaves_concurrent(left_id, leaf_page_id, parent_page_id, leaf_index)
                .await?;
        } else if let Some(right_id) = right_sibling_id {
            self.merge_leaves_concurrent(leaf_page_id, right_id, parent_page_id, leaf_index + 1)
                .await?;
        }

        Ok(())
    }

    /// 형제 Leaf에서 빌려오기 시도 (동시성 안전)
    async fn try_borrow_leaf_concurrent(
        &self,
        leaf_page_id: PageId,
        sibling_id: PageId,
        parent_page_id: PageId,
        leaf_index: usize,
        direction: BorrowDirection,
    ) -> Result<bool> {
        let leaf_guard = self.buffer_pool_manager.fetch_page(leaf_page_id).await?;
        let sibling_guard = self.buffer_pool_manager.fetch_page(sibling_id).await?;

        let mut leaf_page = leaf_guard.lock().await;
        let mut sibling_page = sibling_guard.lock().await;

        let new_separator_key = {
            let mut leaf = LeafPage::new(&mut leaf_page);
            let mut sibling = LeafPage::new(&mut sibling_page);
            match direction {
                BorrowDirection::Left => leaf.borrow_from_left(&mut sibling),
                BorrowDirection::Right => leaf.borrow_from_right(&mut sibling),
            }
        };

        if let Some(new_key) = new_separator_key {
            drop(leaf_page);
            drop(sibling_page);
            self.buffer_pool_manager
                .unpin_page(leaf_page_id, true)
                .await?;
            self.buffer_pool_manager
                .unpin_page(sibling_id, true)
                .await?;

            let key_index = match direction {
                BorrowDirection::Left => leaf_index,
                BorrowDirection::Right => leaf_index + 1,
            };
            self.update_parent_key_concurrent(parent_page_id, key_index, new_key)
                .await?;
            Ok(true)
        } else {
            drop(leaf_page);
            drop(sibling_page);
            self.buffer_pool_manager
                .unpin_page(leaf_page_id, false)
                .await?;
            self.buffer_pool_manager
                .unpin_page(sibling_id, false)
                .await?;
            Ok(false)
        }
    }

    /// 두 Leaf 페이지 병합 (동시성 안전)
    async fn merge_leaves_concurrent(
        &self,
        left_page_id: PageId,
        right_page_id: PageId,
        parent_page_id: PageId,
        right_index_in_parent: usize,
    ) -> Result<()> {
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

        self.remove_entry_from_parent_concurrent(parent_page_id, right_index_in_parent)
            .await?;

        Ok(())
    }

    /// 부모 Internal 페이지의 키 업데이트 (동시성 안전)
    async fn update_parent_key_concurrent(
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

    /// 부모 Internal 페이지에서 엔트리 제거 (동시성 안전)
    async fn remove_entry_from_parent_concurrent(
        &self,
        page_id: PageId,
        index: usize,
    ) -> Result<()> {
        let page_guard = self.buffer_pool_manager.fetch_page(page_id).await?;
        let mut page = page_guard.lock().await;

        let mut internal = InternalPage::new(&mut page);
        internal.remove_at(index);

        drop(page);
        self.buffer_pool_manager.unpin_page(page_id, true).await?;

        Ok(())
    }

    /// Crabbing으로 삭제용 리프 페이지 찾기
    #[inline]
    async fn find_leaf_for_delete(
        &self,
        ctx: &mut CrabbingContext,
        root_id: PageId,
        key: u64,
    ) -> Result<(PageId, bool)> {
        self.find_leaf_for_operation(ctx, root_id, key, SafetyChecker::is_safe_for_delete)
            .await
    }

    /// 리프 페이지에서 키 삭제
    async fn delete_from_leaf(&self, leaf_page_id: PageId, key: u64) -> Result<Option<RecordId>> {
        let page_guard = self.buffer_pool_manager.fetch_page(leaf_page_id).await?;
        let mut page = page_guard.lock().await;

        let mut leaf = LeafPage::new(&mut page);

        let value = leaf.binary_search(key).map(|idx| leaf.value_at(idx));

        let success = leaf.delete(key);

        drop(page);
        self.buffer_pool_manager
            .unpin_page(leaf_page_id, success)
            .await?;

        if success { Ok(value) } else { Ok(None) }
    }
}
