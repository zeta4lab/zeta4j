//! B+ Tree 인덱스 구현
//!
//! 이 모듈은 B+ Tree 인덱스의 핵심 구조체와 유틸리티를 제공합니다.
//! 실제 연산은 다음 모듈들로 분리되어 있습니다:
//! - `search`: 값 조회 및 범위 검색
//! - `insert`: 삽입 및 페이지 분할
//! - `delete`: 삭제 연산
//! - `rebalance`: Underflow 처리 및 페이지 재조정

use super::internal_page::InternalPage;
use super::leaf_page::LeafPage;
use super::metadata::BTreeMetadata;
use super::types::{INVALID_PAGE_ID, IndexPageType, RecordId};
use crate::error::{GraphError, Result};
use crate::storage::cache::BufferPoolManager;
use crate::storage::disk::page::{Page, PageId, read_u16_le, read_u32_le};
use std::sync::Arc;

/// B+ Tree 인덱스 구조체
#[derive(Debug)]
pub struct BPlusTree {
    pub(super) buffer_pool_manager: Arc<BufferPoolManager>,
    pub(super) root_page_id: PageId,
    /// 메타데이터 페이지 존재 여부 (create_new_tree_with_metadata로 생성된 경우 true)
    pub(super) has_metadata: bool,
}

impl BPlusTree {
    /// 기존 트리를 열 때 사용 (메타데이터 유무 명시)
    pub fn new(bpm: Arc<BufferPoolManager>, root_page_id: PageId, has_metadata: bool) -> Self {
        Self {
            buffer_pool_manager: bpm,
            root_page_id,
            has_metadata,
        }
    }

    /// 새 트리 생성 (메타데이터 없음 - 테스트용)
    pub async fn create_new_tree(bpm: &Arc<BufferPoolManager>) -> Result<Self> {
        let root_page = bpm.new_page().await?;
        let root_id = root_page
            .lock()
            .await
            .id
            .ok_or_else(|| GraphError::Storage {
                message: "Page has no ID".to_string(),
            })?;

        // Root는 처음에 Leaf
        let mut page_guard = root_page.lock().await;
        let mut leaf = LeafPage::new(&mut page_guard);
        leaf.init(INVALID_PAGE_ID);
        leaf.set_root_page(true);

        drop(page_guard);
        bpm.unpin_page(root_id, true).await?;

        Ok(Self {
            buffer_pool_manager: bpm.clone(),
            root_page_id: root_id,
            has_metadata: false,
        })
    }

    /// 페이지 타입 확인 헬퍼
    ///
    /// SlottedPage 구조에서 메타데이터(slot 0)의 page_type 필드를 읽습니다.
    ///
    /// # 유효성 검사
    /// - slot_count >= 1 (slot 0이 존재해야 함)
    /// - slot0_offset이 페이지 범위 내에 있어야 함
    /// - 잘못된 페이지인 경우 LeafPage를 기본값으로 반환
    pub(super) fn get_page_type_from_page(page: &Page) -> IndexPageType {
        use crate::storage::disk::page::PAGE_SIZE;

        // SlottedPage 헤더 상수
        const HEADER_SIZE: usize = 16;
        const SLOT_COUNT_OFFSET: usize = 12;

        // slot_count 확인 (slot 0이 존재하는지)
        let slot_count =
            read_u16_le(&page.data[SLOT_COUNT_OFFSET..SLOT_COUNT_OFFSET + size_of::<u16>()]);

        if slot_count == 0 {
            // 슬롯이 없는 빈 페이지 - 기본값 반환
            tracing::warn!("B+Tree page has no slots, assuming LeafPage");
            return IndexPageType::LeafPage;
        }

        // Slot 0의 데이터 오프셋 읽기
        let slot0_offset = read_u16_le(&page.data[HEADER_SIZE..HEADER_SIZE + size_of::<u16>()]);

        // 오프셋 유효성 검사 (PAGE_SIZE - 4 이내여야 함)
        if slot0_offset as usize >= PAGE_SIZE - 4 {
            tracing::warn!(
                "B+Tree page has invalid slot0 offset: {} (max: {}), assuming LeafPage",
                slot0_offset,
                PAGE_SIZE - 4
            );
            return IndexPageType::LeafPage;
        }

        // 메타데이터의 page_type은 오프셋 0에 있음
        let page_type_val = read_u32_le(
            &page.data[slot0_offset as usize..slot0_offset as usize + size_of::<u32>()],
        );

        if page_type_val == 1 {
            IndexPageType::LeafPage
        } else {
            IndexPageType::InternalPage
        }
    }

