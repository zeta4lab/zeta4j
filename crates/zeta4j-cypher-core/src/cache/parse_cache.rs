// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 파싱 캐시 모듈

use super::entry::CacheEntry;
use super::key::QueryCacheKey;
use super::stats::CacheStats;
use crate::ast::Query;
use std::collections::HashMap;

/// 파싱 캐시
///
/// 쿼리 문자열 → AST 매핑을 캐싱합니다.
/// LRU(Least Recently Used) 정책으로 관리됩니다.
pub struct ParseCache {
    /// 캐시 저장소
    cache: HashMap<QueryCacheKey, CacheEntry<Query>>,
    /// 최대 크기
    max_size: usize,
    /// 통계
    stats: CacheStats,
}

impl ParseCache {
    /// 새 ParseCache 생성
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::with_capacity(max_size.min(100)),
            max_size,
            stats: CacheStats::new(),
        }
    }

    /// AST 조회
    pub fn get(&mut self, query: &str) -> Option<Query> {
        let key = QueryCacheKey::from_query(query);

        if let Some(entry) = self.cache.get_mut(&key) {
            entry.touch();
            self.stats.record_hit();
            Some(entry.clone_value())
        } else {
            self.stats.record_miss();
            None
        }
    }

    /// AST 조회 (참조 반환)
    pub fn get_ref(&mut self, query: &str) -> Option<&Query> {
        let key = QueryCacheKey::from_query(query);

        // 먼저 존재 여부 확인
        if !self.cache.contains_key(&key) {
            self.stats.record_miss();
            return None;
        }

        // entry를 가져와서 touch 후 참조 반환
        if let Some(entry) = self.cache.get_mut(&key) {
            entry.touch();
            self.stats.record_hit();
        }

        self.cache.get(&key).map(|e| e.value())
    }

    /// AST 저장
    pub fn put(&mut self, query: &str, ast: Query) {
        let key = QueryCacheKey::from_query(query);

        // 캐시가 가득 차면 LRU 축출
        if self.cache.len() >= self.max_size && !self.cache.contains_key(&key) {
            self.evict_lru();
        }

        self.cache.insert(key, CacheEntry::new(ast));
        self.stats.record_insert();
    }

    /// 특정 쿼리 무효화
    pub fn invalidate(&mut self, query: &str) {
        let key = QueryCacheKey::from_query(query);
        if self.cache.remove(&key).is_some() {
            self.stats.record_invalidation();
        }
    }

    /// 전체 무효화
    pub fn invalidate_all(&mut self) {
        let count = self.cache.len() as u64;
        self.cache.clear();
        self.stats.record_invalidations(count);
    }

    /// 캐시 크기
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// 캐시 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// 최대 크기
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// 통계 반환
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// 통계 리셋
    pub fn reset_stats(&mut self) {
        self.stats.reset();
    }

    /// LRU 축출
    fn evict_lru(&mut self) {
        if let Some(lru_key) = self.find_lru_key() {
            self.cache.remove(&lru_key);
            self.stats.record_eviction();
        }
    }

    /// LRU 키 찾기
    fn find_lru_key(&self) -> Option<QueryCacheKey> {
        self.cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed())
            .map(|(key, _)| key.clone())
    }
}
