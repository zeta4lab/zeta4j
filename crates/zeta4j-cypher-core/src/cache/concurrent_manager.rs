//! 동시성 쿼리 캐시 관리자
//!
//! 스레드-안전한 쿼리 파싱 및 실행 계획 캐시를 제공합니다.
//! `QueryExecutor`에 통합되어 반복 쿼리 실행 성능을 향상시킵니다.

use std::collections::HashSet;
use std::sync::Arc;

use dashmap::DashMap;

use super::concurrent_cache::{CacheStatsSnapshot, ConcurrentLruCache};
use super::config::CacheConfig;
use super::key::QueryCacheKey;
use crate::ast::{Query, QueryPlan};

/// 동시성 쿼리 캐시 관리자
///
/// 파싱 캐시와 실행 계획 캐시를 동시성 환경에서 안전하게 관리합니다.
/// `&self` 메서드만 제공하여 `Arc<ConcurrentQueryCacheManager>`로 공유할 수 있습니다.
///
/// # 캐시 계층
///
/// 1. **ParseCache**: 쿼리 문자열 → AST 매핑
/// 2. **PlanCache**: 쿼리 문자열 + 파라미터 타입 → 실행 계획 매핑
///
/// # 무효화 전략
///
/// - `invalidate_by_label(label)`: 해당 라벨을 사용하는 모든 쿼리 플랜 무효화
/// - `invalidate_all()`: 전체 캐시 무효화 (스키마 변경 시)
///
/// # 예제
///
/// ```ignore
/// use crate::cache::{ConcurrentQueryCacheManager, CacheConfig};
///
/// let manager = ConcurrentQueryCacheManager::new(CacheConfig::default());
///
/// // 파싱 캐시 활용
/// let ast = manager.get_or_parse("MATCH (n) RETURN n", || {
///     parse_query("MATCH (n) RETURN n")
/// })?;
///
/// // 통계 확인
/// let stats = manager.stats();
/// println!("Parse hit ratio: {:.2}%", stats.parse_hit_ratio * 100.0);
/// ```
pub struct ConcurrentQueryCacheManager {
    /// 파싱 캐시: 쿼리 문자열 → AST
    parse_cache: ConcurrentLruCache<QueryCacheKey, Query>,
    /// 실행 계획 캐시: 쿼리 + 파라미터 타입 → QueryPlan
    plan_cache: ConcurrentLruCache<QueryCacheKey, QueryPlan>,
    /// 라벨 → 쿼리 해시 매핑 (무효화용)
    label_index: DashMap<String, HashSet<u64>>,
    /// 캐시 설정
    config: CacheConfig,
}

/// 캐시 통계 요약
#[derive(Debug, Clone, Default)]
pub struct QueryCacheStats {
    /// 파싱 캐시 통계
    pub parse_stats: CacheStatsSnapshot,
    /// 실행 계획 캐시 통계
    pub plan_stats: CacheStatsSnapshot,
    /// 파싱 캐시 크기
    pub parse_cache_size: usize,
    /// 플랜 캐시 크기
    pub plan_cache_size: usize,
    /// 인덱싱된 라벨 수
    pub indexed_labels: usize,
}

impl QueryCacheStats {
    /// 파싱 캐시 히트율
    pub fn parse_hit_ratio(&self) -> f64 {
        self.parse_stats.hit_rate()
    }

    /// 플랜 캐시 히트율
    pub fn plan_hit_ratio(&self) -> f64 {
        self.plan_stats.hit_rate()
    }

    /// 전체 히트율 (가중 평균)
    pub fn total_hit_ratio(&self) -> f64 {
        let parse_total = self.parse_stats.hits + self.parse_stats.misses;
        let plan_total = self.plan_stats.hits + self.plan_stats.misses;
        let total = parse_total + plan_total;

        if total == 0 {
            0.0
        } else {
            let total_hits = self.parse_stats.hits + self.plan_stats.hits;
            total_hits as f64 / total as f64
        }
    }
}