    /// 루트 페이지 ID 반환
    pub fn root_page_id(&self) -> PageId {
        self.root_page_id
    }

    /// BufferPoolManager 참조 반환 (WAL 연결용)
    pub fn buffer_pool_manager(&self) -> &Arc<BufferPoolManager> {
        &self.buffer_pool_manager
    }

    /// 디스크에서 기존 B+ Tree 복구
    ///
    /// 메타데이터 페이지(Page 0)를 읽어 루트 페이지 ID와 기타 정보를 복원합니다.
    /// 메타데이터가 손상된 경우(magic=0) 자동으로 재초기화합니다.
    pub async fn recover_from_disk(bpm: Arc<BufferPoolManager>) -> Result<Self> {
        // 메타데이터 페이지 읽기 (Page 0)
        let meta_page_guard = bpm.fetch_page(BTreeMetadata::METADATA_PAGE_ID).await?;
        let mut meta_page = meta_page_guard.lock().await;

        // 손상된 메타데이터 복구 지원 (기본 루트 페이지 ID = 1)
        let metadata = BTreeMetadata::recover_or_create(&mut meta_page, 1)?;
        metadata.validate()?;

        let root_page_id = metadata.root_page_id;
        let was_recovered = meta_page.is_dirty; // recover_or_create에서 재초기화된 경우 dirty

        drop(meta_page);
        bpm.unpin_page(BTreeMetadata::METADATA_PAGE_ID, was_recovered)
            .await?;

        // 복구된 경우 즉시 플러시하여 영속화
        if was_recovered {
            bpm.flush_page(BTreeMetadata::METADATA_PAGE_ID).await?;
        }

        Ok(Self {
            buffer_pool_manager: bpm,
            root_page_id,
            has_metadata: true,
        })
    }

