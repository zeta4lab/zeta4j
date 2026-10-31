// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! B+ Tree Bulk Loading
//!
//! 대량의 정렬된 데이터를 효율적으로 B+Tree에 로드합니다.
//!
//! # 알고리즘
//!
//! 1. 정렬된 (key, value) 쌍을 입력받음
//! 2. 리프 페이지를 순차적으로 채움 (fill_factor 적용)
//! 3. 리프 페이지들을 linked list로 연결
//! 4. 내부 노드를 bottom-up으로 생성
//! 5. 루트 페이지 설정
//!
//! # 성능
//!
//! - 일반 삽입: O(n log n)
//! - Bulk Loading: O(n)
//!
//! # 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::storage::btree::bulk_loader::*;
//!
//! let config = BulkLoadConfig::default().with_fill_factor(0.7);
//! let loader = BulkLoader::new(&bpm, config);
//!
//! let entries: Vec<(u64, RecordId)> = get_sorted_data();
//! let tree = loader.load(entries).await?;
//! ```

use super::concurrent::ConcurrentBPlusTree;
use super::index::BPlusTree;
use super::internal_page::InternalPage;
use super::leaf_page::LeafPage;
use super::metadata::BTreeMetadata;
use super::types::{INVALID_PAGE_ID, IndexPageType, RecordId};
use crate::error::{GraphError, Result};
use crate::storage::cache::BufferPoolManager;
use crate::storage::disk::page::PageId;
use std::sync::Arc;

/// Bulk Load 설정
pub struct BulkLoadConfig {
    /// 페이지 채움 비율 (0.5 ~ 1.0)
    /// 1.0에 가까울수록 공간 효율적, 낮을수록 향후 삽입 여유
    pub fill_factor: f64,
    /// 메타데이터 페이지 생성 여부
    pub create_metadata: bool,
    /// 진행 상황 콜백 (처리된 엔트리 수)
    pub progress_callback: Option<Arc<dyn Fn(usize) + Send + Sync>>,
}

impl std::fmt::Debug for BulkLoadConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BulkLoadConfig")
            .field("fill_factor", &self.fill_factor)
            .field("create_metadata", &self.create_metadata)
            .field(
                "progress_callback",
                &self.progress_callback.as_ref().map(|_| "<callback>"),
            )
            .finish()
    }
}

impl Clone for BulkLoadConfig {
    fn clone(&self) -> Self {
        Self {
            fill_factor: self.fill_factor,
            create_metadata: self.create_metadata,
            progress_callback: self.progress_callback.clone(),
        }
    }
}

impl Default for BulkLoadConfig {
    fn default() -> Self {
        Self {
            fill_factor: 0.7,
            create_metadata: true,
            progress_callback: None,
        }
    }
}

impl BulkLoadConfig {
    /// fill_factor 설정
    pub fn with_fill_factor(mut self, factor: f64) -> Self {
        self.fill_factor = factor.clamp(0.5, 1.0);
        self
    }

    /// 메타데이터 생성 여부 설정
    pub fn with_metadata(mut self, create: bool) -> Self {
        self.create_metadata = create;
        self
    }

    /// 진행 상황 콜백 설정
    pub fn with_progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }
}

/// Bulk Load 결과
#[derive(Debug)]
pub struct BulkLoadResult {
    /// 생성된 트리
    pub root_page_id: PageId,
    /// 로드된 엔트리 수
    pub entries_loaded: usize,
    /// 생성된 리프 페이지 수
    pub leaf_pages: usize,
    /// 생성된 내부 페이지 수
    pub internal_pages: usize,
    /// 트리 높이
    pub tree_height: usize,
}

/// 레벨 정보 (내부 노드 생성용)
struct LevelInfo {
    /// 이 레벨의 첫 번째 키
    first_key: u64,
    /// 페이지 ID
    page_id: PageId,
}