impl ConcurrentQueryCacheManager {
    /// 새 캐시 관리자 생성
    pub fn new(config: CacheConfig) -> Self {
        Self {
            parse_cache: ConcurrentLruCache::new(config.parse_cache_size),
            plan_cache: ConcurrentLruCache::new(config.plan_cache_size),
            label_index: DashMap::new(),
            config,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(CacheConfig::default())
    }

    /// 설정 반환
    pub fn config(&self) -> &CacheConfig {
        &self.config
    }

    /// 파싱 캐시 조회 또는 파싱 실행
    ///
    /// 캐시에 AST가 있으면 반환하고, 없으면 `parse_fn`을 실행하여 캐싱합니다.
    ///
    /// # 인자
    ///
    /// * `query` - 쿼리 문자열
    /// * `parse_fn` - 캐시 미스 시 실행할 파싱 함수
    ///
    /// # 예제
    ///
    /// ```ignore
    /// let ast = manager.get_or_parse("MATCH (n) RETURN n", || {
    ///     parse_query("MATCH (n) RETURN n").map(|(_, q)| q)
    /// })?;
    /// ```
    pub fn get_or_parse<F, E>(&self, query: &str, parse_fn: F) -> Result<Arc<Query>, E>
    where
        F: FnOnce() -> Result<Query, E>,
    {
        if !self.config.enable_parse_cache {
            return parse_fn().map(Arc::new);
        }

        let key = QueryCacheKey::from_query(query);

        // 캐시 히트
        if let Some(cached) = self.parse_cache.get(&key) {
            return Ok(cached);
        }

        // 캐시 미스: 파싱 실행
        let ast = parse_fn()?;
        let arc_ast = Arc::new(ast);
        self.parse_cache.put_arc(key, Arc::clone(&arc_ast));
        Ok(arc_ast)
    }

    /// 실행 계획 캐시 조회 또는 계획 생성
    ///
    /// 캐시에 실행 계획이 있으면 반환하고, 없으면 `plan_fn`을 실행하여 캐싱합니다.
    /// 캐싱 시 쿼리에서 사용하는 라벨을 추출하여 인덱싱합니다.
    ///
    /// # 인자
    ///
    /// * `query` - 쿼리 문자열
    /// * `param_types` - 파라미터 타입 시그니처 (같은 쿼리라도 타입 다르면 다른 플랜)
    /// * `labels` - 쿼리에서 사용하는 라벨 목록 (무효화용)
    /// * `plan_fn` - 캐시 미스 시 실행할 플래닝 함수
    ///
    /// # 예제
    ///
    /// ```ignore
    /// let plan = manager.get_or_plan(
    ///     "MATCH (n:Person) RETURN n",
    ///     &[],
    ///     &["Person".to_string()],
    ///     || planner.plan(&ast),
    /// )?;
    /// ```
    pub fn get_or_plan<F, E>(
        &self,
        query: &str,
        param_types: &[&str],
        labels: &[String],
        plan_fn: F,
    ) -> Result<Arc<QueryPlan>, E>
    where
        F: FnOnce() -> Result<QueryPlan, E>,
    {
        if !self.config.enable_plan_cache {
            return plan_fn().map(Arc::new);
        }

        let key = QueryCacheKey::from_query_with_params(query, param_types);

        // 캐시 히트
        if let Some(cached) = self.plan_cache.get(&key) {
            return Ok(cached);
        }

        // 캐시 미스: 플래닝 실행
        let plan = plan_fn()?;
        let arc_plan = Arc::new(plan);
        self.plan_cache.put_arc(key.clone(), Arc::clone(&arc_plan));

        // 라벨 인덱스 업데이트 (무효화용)
        let query_hash = key.query_hash();
        for label in labels {
            self.label_index
                .entry(label.clone())
                .or_default()
                .insert(query_hash);
        }

        Ok(arc_plan)
    }

    /// 특정 라벨을 사용하는 모든 쿼리 플랜 무효화
    ///
    /// 인덱스 생성/삭제 시 호출하여 해당 라벨 관련 플랜을 무효화합니다.
    ///
    /// # 인자
    ///
    /// * `label` - 무효화할 라벨 이름
    pub fn invalidate_by_label(&self, label: &str) {
        // 해당 라벨의 모든 쿼리 해시 가져오기
        if let Some(hashes) = self.label_index.get(label) {
            let hashes_to_remove: Vec<u64> = hashes.iter().copied().collect();

            // 해당 해시를 가진 모든 플랜 캐시 엔트리 삭제
            for hash in &hashes_to_remove {
                // 모든 키 순회하며 해당 해시와 매칭되는 항목 삭제
                self.plan_cache.remove_if(|k, _| k.query_hash() == *hash);
            }
        }

        // 라벨 인덱스에서도 제거
        self.label_index.remove(label);
    }

    /// 전체 캐시 무효화
    ///
    /// 스키마 변경, 제약조건 변경 등 전체 무효화가 필요한 경우 호출합니다.
    pub fn invalidate_all(&self) {
        self.parse_cache.clear();
        self.plan_cache.clear();
        self.label_index.clear();
    }

    /// 파싱 캐시만 무효화
    pub fn invalidate_parse_cache(&self) {
        self.parse_cache.clear();
    }

    /// 플랜 캐시만 무효화
    pub fn invalidate_plan_cache(&self) {
        self.plan_cache.clear();
        self.label_index.clear();
    }

    /// 캐시 통계 반환
    pub fn stats(&self) -> QueryCacheStats {
        QueryCacheStats {
            parse_stats: self.parse_cache.stats().snapshot(),
            plan_stats: self.plan_cache.stats().snapshot(),
            parse_cache_size: self.parse_cache.len(),
            plan_cache_size: self.plan_cache.len(),
            indexed_labels: self.label_index.len(),
        }
    }

    /// 통계 리셋
    pub fn reset_stats(&self) {
        self.parse_cache.reset_stats();
        self.plan_cache.reset_stats();
    }

    /// 파싱 캐시 크기
    pub fn parse_cache_size(&self) -> usize {
        self.parse_cache.len()
    }

    /// 플랜 캐시 크기
    pub fn plan_cache_size(&self) -> usize {
        self.plan_cache.len()
    }

    /// 캐싱 활성화 여부
    pub fn is_caching_enabled(&self) -> bool {
        self.config.is_caching_enabled()
    }
}
