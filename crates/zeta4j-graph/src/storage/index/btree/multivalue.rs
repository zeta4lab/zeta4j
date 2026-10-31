// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Multi-value B+Tree
//!
//! 하나의 키에 여러 값을 저장할 수 있는 B+Tree입니다.
//! Secondary Index (Label → NodeId set, Type → EdgeId set)에 사용됩니다.
//!
//! ## 구현 방식
//!
//! 기존 B+Tree를 래핑하여 composite key 방식으로 1:N 매핑을 구현합니다:
//! - 실제 키: `(key_hash, value)` 조합
//! - 이를 통해 같은 key에 대해 여러 value를 저장 가능
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::storage::btree::multivalue::MultiValueBPlusTree;
//!
//! let tree = MultiValueBPlusTree::create_new(bpm)?;
//!
//! // 같은 키에 여러 값 추가
//! tree.insert(b"Person", 1)?;
//! tree.insert(b"Person", 2)?;
//! tree.insert(b"Person", 3)?;
//!
//! // 모든 값 조회
//! let values = tree.get_all(b"Person")?;
//! assert_eq!(values, vec![1, 2, 3]);
//! ```

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::index::BPlusTree;
use super::types::RecordId;
use crate::error::Result;
use crate::storage::cache::BufferPoolManager;
use crate::storage::disk::page::PageId;

/// Multi-value B+Tree
///
/// 하나의 키에 여러 값(u64)을 저장할 수 있는 인덱스입니다.
#[derive(Debug)]
pub struct MultiValueBPlusTree {
    /// 내부 B+Tree (composite key 사용)
    tree: BPlusTree,
}

impl MultiValueBPlusTree {
    /// 기존 트리 열기
    pub fn new(tree: BPlusTree) -> Self {
        Self { tree }
    }

    /// 새 Multi-value B+Tree 생성 (메타데이터 포함)
    pub async fn create_new(bpm: &Arc<BufferPoolManager>) -> Result<Self> {
        let tree = BPlusTree::create_new_tree_with_metadata(bpm).await?;
        Ok(Self { tree })
    }

    /// 디스크에서 복구
    pub async fn recover_from_disk(bpm: Arc<BufferPoolManager>) -> Result<Self> {
        let tree = BPlusTree::recover_from_disk(bpm).await?;
        Ok(Self { tree })
    }

    /// BufferPoolManager 참조 반환 (WAL 연결용)
    pub fn buffer_pool_manager(&self) -> &Arc<BufferPoolManager> {
        self.tree.buffer_pool_manager()
    }

    /// 키에 값 추가
    ///
    /// 이미 존재하는 (key, value) 쌍이면 무시합니다.
    pub async fn insert(&mut self, key: &[u8], value: u64) -> Result<()> {
        let composite_key = self.make_composite_key(key, value);

        // 이미 존재하는지 확인
        if self.tree.get_value(composite_key).await?.is_some() {
            return Ok(()); // 이미 존재하면 무시
        }

        // 값을 RecordId로 인코딩 (page_id에 상위 32비트, slot_id에 하위 16비트)
        let record_id = Self::value_to_record_id(value);
        self.tree.insert(composite_key, record_id).await?;

        Ok(())
    }

    /// 키에서 값 제거
    ///
    /// 해당 (key, value) 쌍이 존재하면 삭제하고 true 반환
    pub async fn remove(&mut self, key: &[u8], value: u64) -> Result<bool> {
        let composite_key = self.make_composite_key(key, value);

        // 존재하는지 확인
        if self.tree.get_value(composite_key).await?.is_none() {
            return Ok(false);
        }

        self.tree.delete(composite_key).await?;
        Ok(true)
    }

    /// 키의 모든 값 조회
    ///
    /// 범위 스캔을 통해 해당 키의 모든 값을 반환합니다.
    pub async fn get_all(&self, key: &[u8]) -> Result<Vec<u64>> {
        let key_hash = Self::hash_key(key);

        // 범위: [key_hash << 32, (key_hash + 1) << 32)
        // 이렇게 하면 해당 key_hash를 가진 모든 composite key를 스캔
        let start = (key_hash as u64) << 32;
        let end = ((key_hash as u64) + 1) << 32;

        let entries = self.tree.range_scan(start, end).await?;

        // 결과에서 value만 추출 (composite key의 하위 32비트)
        let values: Vec<u64> = entries
            .into_iter()
            .map(|(composite_key, _record_id)| composite_key & 0xFFFF_FFFF)
            .collect();

        Ok(values)
    }

    /// 키에 특정 값이 존재하는지 확인
    pub async fn contains(&self, key: &[u8], value: u64) -> Result<bool> {
        let composite_key = self.make_composite_key(key, value);
        Ok(self.tree.get_value(composite_key).await?.is_some())
    }

    /// 키에 연결된 값의 개수 반환
    pub async fn count(&self, key: &[u8]) -> Result<usize> {
        Ok(self.get_all(key).await?.len())
    }

    /// 키 존재 여부 확인
    pub async fn contains_key(&self, key: &[u8]) -> Result<bool> {
        Ok(!self.get_all(key).await?.is_empty())
    }

    /// 루트 페이지 ID 반환
    pub fn root_page_id(&self) -> PageId {
        self.tree.root_page_id()
    }

    /// 내부 B+Tree 참조 반환
    pub fn inner(&self) -> &BPlusTree {
        &self.tree
    }

    /// 버퍼 풀 플러시
    pub async fn flush(&self) -> Result<()> {
        self.tree.buffer_pool_manager.flush_all_pages().await
    }

    // ==================== Private Helpers ====================

    /// Composite key 생성: key_hash (32bit) | value (32bit)
    fn make_composite_key(&self, key: &[u8], value: u64) -> u64 {
        let key_hash = Self::hash_key(key);
        ((key_hash as u64) << 32) | (value & 0xFFFF_FFFF)
    }

    /// 키의 해시값 계산 (32비트)
    fn hash_key(key: &[u8]) -> u32 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish() as u32
    }

    /// u64 값을 RecordId로 인코딩
    fn value_to_record_id(value: u64) -> RecordId {
        RecordId {
            page_id: (value >> 16) as u32,
            slot_id: (value & 0xFFFF) as u16,
        }
    }

    /// RecordId에서 u64 값 디코딩
    #[expect(dead_code)]
    fn record_id_to_value(record_id: RecordId) -> u64 {
        ((record_id.page_id as u64) << 16) | (record_id.slot_id as u64)
    }
}

/// 문자열 키용 헬퍼
impl MultiValueBPlusTree {
    /// 문자열 키에 값 추가
    pub async fn insert_str(&mut self, key: &str, value: u64) -> Result<()> {
        self.insert(key.as_bytes(), value).await
    }

    /// 문자열 키에서 값 제거
    pub async fn remove_str(&mut self, key: &str, value: u64) -> Result<bool> {
        self.remove(key.as_bytes(), value).await
    }

    /// 문자열 키의 모든 값 조회
    pub async fn get_all_str(&self, key: &str) -> Result<Vec<u64>> {
        self.get_all(key.as_bytes()).await
    }

    /// 문자열 키에 특정 값이 존재하는지 확인
    pub async fn contains_str(&self, key: &str, value: u64) -> Result<bool> {
        self.contains(key.as_bytes(), value).await
    }
}