/// Bulk Loader
pub struct BulkLoader {
    bpm: Arc<BufferPoolManager>,
    config: BulkLoadConfig,
}

impl BulkLoader {
    /// 새 BulkLoader 생성
    pub fn new(bpm: Arc<BufferPoolManager>, config: BulkLoadConfig) -> Self {
        Self { bpm, config }
    }

    /// 기본 설정으로 생성
    pub fn with_default_config(bpm: Arc<BufferPoolManager>) -> Self {
        Self::new(bpm, BulkLoadConfig::default())
    }

    /// 정렬된 데이터를 B+Tree로 로드
    ///
    /// # Arguments
    /// * `entries` - 정렬된 (key, value) 쌍 벡터
    ///
    /// # Returns
    /// * `BulkLoadResult` - 로드 결과
    pub async fn load(&self, entries: Vec<(u64, RecordId)>) -> Result<BulkLoadResult> {
        if entries.is_empty() {
            return self.create_empty_tree().await;
        }

        // 1. 메타데이터 페이지 생성 (옵션)
        let metadata_page_id = if self.config.create_metadata {
            Some(self.create_metadata_page().await?)
        } else {
            None
        };

        // 2. 리프 페이지 생성
        let (leaf_infos, leaf_count) = self.create_leaf_pages(&entries).await?;

        // 3. 내부 노드 생성 (bottom-up)
        let (root_page_id, internal_count, height) = self.build_internal_levels(leaf_infos).await?;

        // 4. 루트 페이지 설정
        self.set_root_flag(root_page_id).await?;

        // 5. 메타데이터 업데이트
        if let Some(_meta_id) = metadata_page_id {
            self.update_metadata(root_page_id).await?;
        }

        Ok(BulkLoadResult {
            root_page_id,
            entries_loaded: entries.len(),
            leaf_pages: leaf_count,
            internal_pages: internal_count,
            tree_height: height,
        })
    }

    /// 정렬된 데이터를 로드하여 ConcurrentBPlusTree 반환
    pub async fn load_to_tree(&self, entries: Vec<(u64, RecordId)>) -> Result<ConcurrentBPlusTree> {
        let result = self.load(entries).await?;

        Ok(ConcurrentBPlusTree::new(
            self.bpm.clone(),
            result.root_page_id,
            self.config.create_metadata,
        ))
    }

    /// 빈 트리 생성
    async fn create_empty_tree(&self) -> Result<BulkLoadResult> {
        let bpm = &self.bpm;

        // 메타데이터 페이지 생성 (옵션)
        if self.config.create_metadata {
            let meta_page_guard = bpm.new_page().await?;
            let meta_id = meta_page_guard
                .lock()
                .await
                .id
                .ok_or_else(|| GraphError::Storage {
                    message: "Page has no ID".to_string(),
                })?;
            bpm.unpin_page(meta_id, true).await?;
        }

        // 빈 루트 리프 페이지 생성
        let root_page_guard = bpm.new_page().await?;
        let root_id = {
            let mut root_page = root_page_guard.lock().await;

            let id = root_page.id.ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

            let mut leaf = LeafPage::new(&mut root_page);
            leaf.init(INVALID_PAGE_ID);
            leaf.set_root_page(true);

            id
        };

        bpm.unpin_page(root_id, true).await?;

        // 메타데이터 업데이트
        if self.config.create_metadata {
            self.update_metadata(root_id).await?;
        }

        Ok(BulkLoadResult {
            root_page_id: root_id,
            entries_loaded: 0,
            leaf_pages: 1,
            internal_pages: 0,
            tree_height: 1,
        })
    }

