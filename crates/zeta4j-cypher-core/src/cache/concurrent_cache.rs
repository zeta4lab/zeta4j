// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 동시성 지원 LRU 캐시
//!
//! DashMap + 효율적인 LRU 추적을 결합한 스레드-안전 캐시입니다.
//! O(1) 조회/삽입/삭제 성능을 제공합니다.

use std::collections::VecDeque;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use dashmap::DashMap;
use parking_lot::RwLock;

/// 동시성 캐시 통계
#[derive(Debug, Default)]
pub struct ConcurrentCacheStats {
    /// 캐시 히트 수
    pub hits: AtomicU64,
    /// 캐시 미스 수
    pub misses: AtomicU64,
    /// 삽입 수
    pub inserts: AtomicU64,
    /// 축출 수
    pub evictions: AtomicU64,
    /// 무효화 수
    pub invalidations: AtomicU64,
}

impl ConcurrentCacheStats {
    /// 새 통계 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 히트 기록
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// 미스 기록
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// 삽입 기록
    pub fn record_insert(&self) {
        self.inserts.fetch_add(1, Ordering::Relaxed);
    }

    /// 축출 기록
    pub fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    /// 무효화 기록
    pub fn record_invalidation(&self) {
        self.invalidations.fetch_add(1, Ordering::Relaxed);
    }

    /// 히트율 계산
    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    /// 스냅샷 반환
    pub fn snapshot(&self) -> CacheStatsSnapshot {
        CacheStatsSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            inserts: self.inserts.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            invalidations: self.invalidations.load(Ordering::Relaxed),
        }
    }

    /// 통계 리셋
    pub fn reset(&self) {
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.inserts.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
        self.invalidations.store(0, Ordering::Relaxed);
    }
}

/// 통계 스냅샷
#[derive(Debug, Clone, Default)]
pub struct CacheStatsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub inserts: u64,
    pub evictions: u64,
    pub invalidations: u64,
}

impl CacheStatsSnapshot {
    /// 히트율 계산
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// LRU 항목
#[derive(Debug, Clone)]
struct LruEntry<K> {
    key: K,
    last_access: Instant,
}

/// 동시성 LRU 캐시
///
/// 특징:
/// - DashMap 기반으로 동시 읽기/쓰기 지원
/// - Arc로 값을 감싸서 clone 비용 최소화
/// - 근사 LRU (샘플링 기반) 으로 성능과 정확도 균형
pub struct ConcurrentLruCache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// 캐시 저장소
    cache: DashMap<K, CacheValue<V>>,
    /// LRU 순서 추적 (근사 LRU)
    lru_queue: RwLock<VecDeque<LruEntry<K>>>,
    /// 최대 크기
    max_size: usize,
    /// 통계
    stats: Arc<ConcurrentCacheStats>,
}

/// 캐시 값 (메타데이터 포함)
#[derive(Debug, Clone)]
struct CacheValue<V> {
    /// 실제 값 (Arc로 공유)
    value: Arc<V>,
    /// 마지막 접근 시간
    last_access: Instant,
}

