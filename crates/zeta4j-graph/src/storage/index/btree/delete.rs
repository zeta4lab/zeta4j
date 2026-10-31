// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! B+ Tree 삭제 연산
//!
//! 키 삭제 기능을 제공합니다.

use super::index::BPlusTree;
use super::leaf_page::LeafPage;
use super::types::RecordId;
use crate::error::{GraphError, Result};
impl BPlusTree {
    /// 삭제 연산 (Underflow 처리 포함, 이진 탐색 사용)
    pub async fn delete(&mut self, key: u64) -> Result<Option<RecordId>> {
        let leaf_page_guard = self.find_leaf_page(key, true).await?;
        let mut page = leaf_page_guard.lock().await;
        let page_id = page.id.ok_or_else(|| GraphError::Storage {
            message: "Page has no ID".to_string(),
        })?;

        let found_value: Option<RecordId>;
        let need_rebalance: bool;
        let is_root: bool;
        let parent_page_id: u32;

        {
            let mut leaf = LeafPage::new(&mut page);

            // 이진 탐색으로 키 찾기
            let found_idx = leaf.binary_search(key);

            let idx = match found_idx {
                Some(i) => i,
                None => {
                    drop(page);
                    self.buffer_pool_manager.unpin_page(page_id, false).await?;
                    return Ok(None);
                }
            };
            found_value = Some(leaf.value_at(idx));

            // SlottedPage에서는 delete()로 슬롯을 삭제하면 자동으로 크기 관리됨
            leaf.delete(key);

            is_root = leaf.is_root_page();
            parent_page_id = leaf.get_parent_page_id();
            need_rebalance = !is_root && leaf.is_underflow();
        }

        drop(page);
        self.buffer_pool_manager.unpin_page(page_id, true).await?;

        // Underflow 처리
        if need_rebalance {
            self.rebalance_leaf(page_id, parent_page_id).await?;
        }

        Ok(found_value)
    }
}
