// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! L1 캐시 (moka 기반 인메모리 LRU)

use moka::sync::Cache;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;

/// L1 캐시 래퍼
pub struct L1Cache<K, V> {
    inner: Cache<K, V>,
}

impl<K, V> L1Cache<K, V>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// 새 L1 캐시 생성
    pub fn new(max_capacity: u64, ttl: Duration) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_capacity)
            .time_to_live(ttl)
            .build();

        Self { inner: cache }
    }

    /// 값 조회
    pub fn get(&self, key: &K) -> Option<V> {
        self.inner.get(key)
    }

    /// 값 삽입
    pub fn insert(&self, key: K, value: V) {
        self.inner.insert(key, value);
    }

    /// 값 무효화
    pub fn invalidate(&self, key: &K) {
        self.inner.invalidate(key);
    }

    /// 모든 값 무효화
    pub fn clear(&self) {
        self.inner.invalidate_all();
    }

    /// 현재 항목 수
    pub fn len(&self) -> usize {
        self.inner.entry_count() as usize
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 보류 중인 작업 실행 (entry_count 동기화)
    ///
    /// moka 캐시는 비동기적으로 entry_count를 업데이트합니다.
    /// 테스트에서 정확한 카운트가 필요한 경우 이 메서드를 호출하세요.
    pub fn sync(&self) {
        self.inner.run_pending_tasks();
    }
}

/// Arc로 감싼 L1 캐시
pub type SharedL1Cache<K, V> = Arc<L1Cache<K, V>>;
