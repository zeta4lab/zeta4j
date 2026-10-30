//! 쿼리 캐시 관리자 모듈

use super::config::CacheConfig;
use super::invalidation::{InvalidationEvent, InvalidationResult};
use super::parse_cache::ParseCache;
use super::plan_cache::PlanCache;
use super::stats::CombinedCacheStats;
use crate::ast::{Query, QueryPlan};
use zeta4j_graph::error::Result;

/// 쿼리 캐시 관리자
///
/// 파싱 캐시와 실행 계획 캐시를 통합 관리합니다.
pub struct QueryCacheManager {
    /// 파싱 캐시
    parse_cache: ParseCache,
    /// 실행 계획 캐시
    plan_cache: PlanCache,
    /// 설정
    config: CacheConfig,
}

impl QueryCacheManager {
    /// 새 관리자 생성
    pub fn new(config: CacheConfig) -> Self {
        Self {
            parse_cache: ParseCache::new(config.parse_cache_size),
            plan_cache: PlanCache::new(config.plan_cache_size),
            config,
        }
    }

    /// 기본 설정으로 관리자 생성
    pub fn with_defaults() -> Self {
        Self::new(CacheConfig::default())
    }

    /// 파싱된 AST 조회/저장
    ///
    /// 캐시에 있으면 반환, 없으면 parse_fn 호출 후 캐싱
    pub fn get_or_parse<F>(&mut self, query: &str, parse_fn: F) -> Result<Query>
    where
        F: FnOnce(&str) -> Result<Query>,
    {
        if !self.config.enable_parse_cache {
            return parse_fn(query);
        }

        if let Some(cached) = self.parse_cache.get(query) {
            return Ok(cached);
        }

        let ast = parse_fn(query)?;
        self.parse_cache.put(query, ast.clone());
        Ok(ast)
    }

    /// 실행 계획 조회/생성
    ///
    /// 캐시에 있으면 반환, 없으면 plan_fn 호출 후 캐싱
    pub fn get_or_plan<F>(
        &mut self,
        query: &str,
        param_types: &[&str],
        plan_fn: F,
    ) -> Result<QueryPlan>
    where
        F: FnOnce(&str, &[&str]) -> Result<QueryPlan>,
    {
        if !self.config.enable_plan_cache {
            return plan_fn(query, param_types);
        }

        if let Some(cached) = self.plan_cache.get(query, param_types) {
            return Ok(cached);
        }

        let plan = plan_fn(query, param_types)?;
        self.plan_cache.put(query, param_types, plan.clone());
        Ok(plan)
    }

    /// 실행 계획 조회/생성 (레이블 정보 포함)
    pub fn get_or_plan_with_labels<F>(
        &mut self,
        query: &str,
        param_types: &[&str],
        labels: &[String],
        plan_fn: F,
    ) -> Result<QueryPlan>
    where
        F: FnOnce(&str, &[&str]) -> Result<QueryPlan>,
    {
        if !self.config.enable_plan_cache {
            return plan_fn(query, param_types);
        }

        if let Some(cached) = self.plan_cache.get(query, param_types) {
            return Ok(cached);
        }

        let plan = plan_fn(query, param_types)?;
        self.plan_cache
            .put_with_labels(query, param_types, plan.clone(), labels);
        Ok(plan)
    }

    /// 무효화 이벤트 처리
    pub fn handle_invalidation(&mut self, event: InvalidationEvent) -> InvalidationResult {
        let mut result = InvalidationResult::new(&event);

        if event.requires_full_invalidation() {
            // 전체 무효화
            result.parse_invalidated = self.parse_cache.len();
            result.plan_invalidated = self.plan_cache.len();
            self.parse_cache.invalidate_all();
            self.plan_cache.invalidate_all();
        } else {
            // 부분 무효화
            match &event {
                InvalidationEvent::Manual { query: Some(q) } => {
                    let before_parse = self.parse_cache.len();
                    let before_plan = self.plan_cache.len();
                    self.parse_cache.invalidate(q);
                    self.plan_cache.invalidate(q);
                    result.parse_invalidated = before_parse - self.parse_cache.len();
                    result.plan_invalidated = before_plan - self.plan_cache.len();
                }
                InvalidationEvent::IndexChange { label, .. }
                | InvalidationEvent::BulkNodeChange { label } => {
                    let before = self.plan_cache.len();
                    self.plan_cache.invalidate_by_label(label);
                    result.plan_invalidated = before - self.plan_cache.len();
                }
                InvalidationEvent::StatsChange { labels }
                    if self.config.auto_invalidate_on_stats =>
                {
                    let before = self.plan_cache.len();
                    for label in labels {
                        self.plan_cache.invalidate_by_label(label);
                    }
                    result.plan_invalidated = before - self.plan_cache.len();
                }
                _ => {}
            }
        }

        result
    }

    /// 스키마 변경 시 무효화
    pub fn on_schema_change(&mut self) -> InvalidationResult {
        self.handle_invalidation(InvalidationEvent::schema_change())
    }

    /// 인덱스 변경 시 무효화
    pub fn on_index_change(&mut self, label: &str, property: &str) -> InvalidationResult {
        self.handle_invalidation(InvalidationEvent::index_change(label, property))
    }

    /// 통계 변경 시 무효화
    pub fn on_stats_change(&mut self, labels: Vec<String>) -> InvalidationResult {
        self.handle_invalidation(InvalidationEvent::stats_change(labels))
    }

    /// 캐시 통계 조회
    pub fn stats(&self) -> CombinedCacheStats {
        CombinedCacheStats {
            parse: self.parse_cache.stats().clone(),
            plan: self.plan_cache.stats().clone(),
        }
    }

    /// 통계 리셋
    pub fn reset_stats(&mut self) {
        self.parse_cache.reset_stats();
        self.plan_cache.reset_stats();
    }

    /// 캐시 클리어
    pub fn clear(&mut self) {
        self.parse_cache.invalidate_all();
        self.plan_cache.invalidate_all();
    }

    /// 파싱 캐시 크기
    pub fn parse_cache_size(&self) -> usize {
        self.parse_cache.len()
    }

    /// 실행 계획 캐시 크기
    pub fn plan_cache_size(&self) -> usize {
        self.plan_cache.len()
    }

    /// 설정 반환
    pub fn config(&self) -> &CacheConfig {
        &self.config
    }
}

impl Default for QueryCacheManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}