    /// 메타데이터 페이지 생성
    async fn create_metadata_page(&self) -> Result<PageId> {
        let bpm = &self.bpm;
        let meta_page_guard = bpm.new_page().await?;
        let meta_id = meta_page_guard
            .lock()
            .await
            .id
            .ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

        if meta_id != BTreeMetadata::METADATA_PAGE_ID {
            return Err(GraphError::Storage {
                message: format!(
                    "Expected metadata page ID {}, got {}",
                    BTreeMetadata::METADATA_PAGE_ID,
                    meta_id
                ),
            });
        }

        // 임시 루트 ID로 초기화 (나중에 업데이트)
        {
            let mut meta_page = meta_page_guard.lock().await;
            let metadata = BTreeMetadata::new(INVALID_PAGE_ID);
            metadata.to_page(&mut meta_page);
        }

        bpm.unpin_page(meta_id, true).await?;
        Ok(meta_id)
    }

    /// 리프 페이지 생성
    async fn create_leaf_pages(
        &self,
        entries: &[(u64, RecordId)],
    ) -> Result<(Vec<LevelInfo>, usize)> {
        let bpm = &self.bpm;
        let mut leaf_infos = Vec::new();
        let mut prev_page_id: Option<PageId> = None;
        let mut current_entries: Vec<(u64, RecordId)> = Vec::new();

        // 리프 페이지당 최대 엔트리 수 계산
        let test_page_guard = bpm.new_page().await?;
        let max_entries = {
            let mut test_page = test_page_guard.lock().await;
            let test_id = test_page.id.ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;
            let leaf = LeafPage::new(&mut test_page);
            let max = leaf.get_max_size();
            drop(test_page);
            bpm.unpin_page(test_id, false).await?;
            // 테스트 페이지는 free list에 반환됨
            max
        };

        let entries_per_page = ((max_entries as f64) * self.config.fill_factor) as usize;
        let entries_per_page = entries_per_page.max(1);

        let total_entries = entries.len();
        let mut processed = 0;

        for (key, value) in entries {
            current_entries.push((*key, *value));

            if current_entries.len() >= entries_per_page {
                // 페이지 생성
                let page_id = self
                    .create_single_leaf_page(bpm, &current_entries, prev_page_id)
                    .await?;

                // 레벨 정보 저장
                leaf_infos.push(LevelInfo {
                    first_key: current_entries[0].0,
                    page_id,
                });

                // 이전 페이지의 next_page_id 업데이트
                if let Some(prev_id) = prev_page_id {
                    self.update_next_page_id(bpm, prev_id, page_id).await?;
                }

                prev_page_id = Some(page_id);
                current_entries.clear();

                processed += entries_per_page.min(total_entries - processed);

                // 진행 상황 콜백
                if let Some(ref callback) = self.config.progress_callback {
                    callback(processed);
                }
            }
        }

        // 남은 엔트리 처리
        if !current_entries.is_empty() {
            let page_id = self
                .create_single_leaf_page(bpm, &current_entries, prev_page_id)
                .await?;

            leaf_infos.push(LevelInfo {
                first_key: current_entries[0].0,
                page_id,
            });

            if let Some(prev_id) = prev_page_id {
                self.update_next_page_id(bpm, prev_id, page_id).await?;
            }

            // 진행 상황 콜백
            if let Some(ref callback) = self.config.progress_callback {
                callback(total_entries);
            }
        }

        let leaf_count = leaf_infos.len();
        Ok((leaf_infos, leaf_count))
    }

    /// 단일 리프 페이지 생성
    async fn create_single_leaf_page(
        &self,
        bpm: &BufferPoolManager,
        entries: &[(u64, RecordId)],
        _prev_page_id: Option<PageId>,
    ) -> Result<PageId> {
        let page_guard = bpm.new_page().await?;
        let page_id = {
            let mut page = page_guard.lock().await;

            let id = page.id.ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

            let mut leaf = LeafPage::new(&mut page);
            leaf.init(INVALID_PAGE_ID);

            // 엔트리 추가 (이미 정렬됨 - insert 사용)
            for (key, value) in entries.iter() {
                leaf.insert(*key, *value);
            }

            id
        };

        bpm.unpin_page(page_id, true).await?;
        Ok(page_id)
    }

