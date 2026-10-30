//! 캐시 설정

use std::time::Duration;

/// 캐시 설정
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// L1 캐시 최대 항목 수
    pub l1_max_capacity: u64,
    /// L1 TTL (Time to Live)
    pub l1_ttl: Duration,
    /// L2 활성화 여부
    pub l2_enabled: bool,
    /// 통계 수집 활성화
    pub stats_enabled: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            l1_max_capacity: 10_000,
            l1_ttl: Duration::from_secs(300), // 5분
            l2_enabled: true,
            stats_enabled: true,
        }
    }
}

impl CacheConfig {
    /// L1 전용 캐시 (L2 비활성화)
    pub fn l1_only() -> Self {
        Self {
            l2_enabled: false,
            ..Default::default()
        }
    }

    /// 인덱스 캐시용 설정 (긴 TTL, 작은 용량)
    pub fn for_index_cache() -> Self {
        Self {
            l1_max_capacity: 1_000,
            l1_ttl: Duration::from_secs(600), // 10분
            l2_enabled: true,
            stats_enabled: true,
        }
    }

    /// L1 최대 용량 설정
    pub fn with_l1_capacity(mut self, capacity: u64) -> Self {
        self.l1_max_capacity = capacity;
        self
    }

    /// L1 TTL 설정
    pub fn with_l1_ttl(mut self, ttl: Duration) -> Self {
        self.l1_ttl = ttl;
        self
    }

    /// L2 활성화/비활성화
    pub fn with_l2(mut self, enabled: bool) -> Self {
        self.l2_enabled = enabled;
        self
    }

    /// 통계 수집 활성화/비활성화
    pub fn with_stats(mut self, enabled: bool) -> Self {
        self.stats_enabled = enabled;
        self
    }
}
