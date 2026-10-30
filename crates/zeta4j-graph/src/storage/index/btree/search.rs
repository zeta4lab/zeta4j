//! B+ Tree 검색 연산
//!
//! 키 조회 및 범위 스캔 기능을 제공합니다.

use super::index::BPlusTree;
use super::internal_page::InternalPage;
use super::leaf_page::LeafPage;
use super::types::{INVALID_PAGE_ID, IndexPageType, RecordId};
use crate::error::{GraphError, Result};
use crate::storage::disk::page::Page;
use std::sync::Arc;

impl BPlusTree {
    /// 값 조회 (이진 탐색 사용)
    pub async fn get_value(&self, key: u64) -> Result<Option<RecordId>> {
        // 1. Find Leaf Page
        let leaf_page_guard = self.find_leaf_page(key, false).await?;
        let mut page = leaf_page_guard.lock().await;
        let page_id = page.id.ok_or_else(|| GraphError::Storage {
            message: "Page has no ID".to_string(),
        })?;

        // 2. 이진 탐색으로 키 찾기
        let leaf = LeafPage::new(&mut page);
        let result = leaf.binary_search(key).map(|idx| leaf.value_at(idx));

        // 3. Unpin
        drop(page);
        self.buffer_pool_manager.unpin_page(page_id, false).await?;

        Ok(result)
    }

    /// 키에 해당하는 Leaf Page 찾기
    /// Internal Node를 탐색하여 최종 Leaf Page를 반환합니다.
    ///
    /// 최대 탐색 깊이를 제한하여 손상된 트리에서의 무한루프를 방지합니다.
    pub(super) async fn find_leaf_page(
        &self,
        key: u64,
        _write_mode: bool,
    ) -> Result<Arc<tokio::sync::Mutex<Page>>> {
        let mut current_page_id = self.root_page_id;
        const MAX_TREE_DEPTH: usize = 64;

        for _depth in 0..MAX_TREE_DEPTH {
            let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;

            let mut page = page_guard.lock().await;

            // 페이지 타입 확인 (SlottedPage 구조에서 메타데이터 읽기)
            let page_type = Self::get_page_type_from_page(&page);

            match page_type {
                IndexPageType::LeafPage => {
                    // Leaf 페이지 발견 - 반환
                    drop(page);
                    return Ok(page_guard);
                }
                IndexPageType::InternalPage => {
                    // Internal 페이지 - 다음 자식 찾기
                    let internal = InternalPage::new(&mut page);
                    let next_page_id = internal.lookup(key);

                    // 현재 페이지 unpin
                    drop(page);
                    drop(page_guard);
                    self.buffer_pool_manager
                        .unpin_page(current_page_id, false)
                        .await?;

                    // INVALID_PAGE_ID면 빈 내부 페이지 → 빈 트리
                    if next_page_id == INVALID_PAGE_ID {
                        return Err(GraphError::Storage {
                            message: format!(
                                "B+Tree traversal hit empty internal page at page_id={}",
                                current_page_id
                            ),
                        });
                    }

                    // 다음 페이지로 이동
                    current_page_id = next_page_id;
                }
            }
        }

        Err(GraphError::Storage {
            message: format!(
                "B+Tree traversal exceeded max depth ({}) — possible cycle at page_id={}",
                MAX_TREE_DEPTH, current_page_id
            ),
        })
    }

    /// 범위 조회 (start_key 이상, end_key 미만)
    /// 연속된 Leaf 페이지를 따라가며 모든 결과를 수집
    pub async fn range_scan(&self, start_key: u64, end_key: u64) -> Result<Vec<(u64, RecordId)>> {
        let mut results = Vec::new();

        // 시작 키에 해당하는 Leaf 찾기
        let leaf_page_guard = self.find_leaf_page(start_key, false).await?;
        let mut current_page_id =
            leaf_page_guard
                .lock()
                .await
                .id
                .ok_or_else(|| GraphError::Storage {
                    message: "Page has no ID".to_string(),
                })?;

        // find_leaf_page가 반환한 페이지를 먼저 처리
        let (mut next_page_id, mut reached_end) = {
            let mut page = leaf_page_guard.lock().await;
            let leaf = LeafPage::new(&mut page);
            let size = leaf.get_size();
            let next_id = leaf.get_next_page_id();
            let mut end_reached = false;

            for i in 0..size as usize {
                let key = leaf.key_at(i);
                if key >= end_key {
                    end_reached = true;
                    break;
                }
                if key >= start_key {
                    results.push((key, leaf.value_at(i)));
                }
            }

            (next_id, end_reached)
        };

        // find_leaf_page가 반환한 페이지 unpin
        self.buffer_pool_manager
            .unpin_page(current_page_id, false)
            .await?;

        // 다음 페이지들 처리
        // 최대 순회 횟수를 제한하여 손상된 linked list에서의 무한 루프 방지
        const MAX_LEAF_CHAIN: usize = 1_000_000;
        let mut chain_count = 0;
        let mut prev_page_id = current_page_id;

        while !reached_end && next_page_id != INVALID_PAGE_ID {
            chain_count += 1;
            if chain_count > MAX_LEAF_CHAIN {
                tracing::error!(
                    chain_count,
                    last_page_id = next_page_id,
                    "B+Tree range scan: leaf chain exceeded max length, aborting"
                );
                break;
            }

            // 자기 자신을 가리키는 순환 감지
            if next_page_id == prev_page_id {
                tracing::error!(
                    page_id = next_page_id,
                    "B+Tree range scan: leaf chain cycle detected (self-loop), aborting"
                );
                break;
            }

            prev_page_id = current_page_id;
            current_page_id = next_page_id;

            let page_guard = self.buffer_pool_manager.fetch_page(current_page_id).await?;

            let result = {
                let mut page = page_guard.lock().await;
                let leaf = LeafPage::new(&mut page);
                let size = leaf.get_size();
                let next_id = leaf.get_next_page_id();
                let mut end_reached = false;

                for i in 0..size as usize {
                    let key = leaf.key_at(i);
                    if key >= end_key {
                        end_reached = true;
                        break;
                    }
                    if key >= start_key {
                        results.push((key, leaf.value_at(i)));
                    }
                }

                (next_id, end_reached)
            };

            self.buffer_pool_manager
                .unpin_page(current_page_id, false)
                .await?;

            next_page_id = result.0;
            reached_end = result.1;
        }

        Ok(results)
    }
}