    /// 새 B+ Tree 생성 (메타데이터 페이지 포함)
    ///
    /// Page 0에 메타데이터를, Page 1에 루트 Leaf 페이지를 생성합니다.
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
            root_page_id,
            has_metadata: true,
        })
    }

    /// 새 B+ Tree 생성 후 메타데이터 페이지를 즉시 디스크에 플러시
    ///
    /// 크래시 안전성을 보장하기 위해 메타데이터와 루트 페이지를 디스크에 동기화합니다.
    /// 벡터 인덱스처럼 서버 종료 시 데이터 손실이 발생할 수 있는 경우 이 메서드를 사용하세요.
    pub async fn create_new_tree_flushed(bpm: &Arc<BufferPoolManager>) -> Result<Self> {
        let tree = Self::create_new_tree_with_metadata(bpm).await?;

        // 크래시 안전성: 메타데이터 페이지와 루트 페이지를 즉시 디스크에 플러시
        bpm.flush_page(BTreeMetadata::METADATA_PAGE_ID).await?;
        bpm.flush_page(tree.root_page_id).await?;

        Ok(tree)
    }

    /// 메타데이터를 디스크에 저장
    ///
    /// 루트 페이지 ID가 변경되었을 때(분할로 인한 새 루트 생성 등) 호출합니다.
    /// 메타데이터가 없는 트리(create_new_tree로 생성)에서는 아무 것도 하지 않습니다.
    pub(super) async fn persist_metadata(&self) -> Result<()> {
        if !self.has_metadata {
            return Ok(());
        }

        let bpm = &self.buffer_pool_manager;

        let meta_page_guard = bpm.fetch_page(BTreeMetadata::METADATA_PAGE_ID).await?;
        let mut meta_page = meta_page_guard.lock().await;

        // 기존 메타데이터 로드
        let mut metadata = BTreeMetadata::from_page(&meta_page)?;

        // 루트 페이지 ID 업데이트
        metadata.root_page_id = self.root_page_id;
        metadata.touch();

        // 저장
        metadata.to_page(&mut meta_page);

        drop(meta_page);
        bpm.unpin_page(BTreeMetadata::METADATA_PAGE_ID, true)
            .await?;

        Ok(())
    }

    /// 현재 트리가 비어있는지 확인
    pub async fn is_empty(&self) -> Result<bool> {
        let bpm = &self.buffer_pool_manager;
        let root_guard = bpm.fetch_page(self.root_page_id).await?;
        let mut root_page = root_guard.lock().await;

        let page_type = Self::get_page_type_from_page(&root_page);
        let is_empty = match page_type {
            IndexPageType::LeafPage => {
                let leaf = LeafPage::new(&mut root_page);
                leaf.get_size() == 0
            }
            IndexPageType::InternalPage => false, // Internal이면 비어있지 않음
        };

        let root_id = self.root_page_id;
        drop(root_page);
        drop(root_guard);
        bpm.unpin_page(root_id, false).await?;

        Ok(is_empty)
    }

    // ============================================================
    // Phase 5: Online Rebuild
    // ============================================================

    /// 모든 엔트리 스캔 (왼쪽부터 오른쪽으로)
    ///
    /// 리프 페이지의 linked list를 따라가며 모든 (key, value) 쌍을 수집합니다.
    pub async fn scan_all(&self) -> Result<Vec<(u64, RecordId)>> {
        let mut results = Vec::new();

        // 가장 왼쪽 리프 찾기 (key=0으로 검색)
        let first_leaf_guard = self.find_leaf_page(0, false).await?;
        let mut current_page_id =
            first_leaf_guard
                .lock()
                .await
                .id
                .ok_or_else(|| GraphError::Storage {
                    message: "Page has no ID".to_string(),
                })?;

        // 첫 번째 리프 처리
        let mut next_page_id = {
            let mut page = first_leaf_guard.lock().await;
            let leaf = LeafPage::new(&mut page);
            let size = leaf.get_size();
            let next_id = leaf.get_next_page_id();

            for i in 0..size as usize {
                results.push((leaf.key_at(i), leaf.value_at(i)));
            }

            next_id
        };

        self.buffer_pool_manager
            .unpin_page(current_page_id, false)
            .await?;

        // 다음 리프들 처리
        while next_page_id != INVALID_PAGE_ID {
            current_page_id = next_page_id;

            let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;

            next_page_id = {
                let mut page = page_guard.lock().await;
                let leaf = LeafPage::new(&mut page);
                let size = leaf.get_size();
                let next_id = leaf.get_next_page_id();

                for i in 0..size as usize {
                    results.push((leaf.key_at(i), leaf.value_at(i)));
                }

                next_id
            };

            self.buffer_pool_manager
                .unpin_page(current_page_id, false)
                .await?;
        }

        Ok(results)
    }

    /// 인덱스 재구축 (Online Rebuild)
    ///
    /// 기존 트리의 모든 엔트리를 스캔하여 새 트리를 구축하고,
    /// 메타데이터를 업데이트하여 원자적으로 교체합니다.
    ///
    /// # Arguments
    /// * `fill_factor` - 새 트리의 페이지 채움 비율 (0.5 ~ 1.0, 기본값 0.7)
    ///
    /// # Returns
    /// * `RebuildResult` - 재구축 결과 통계
    pub async fn rebuild(&mut self, fill_factor: Option<f64>) -> Result<RebuildResult> {
        // 1. 기존 데이터 스캔
        let entries = self.scan_all().await?;
        let old_root = self.root_page_id;

        if entries.is_empty() {
            return Ok(RebuildResult {
                old_root_page_id: old_root,
                new_root_page_id: old_root,
                entries_migrated: 0,
                old_leaf_pages: 0,
                new_leaf_pages: 0,
            });
        }

        // In-place rebuild 수행
        // (BulkLoader는 RwLock<BPM>을 필요로 하여 현재 구조와 호환되지 않음)
        self.rebuild_in_place(entries, fill_factor.unwrap_or(0.7))
            .await
    }

    /// In-place rebuild (같은 파일 내에서 재구축)
    async fn rebuild_in_place(
        &mut self,
        entries: Vec<(u64, RecordId)>,
        fill_factor: f64,
    ) -> Result<RebuildResult> {
        let old_root = self.root_page_id;
        let entry_count = entries.len();

        // 새 루트 리프 페이지 생성
        let new_root_id = {
            let bpm = &self.buffer_pool_manager;
            let new_page = bpm.new_page().await?;
            let id = new_page
                .lock()
                .await
                .id
                .ok_or_else(|| GraphError::Storage {
                    message: "Page has no ID".to_string(),
                })?;

            {
                let mut page = new_page.lock().await;
                let mut leaf = LeafPage::new(&mut page);
                leaf.init(INVALID_PAGE_ID);
                leaf.set_root_page(true);
            }

            bpm.unpin_page(id, true).await?;
            id
        };

        // 루트 교체
        self.root_page_id = new_root_id;

        // 엔트리 재삽입
        for (key, value) in entries {
            self.upsert(key, value).await?;
        }

        // 메타데이터 업데이트
        self.persist_metadata().await?;

        // 새 트리의 리프 페이지 수 계산
        let new_leaf_count = self.count_leaf_pages().await?;

        let _ = fill_factor; // 현재는 사용하지 않음 (upsert 사용)

        Ok(RebuildResult {
            old_root_page_id: old_root,
            new_root_page_id: self.root_page_id,
            entries_migrated: entry_count,
            old_leaf_pages: 0,
            new_leaf_pages: new_leaf_count,
        })
    }

    /// 리프 페이지 수 계산
    async fn count_leaf_pages(&self) -> Result<usize> {
        let mut count = 0;

        // 가장 왼쪽 리프 찾기
        let first_leaf_guard = self.find_leaf_page(0, false).await?;
        let mut current_page_id =
            first_leaf_guard
                .lock()
                .await
                .id
                .ok_or_else(|| GraphError::Storage {
                    message: "Page has no ID".to_string(),
                })?;

        let mut next_page_id = {
            let mut page = first_leaf_guard.lock().await;
            let leaf = LeafPage::new(&mut page);
            count += 1;
            leaf.get_next_page_id()
        };

        self.buffer_pool_manager
            .unpin_page(current_page_id, false)
            .await?;

        while next_page_id != INVALID_PAGE_ID {
            current_page_id = next_page_id;
            count += 1;

            let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;

            next_page_id = {
                let mut page = page_guard.lock().await;
                let leaf = LeafPage::new(&mut page);
                leaf.get_next_page_id()
            };

            self.buffer_pool_manager
                .unpin_page(current_page_id, false)
                .await?;
        }

        Ok(count)
    }

    /// 트리 구조 검증
    ///
    /// B+Tree 불변식을 검증합니다:
    /// - 모든 리프가 같은 레벨에 있는지
    /// - 키가 정렬되어 있는지
    /// - 내부 노드의 키가 자식 범위를 올바르게 구분하는지
    pub async fn verify(&self) -> Result<VerifyResult> {
        let mut result = VerifyResult {
            is_valid: true,
            entry_count: 0,
            leaf_count: 0,
            internal_count: 0,
            height: 0,
            errors: Vec::new(),
        };

        // 높이 계산 및 리프 순회
        let height = self.calculate_height().await?;
        result.height = height;

        // 리프 순회하며 키 정렬 검증
        let entries = self.scan_all().await?;
        result.entry_count = entries.len();

        // 키 정렬 검증
        let mut prev_key: Option<u64> = None;
        for (key, _) in &entries {
            if let Some(pk) = prev_key
                && *key <= pk
            {
                result.is_valid = false;
                result
                    .errors
                    .push(format!("Keys not sorted: {} should be > {}", key, pk));
            }
            prev_key = Some(*key);
        }

        // 리프 페이지 수 계산
        result.leaf_count = self.count_leaf_pages().await?;

        Ok(result)
    }

    /// Leaf chain 무결성 진단
    ///
    /// range_scan이 사용하는 leaf linked list를 검증하고,
    /// 내부 페이지 라우팅(get_value)으로만 접근 가능한 키와 비교하여
    /// linked list에서 누락된 키를 찾아냅니다.
    ///
    /// 주의: O(max_key · log n) 비용이므로 서버 시작(recovery) 시에만 호출할 것.
    /// hot path에서 매번 호출하면 안 됩니다.
    pub async fn verify_leaf_chain(&self) -> Result<LeafChainReport> {
        let mut report = LeafChainReport {
            is_valid: true,
            chain_leaf_count: 0,
            chain_entry_count: 0,
            chain_pages: Vec::new(),
            missing_keys: Vec::new(),
            orphan_keys: Vec::new(),
        };

        // 단일 순회: leaf linked list를 따라가며 엔트리 수집 + 페이지 상세 정보를 동시에 구축
        let mut chain_keys = std::collections::HashSet::new();
        let mut visited_pages = std::collections::HashSet::new();

        let first_leaf_guard = self.find_leaf_page(0, false).await?;
        let mut current_page_id =
            first_leaf_guard
                .lock()
                .await
                .id
                .ok_or_else(|| GraphError::Storage {
                    message: "Page has no ID".to_string(),
                })?;

        {
            let mut page = first_leaf_guard.lock().await;
            let leaf = LeafPage::new(&mut page);
            let entry_count = leaf.get_size() as usize;
            let next_id = leaf.get_next_page_id();
            for i in 0..leaf.get_size() {
                chain_keys.insert(leaf.key_at(i as usize));
            }
            report.chain_pages.push(LeafPageInfo {
                page_id: current_page_id,
                entry_count,
                next_page_id: next_id,
                min_key: if entry_count > 0 {
                    Some(leaf.key_at(0))
                } else {
                    None
                },
                max_key: if entry_count > 0 {
                    Some(leaf.key_at(entry_count - 1))
                } else {
                    None
                },
            });
            report.chain_leaf_count += 1;
            visited_pages.insert(current_page_id);

            current_page_id = next_id;
        }
        self.buffer_pool_manager
            .unpin_page(report.chain_pages[0].page_id, false)
            .await?;

        const MAX_LEAF_CHAIN: usize = 1_000_000;

        while current_page_id != INVALID_PAGE_ID {
            // Cycle guard: 이미 방문한 페이지면 중단
            if visited_pages.contains(&current_page_id) {
                tracing::error!(
                    page_id = current_page_id,
                    "verify_leaf_chain: cycle detected in leaf linked list"
                );
                report.is_valid = false;
                break;
            }
            if visited_pages.len() >= MAX_LEAF_CHAIN {
                tracing::error!(
                    chain_len = visited_pages.len(),
                    "verify_leaf_chain: leaf chain exceeded max length"
                );
                report.is_valid = false;
                break;
            }
            visited_pages.insert(current_page_id);

            let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;

            let next_id = {
                let mut page = page_guard.lock().await;
                let leaf = LeafPage::new(&mut page);
                let entry_count = leaf.get_size() as usize;
                let next = leaf.get_next_page_id();
                for i in 0..leaf.get_size() {
                    chain_keys.insert(leaf.key_at(i as usize));
                }
                report.chain_pages.push(LeafPageInfo {
                    page_id: current_page_id,
                    entry_count,
                    next_page_id: next,
                    min_key: if entry_count > 0 {
                        Some(leaf.key_at(0))
                    } else {
                        None
                    },
                    max_key: if entry_count > 0 {
                        Some(leaf.key_at(entry_count - 1))
                    } else {
                        None
                    },
                });
                report.chain_leaf_count += 1;
                next
            };

            self.buffer_pool_manager
                .unpin_page(current_page_id, false)
                .await?;

            current_page_id = next_id;
        }

        report.chain_entry_count = chain_keys.len();

        // Phase 2: get_value로 교차 검증 (min_key..=max_key 범위만)
        let min_key = report.chain_pages.first().and_then(|p| p.min_key);
        let max_key = report.chain_pages.last().and_then(|p| p.max_key);

        if let (Some(lo), Some(hi)) = (min_key, max_key) {
            for key in lo..=hi {
                let found = self.get_value(key).await?.is_some();
                let in_chain = chain_keys.contains(&key);

                if found && !in_chain {
                    report.missing_keys.push(key);
                    report.is_valid = false;
                } else if !found && in_chain {
                    report.orphan_keys.push(key);
                    report.is_valid = false;
                }
            }
        }

        Ok(report)
    }

    /// 트리 높이 계산
    async fn calculate_height(&self) -> Result<usize> {
        let mut height = 0;
        let mut current_page_id = self.root_page_id;

        loop {
            height += 1;

            let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;

            let (page_type, next_child) = {
                let mut page = page_guard.lock().await;

                let page_type = Self::get_page_type_from_page(&page);
                let next = match page_type {
                    IndexPageType::LeafPage => None,
                    IndexPageType::InternalPage => {
                        let internal = InternalPage::new(&mut page);
                        if internal.get_size() > 0 {
                            Some(internal.value_at(0))
                        } else {
                            None
                        }
                    }
                };

                (page_type, next)
            };

            self.buffer_pool_manager
                .unpin_page(current_page_id, false)
                .await?;

            match page_type {
                IndexPageType::LeafPage => break,
                IndexPageType::InternalPage => {
                    if let Some(child_id) = next_child {
                        current_page_id = child_id;
                    } else {
                        break;
                    }
                }
            }
        }

        Ok(height)
    }
}

