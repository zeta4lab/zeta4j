// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 캐시 통계 모듈

use std::fmt;

/// 캐시 통계
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// 히트 수
    pub hits: u64,
    /// 미스 수
    pub misses: u64,
    /// 삽입 수
    pub inserts: u64,
    /// 축출 수 (LRU eviction)
    pub evictions: u64,
    /// 무효화 수
    pub invalidations: u64,
}

impl CacheStats {
    /// 새 통계 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 히트 기록
    pub fn record_hit(&mut self) {
        self.hits += 1;
    }

    /// 미스 기록
    pub fn record_miss(&mut self) {
        self.misses += 1;
    }

    /// 삽입 기록
    pub fn record_insert(&mut self) {
        self.inserts += 1;
    }

    /// 축출 기록
    pub fn record_eviction(&mut self) {
        self.evictions += 1;
    }

    /// 무효화 기록
    pub fn record_invalidation(&mut self) {
        self.invalidations += 1;
    }

    /// 다중 무효화 기록
    pub fn record_invalidations(&mut self, count: u64) {
        self.invalidations += count;
    }

    /// 총 요청 수
    pub fn total_requests(&self) -> u64 {
        self.hits + self.misses
    }

    /// 히트율 계산 (0.0 ~ 1.0)
    pub fn hit_ratio(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// 미스율 계산 (0.0 ~ 1.0)
    pub fn miss_ratio(&self) -> f64 {
        1.0 - self.hit_ratio()
    }

    /// 통계 리셋
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 다른 통계와 병합
    pub fn merge(&mut self, other: &CacheStats) {
        self.hits += other.hits;
        self.misses += other.misses;
        self.inserts += other.inserts;
        self.evictions += other.evictions;
        self.invalidations += other.invalidations;
    }
}

impl fmt::Display for CacheStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "CacheStats {{ hits: {}, misses: {}, hit_ratio: {:.2}%, inserts: {}, evictions: {}, invalidations: {} }}",
            self.hits,
            self.misses,
            self.hit_ratio() * 100.0,
            self.inserts,
            self.evictions,
            self.invalidations
        )
    }
}

/// 통합 캐시 통계
#[derive(Debug, Clone, Default)]
pub struct CombinedCacheStats {
    /// 파싱 캐시 통계
    pub parse: CacheStats,
    /// 실행 계획 캐시 통계
    pub plan: CacheStats,
}

impl CombinedCacheStats {
    /// 새 통합 통계 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 통합 히트율
    pub fn total_hit_ratio(&self) -> f64 {
        let total_hits = self.parse.hits + self.plan.hits;
        let total_requests = self.parse.total_requests() + self.plan.total_requests();

        if total_requests == 0 {
            0.0
        } else {
            total_hits as f64 / total_requests as f64
        }
    }

    /// 통계 리셋
    pub fn reset(&mut self) {
        self.parse.reset();
        self.plan.reset();
    }
}

impl fmt::Display for CombinedCacheStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Query Cache Statistics:")?;
        writeln!(f, "  Parse Cache: {}", self.parse)?;
        writeln!(f, "  Plan Cache:  {}", self.plan)?;
        write!(
            f,
            "  Total hit ratio: {:.2}%",
            self.total_hit_ratio() * 100.0
        )
    }
}
