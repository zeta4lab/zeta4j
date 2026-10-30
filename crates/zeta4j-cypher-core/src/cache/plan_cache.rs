//! 실행 계획 캐시 모듈

use super::entry::CacheEntry;
use super::key::QueryCacheKey;
use super::stats::CacheStats;
use crate::ast::QueryPlan;
use std::collections::HashMap;

/// 실행 계획 캐시
///
/// AST + 파라미터 → 실행 계획 매핑을 캐싱합니다.
/// LRU(Least Recently Used) 정책으로 관리됩니다.
pub struct PlanCache {
    /// 캐시 저장소
    cache: HashMap<QueryCacheKey, CacheEntry<QueryPlan>>,
    /// 쿼리 해시 → 레이블 매핑 (무효화용)
    query_labels: HashMap<u64, Vec<String>>,
    /// 최대 크기
    max_size: usize,
    /// 통계
    stats: CacheStats,
}

impl PlanCache {
    /// 새 PlanCache 생성
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::with_capacity(max_size.min(100)),
            query_labels: HashMap::new(),
            max_size,
            stats: CacheStats::new(),
        }
    }

    /// 실행 계획 조회
    pub fn get(&mut self, query: &str, param_types: &[&str]) -> Option<QueryPlan> {
        let key = QueryCacheKey::from_query_with_params(query, param_types);

        if let Some(entry) = self.cache.get_mut(&key) {
            entry.touch();
            self.stats.record_hit();
            Some(entry.clone_value())
        } else {
            self.stats.record_miss();
            None
        }
    }

    /// 실행 계획 조회 (참조 반환)
    pub fn get_ref(&mut self, query: &str, param_types: &[&str]) -> Option<&QueryPlan> {
        let key = QueryCacheKey::from_query_with_params(query, param_types);

        if !self.cache.contains_key(&key) {
            self.stats.record_miss();
            return None;
        }

        if let Some(entry) = self.cache.get_mut(&key) {
            entry.touch();
            self.stats.record_hit();
        }

        self.cache.get(&key).map(|e| e.value())
    }

    /// 실행 계획 저장
    pub fn put(&mut self, query: &str, param_types: &[&str], plan: QueryPlan) {
        self.put_with_labels(query, param_types, plan, &[])
    }

    /// 실행 계획 저장 (레이블 정보 포함)
    pub fn put_with_labels(
        &mut self,
        query: &str,
        param_types: &[&str],
        plan: QueryPlan,
        labels: &[String],
    ) {
        let key = QueryCacheKey::from_query_with_params(query, param_types);

        // 캐시가 가득 차면 LRU 축출
        if self.cache.len() >= self.max_size && !self.cache.contains_key(&key) {
            self.evict_lru();
        }

        // 레이블 매핑 저장 (무효화용)
        if !labels.is_empty() {
            self.query_labels.insert(key.query_hash(), labels.to_vec());
        }

        self.cache.insert(key, CacheEntry::new(plan));
        self.stats.record_insert();
    }

    /// 특정 쿼리 무효화
    pub fn invalidate(&mut self, query: &str) {
        let base_key = QueryCacheKey::from_query(query);
        let query_hash = base_key.query_hash();

        // 동일 쿼리 해시를 가진 모든 엔트리 삭제
        let keys_to_remove: Vec<_> = self
            .cache
            .keys()
            .filter(|k| k.query_hash() == query_hash)
            .cloned()
            .collect();

        let count = keys_to_remove.len();
        for key in keys_to_remove {
            self.cache.remove(&key);
        }

        // 레이블 매핑도 삭제
        self.query_labels.remove(&query_hash);

        if count > 0 {
            self.stats.record_invalidations(count as u64);
        }
    }

    /// 레이블 관련 무효화
    pub fn invalidate_by_label(&mut self, label: &str) {
        // 해당 레이블을 사용하는 쿼리 해시 찾기
        let query_hashes: Vec<_> = self
            .query_labels
            .iter()
            .filter(|(_, labels)| labels.iter().any(|l| l == label))
            .map(|(hash, _)| *hash)
            .collect();

        let mut total_removed = 0;

        for query_hash in &query_hashes {
            // 해당 쿼리 해시를 가진 모든 엔트리 삭제
            let keys_to_remove: Vec<_> = self
                .cache
                .keys()
                .filter(|k| k.query_hash() == *query_hash)
                .cloned()
                .collect();

            total_removed += keys_to_remove.len();

            for key in keys_to_remove {
                self.cache.remove(&key);
            }

            self.query_labels.remove(query_hash);
        }

        if total_removed > 0 {
            self.stats.record_invalidations(total_removed as u64);
        }
    }

    /// 전체 무효화
    pub fn invalidate_all(&mut self) {
        let count = self.cache.len() as u64;
        self.cache.clear();
        self.query_labels.clear();
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