/// Rebuild 결과
#[derive(Debug, Clone)]
pub struct RebuildResult {
    /// 이전 루트 페이지 ID
    pub old_root_page_id: PageId,
    /// 새 루트 페이지 ID
    pub new_root_page_id: PageId,
    /// 마이그레이션된 엔트리 수
    pub entries_migrated: usize,
    /// 이전 리프 페이지 수
    pub old_leaf_pages: usize,
    /// 새 리프 페이지 수
    pub new_leaf_pages: usize,
}

/// 검증 결과
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// 트리가 유효한지
    pub is_valid: bool,
    /// 총 엔트리 수
    pub entry_count: usize,
    /// 리프 페이지 수
    pub leaf_count: usize,
    /// 내부 페이지 수
    pub internal_count: usize,
    /// 트리 높이
    pub height: usize,
    /// 발견된 오류들
    pub errors: Vec<String>,
}

/// Leaf chain 진단 결과
#[derive(Debug, Clone)]
pub struct LeafChainReport {
    /// leaf chain이 유효한지
    pub is_valid: bool,
    /// linked list로 순회한 리프 페이지 수
    pub chain_leaf_count: usize,
    /// linked list로 수집한 총 엔트리 수
    pub chain_entry_count: usize,
    /// 각 리프 페이지의 상세 정보
    pub chain_pages: Vec<LeafPageInfo>,
    /// get_value로는 찾지만 range_scan에서 누락된 키
    pub missing_keys: Vec<u64>,
    /// range_scan에는 있지만 get_value로 찾을 수 없는 키
    pub orphan_keys: Vec<u64>,
}