impl<K, V> ConcurrentLruCache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// 새 캐시 생성
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: DashMap::with_capacity(max_size.min(1000)),
            lru_queue: RwLock::new(VecDeque::with_capacity(max_size.min(1000))),
            max_size,
            stats: Arc::new(ConcurrentCacheStats::new()),
        }
    }

    /// 값 조회 (Arc 반환)
    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        match self.cache.get_mut(key) {
            Some(mut entry) => {
                entry.last_access = Instant::now();
                self.stats.record_hit();
                Some(Arc::clone(&entry.value))
            }
            _ => {
                self.stats.record_miss();
                None
            }
        }
    }

    /// 값 조회 (clone 반환)
    pub fn get_clone(&self, key: &K) -> Option<V> {
        self.get(key).map(|arc| (*arc).clone())
    }

    /// 값 존재 여부 확인
    pub fn contains(&self, key: &K) -> bool {
        self.cache.contains_key(key)
    }

    /// 값 삽입
    pub fn put(&self, key: K, value: V) {
        let now = Instant::now();

        // 새 항목인 경우에만 크기 확인
        if !self.cache.contains_key(&key) && self.cache.len() >= self.max_size {
            self.evict_lru();
        }

        let cache_value = CacheValue {
            value: Arc::new(value),
            last_access: now,
        };

        self.cache.insert(key.clone(), cache_value);
        self.stats.record_insert();

        // LRU 큐 업데이트
        let mut lru = self.lru_queue.write();
        lru.push_back(LruEntry {
            key,
            last_access: now,
        });

        // 큐 크기 관리 (캐시 크기의 2배까지 허용)
        if lru.len() > self.max_size * 2 {
            self.compact_lru_queue(&mut lru);
        }
    }

    /// 값 삽입 (Arc로)
    pub fn put_arc(&self, key: K, value: Arc<V>) {
        let now = Instant::now();

        if !self.cache.contains_key(&key) && self.cache.len() >= self.max_size {
            self.evict_lru();
        }

        let cache_value = CacheValue {
            value,
            last_access: now,
        };

        self.cache.insert(key.clone(), cache_value);
        self.stats.record_insert();

        let mut lru = self.lru_queue.write();
        lru.push_back(LruEntry {
            key,
            last_access: now,
        });
    }

    /// 특정 키 삭제
    pub fn remove(&self, key: &K) -> Option<Arc<V>> {
        match self.cache.remove(key) {
            Some((_, entry)) => {
                self.stats.record_invalidation();
                Some(entry.value)
            }
            _ => None,
        }
    }

    /// 조건에 맞는 항목 삭제
    pub fn remove_if<F>(&self, predicate: F) -> usize
    where
        F: Fn(&K, &V) -> bool,
    {
        let keys_to_remove: Vec<K> = self
            .cache
            .iter()
            .filter(|entry| predicate(entry.key(), &entry.value().value))
            .map(|entry| entry.key().clone())
            .collect();

        let count = keys_to_remove.len();
        for key in keys_to_remove {
            self.cache.remove(&key);
        }

        for _ in 0..count {
            self.stats.record_invalidation();
        }

        count
    }

    /// 전체 삭제
    pub fn clear(&self) {
        let count = self.cache.len();
        self.cache.clear();
        self.lru_queue.write().clear();

        for _ in 0..count {
            self.stats.record_invalidation();
        }
    }

    /// 캐시 크기
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// 최대 크기
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// 통계 참조
    pub fn stats(&self) -> &ConcurrentCacheStats {
        &self.stats
    }

    /// 통계 리셋
    pub fn reset_stats(&self) {
        self.stats.reset();
    }

    /// LRU 축출 (샘플링 기반)
    fn evict_lru(&self) {
        let mut lru = self.lru_queue.write();

        // 큐에서 실제 존재하는 가장 오래된 항목 찾기
        while let Some(entry) = lru.pop_front() {
            if self.cache.contains_key(&entry.key) {
                // 마지막 접근 시간 확인
                if let Some(cache_entry) = self.cache.get(&entry.key) {
                    // 큐의 시간과 실제 시간이 다르면 (중간에 접근됨) 스킵
                    if cache_entry.last_access > entry.last_access {
                        continue;
                    }
                }

                // 삭제
                self.cache.remove(&entry.key);
                self.stats.record_eviction();
                return;
            }
        }

        // 큐가 비어있으면 임의 삭제 (fallback)
        drop(lru);
        if let Some(entry) = self.cache.iter().next() {
            let key = entry.key().clone();
            drop(entry);
            self.cache.remove(&key);
            self.stats.record_eviction();
        }
    }

    /// LRU 큐 압축 (삭제된 항목 제거)
    fn compact_lru_queue(&self, lru: &mut VecDeque<LruEntry<K>>) {
        let retained: VecDeque<_> = lru
            .drain(..)
            .filter(|entry| self.cache.contains_key(&entry.key))
            .collect();
        *lru = retained;
    }
}

/// 모든 키 반복자
impl<K, V> ConcurrentLruCache<K, V>
where
    K: Eq + Hash + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
{
    /// 모든 키 반환
    pub fn keys(&self) -> Vec<K> {
        self.cache.iter().map(|e| e.key().clone()).collect()
    }

    /// 키-값 쌍 반복
    pub fn iter(&self) -> impl Iterator<Item = (K, Arc<V>)> + '_ {
        self.cache
            .iter()
            .map(|e| (e.key().clone(), Arc::clone(&e.value)))
    }
}