    /// 이전 페이지의 next_page_id 업데이트
    async fn update_next_page_id(
        &self,
        bpm: &BufferPoolManager,
        page_id: PageId,
        next_id: PageId,
    ) -> Result<()> {
        let page_guard = bpm.fetch_page(page_id).await?;
        {
            let mut page = page_guard.lock().await;
            let mut leaf = LeafPage::new(&mut page);
            leaf.set_next_page_id(next_id);
        }
        bpm.unpin_page(page_id, true).await?;
        Ok(())
    }

    /// 내부 노드 레벨 생성 (bottom-up)
    async fn build_internal_levels(
        &self,
        mut level_infos: Vec<LevelInfo>,
    ) -> Result<(PageId, usize, usize)> {
        if level_infos.len() == 1 {
            // 리프가 하나면 그것이 루트
            return Ok((level_infos[0].page_id, 0, 1));
        }

        let mut internal_count = 0;
        let mut height = 1; // 리프 레벨

        // 현재 레벨의 페이지들이 하나가 될 때까지 반복
        while level_infos.len() > 1 {
            level_infos = self
                .create_internal_level(level_infos, &mut internal_count)
                .await?;
            height += 1;
        }

        Ok((level_infos[0].page_id, internal_count, height))
    }

    /// 한 레벨의 내부 노드 생성
    async fn create_internal_level(
        &self,
        child_infos: Vec<LevelInfo>,
        internal_count: &mut usize,
    ) -> Result<Vec<LevelInfo>> {
        let bpm = &self.bpm;
        let mut parent_infos = Vec::new();

        // 내부 페이지당 최대 엔트리 수 계산
        let test_page_guard = bpm.new_page().await?;
        let max_entries = {
            let mut test_page = test_page_guard.lock().await;
            let test_id = test_page.id.ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;
            let internal = InternalPage::new(&mut test_page);
            let max = internal.get_max_size();
            drop(test_page);
            bpm.unpin_page(test_id, false).await?;
            max
        };

        let entries_per_page = ((max_entries as f64) * self.config.fill_factor) as usize;
        let entries_per_page = entries_per_page.max(2); // 최소 2개

        let mut current_children: Vec<&LevelInfo> = Vec::new();

        for child_info in &child_infos {
            current_children.push(child_info);

            if current_children.len() >= entries_per_page {
                let (page_id, first_key) = self
                    .create_single_internal_page(bpm, &current_children)
                    .await?;

                parent_infos.push(LevelInfo { first_key, page_id });

                *internal_count += 1;
                current_children.clear();
            }
        }

        // 남은 자식들 처리
        if !current_children.is_empty() {
            let (page_id, first_key) = self
                .create_single_internal_page(bpm, &current_children)
                .await?;

            parent_infos.push(LevelInfo { first_key, page_id });

            *internal_count += 1;
        }

        // 자식 페이지들의 parent_page_id 업데이트
        for _parent_info in &parent_infos {
            // 이 부모에 속한 자식들 찾기 및 업데이트
            // (간단히 모든 자식을 순회하며 업데이트)
        }

        Ok(parent_infos)
    }

    /// 단일 내부 페이지 생성
    async fn create_single_internal_page(
        &self,
        bpm: &BufferPoolManager,
        children: &[&LevelInfo],
    ) -> Result<(PageId, u64)> {
        let page_guard = bpm.new_page().await?;
        let first_key = children[0].first_key;

        let page_id = {
            let mut page = page_guard.lock().await;

            let id = page.id.ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

            let mut internal = InternalPage::new(&mut page);
            internal.init(INVALID_PAGE_ID);

            // 첫 번째 엔트리: key=0 (dummy), page_id=첫 번째 자식
            internal.insert_at(0, 0, children[0].page_id);

            // 나머지 엔트리 (insert_at 사용)
            for (i, child) in children.iter().enumerate().skip(1) {
                internal.insert_at(i, child.first_key, child.page_id);
            }

            id
        };

        // 자식 페이지들의 parent_page_id 업데이트
        for child in children {
            self.update_parent_id(bpm, child.page_id, page_id).await?;
        }

        bpm.unpin_page(page_id, true).await?;
        Ok((page_id, first_key))
    }