/// 개별 리프 페이지 정보
#[derive(Debug, Clone)]
pub struct LeafPageInfo {
    /// 페이지 ID
    pub page_id: PageId,
    /// 엔트리 수
    pub entry_count: usize,
    /// 다음 리프 페이지 ID
    pub next_page_id: PageId,
    /// 최소 키
    pub min_key: Option<u64>,
    /// 최대 키
    pub max_key: Option<u64>,
}

impl std::fmt::Display for LeafChainReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== Leaf Chain Report ===")?;
        writeln!(f, "Valid: {}", self.is_valid)?;
        writeln!(f, "Leaf pages in chain: {}", self.chain_leaf_count)?;
        writeln!(f, "Entries in chain: {}", self.chain_entry_count)?;

        if !self.missing_keys.is_empty() {
            writeln!(
                f,
                "MISSING from range_scan ({} keys): {:?}",
                self.missing_keys.len(),
                self.missing_keys
            )?;
        }
        if !self.orphan_keys.is_empty() {
            writeln!(
                f,
                "ORPHAN in range_scan ({} keys): {:?}",
                self.orphan_keys.len(),
                self.orphan_keys
            )?;
        }

        writeln!(f, "\nLeaf page chain:")?;
        for (i, page) in self.chain_pages.iter().enumerate() {
            writeln!(
                f,
                "  [{}] page_id={}, entries={}, keys=[{:?}..{:?}], next={}",
                i,
                page.page_id,
                page.entry_count,
                page.min_key,
                page.max_key,
                if page.next_page_id == u32::MAX {
                    "END".to_string()
                } else {
                    page.next_page_id.to_string()
                },
            )?;
        }

        Ok(())
    }
}
