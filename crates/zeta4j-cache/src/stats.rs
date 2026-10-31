// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 캐시 통계

use std::sync::atomic::{AtomicU64, Ordering};

/// 캐시 통계
#[derive(Debug, Default)]
pub struct CacheStats {
    /// L1 히트 수
    l1_hits: AtomicU64,
    /// L2 히트 수
    l2_hits: AtomicU64,
    /// 미스 수 (L1, L2 모두 미스)
    misses: AtomicU64,
    /// 삽입 수
    inserts: AtomicU64,
    /// 무효화 수
    invalidations: AtomicU64,
}

impl CacheStats {
    /// 새 통계 객체 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// L1 히트 기록
    pub fn record_l1_hit(&self) {
        self.l1_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// L2 히트 기록
    pub fn record_l2_hit(&self) {
        self.l2_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// 미스 기록
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// 삽입 기록
    pub fn record_insert(&self) {
        self.inserts.fetch_add(1, Ordering::Relaxed);
    }

    /// 무효화 기록
    pub fn record_invalidation(&self) {
        self.invalidations.fetch_add(1, Ordering::Relaxed);
    }

    /// L1 히트 수
    pub fn l1_hits(&self) -> u64 {
        self.l1_hits.load(Ordering::Relaxed)
    }

    /// L2 히트 수
    pub fn l2_hits(&self) -> u64 {
        self.l2_hits.load(Ordering::Relaxed)
    }

    /// 미스 수
    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    /// 삽입 수
    pub fn inserts(&self) -> u64 {
        self.inserts.load(Ordering::Relaxed)
    }

    /// 무효화 수
    pub fn invalidations(&self) -> u64 {
        self.invalidations.load(Ordering::Relaxed)
    }

    /// 총 요청 수
    pub fn total_requests(&self) -> u64 {
        self.l1_hits() + self.l2_hits() + self.misses()
    }

    /// L1 히트율 (0.0 ~ 1.0)
    pub fn l1_hit_rate(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            0.0
        } else {
            self.l1_hits() as f64 / total as f64
        }
    }

    /// 전체 히트율 (L1 + L2) (0.0 ~ 1.0)
    pub fn total_hit_rate(&self) -> f64 {
        let total = self.total_requests();
        if total == 0 {
            0.0
        } else {
            (self.l1_hits() + self.l2_hits()) as f64 / total as f64
        }
    }

    /// 통계 초기화
    pub fn reset(&self) {
        self.l1_hits.store(0, Ordering::Relaxed);
        self.l2_hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.inserts.store(0, Ordering::Relaxed);
        self.invalidations.store(0, Ordering::Relaxed);
    }
}