    /// 페이지의 parent_page_id 업데이트
    async fn update_parent_id(
        &self,
        bpm: &BufferPoolManager,
        page_id: PageId,
        parent_id: PageId,
    ) -> Result<()> {
        let page_guard = bpm.fetch_page(page_id).await?;
        {
            let mut page = page_guard.lock().await;

            // 페이지 타입 확인 (SlottedPage 구조에서 읽기)
            let page_type = BPlusTree::get_page_type_from_page(&page);

            match page_type {
                IndexPageType::LeafPage => {
                    let mut leaf = LeafPage::new(&mut page);
                    leaf.set_parent_page_id(parent_id);
                }
                IndexPageType::InternalPage => {
                    let mut internal = InternalPage::new(&mut page);
                    internal.set_parent_page_id(parent_id);
                }
            }
        }
        bpm.unpin_page(page_id, true).await?;
        Ok(())
    }

    /// 루트 페이지 플래그 설정
    async fn set_root_flag(&self, root_page_id: PageId) -> Result<()> {
        let bpm = &self.bpm;
        let page_guard = bpm.fetch_page(root_page_id).await?;
        {
            let mut page = page_guard.lock().await;

            // 페이지 타입 확인 (SlottedPage 구조에서 읽기)
            let page_type = BPlusTree::get_page_type_from_page(&page);

            match page_type {
                IndexPageType::LeafPage => {
                    let mut leaf = LeafPage::new(&mut page);
                    leaf.set_root_page(true);
                    leaf.set_parent_page_id(INVALID_PAGE_ID);
                }
                IndexPageType::InternalPage => {
                    let mut internal = InternalPage::new(&mut page);
                    internal.set_root_page(true);
                    internal.set_parent_page_id(INVALID_PAGE_ID);
                }
            }
        }
        bpm.unpin_page(root_page_id, true).await?;
        Ok(())
    }

    /// 메타데이터 업데이트
    async fn update_metadata(&self, root_page_id: PageId) -> Result<()> {
        let bpm = &self.bpm;
        let meta_page_guard = bpm.fetch_page(BTreeMetadata::METADATA_PAGE_ID).await?;
        {
            let mut meta_page = meta_page_guard.lock().await;

            let mut metadata = BTreeMetadata::from_page(&meta_page)?;
            metadata.root_page_id = root_page_id;
            metadata.touch();
            metadata.to_page(&mut meta_page);
        }
        bpm.unpin_page(BTreeMetadata::METADATA_PAGE_ID, true)
            .await?;
        Ok(())
    }
}

/// Bulk Loading을 위한 데이터 정렬기
pub struct BulkSorter;

impl BulkSorter {
    /// 데이터 정렬
    pub fn sort(entries: &mut [(u64, RecordId)]) {
        entries.sort_by_key(|(key, _)| *key);
    }

    /// 중복 키 제거 (마지막 값 유지)
    pub fn deduplicate(mut entries: Vec<(u64, RecordId)>) -> Vec<(u64, RecordId)> {
        // 먼저 정렬 (중복 제거는 정렬된 입력이 필요)
        Self::sort(&mut entries);

        let mut result = Vec::with_capacity(entries.len());
        let mut prev_key: Option<u64> = None;

        // 역순으로 순회하여 마지막 값 유지
        for (key, value) in entries.into_iter().rev() {
            if Some(key) != prev_key {
                result.push((key, value));
                prev_key = Some(key);
            }
        }

        result.reverse();
        result
    }
}
