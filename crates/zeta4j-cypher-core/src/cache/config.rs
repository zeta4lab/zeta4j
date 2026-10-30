//! 캐시 설정 모듈

/// 캐시 설정
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// ParseCache 활성화
    pub enable_parse_cache: bool,
    /// PlanCache 활성화
    pub enable_plan_cache: bool,
    /// ParseCache 최대 크기
    pub parse_cache_size: usize,
    /// PlanCache 최대 크기
    pub plan_cache_size: usize,
    /// 통계 변경 시 자동 무효화
    pub auto_invalidate_on_stats: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enable_parse_cache: true,
            enable_plan_cache: true,
            parse_cache_size: 1000,
            plan_cache_size: 500,
            auto_invalidate_on_stats: true,
        }
    }
}

impl CacheConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 캐시 비활성화 설정
    pub fn disabled() -> Self {
        Self {
            enable_parse_cache: false,
            enable_plan_cache: false,
            ..Self::default()
        }
    }

    /// 파싱 캐시만 활성화
    pub fn parse_only() -> Self {
        Self {
            enable_parse_cache: true,
            enable_plan_cache: false,
            ..Self::default()
        }
    }

    /// 실행 계획 캐시만 활성화
    pub fn plan_only() -> Self {
        Self {
            enable_parse_cache: false,
            enable_plan_cache: true,
            ..Self::default()
        }
    }

    /// 파싱 캐시 크기 설정
    pub fn with_parse_cache_size(mut self, size: usize) -> Self {
        self.parse_cache_size = size;
        self
    }

    /// 실행 계획 캐시 크기 설정
    pub fn with_plan_cache_size(mut self, size: usize) -> Self {
        self.plan_cache_size = size;
        self
    }

    /// 파싱 캐시 활성화 설정
    pub fn with_parse_cache(mut self, enabled: bool) -> Self {
        self.enable_parse_cache = enabled;
        self
    }

    /// 실행 계획 캐시 활성화 설정
    pub fn with_plan_cache(mut self, enabled: bool) -> Self {
        self.enable_plan_cache = enabled;
        self
    }

    /// 통계 변경 시 자동 무효화 설정
    pub fn with_auto_invalidate(mut self, enabled: bool) -> Self {
        self.auto_invalidate_on_stats = enabled;
        self
    }

    /// 캐시 활성화 여부
    pub fn is_caching_enabled(&self) -> bool {
        self.enable_parse_cache || self.enable_plan_cache
    }
}
