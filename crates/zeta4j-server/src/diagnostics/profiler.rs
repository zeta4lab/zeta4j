//! 쿼리 프로파일러 모듈
//!
//! v1.61.0: 샘플링 기반 프로파일링 및 오버헤드 최적화
//! - VecDeque 기반 원형 버퍼 (O(1) 삽입/삭제)
//! - 적응형 샘플링 (정상 쿼리 샘플링, 느린 쿼리 100% 기록)
//! - 쿼리 해시 저장으로 메모리 최적화
//! - Development/Production 모드 지원

use std::collections::VecDeque;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// 프로파일링 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProfilingMode {
    /// 개발 모드: 모든 쿼리 기록, 전체 쿼리 문자열 저장
    #[default]
    Development,
    /// 프로덕션 모드: 샘플링 적용, 쿼리 해시만 저장
    Production,
    /// 비활성: 프로파일링 완전 비활성화
    Disabled,
}

/// 샘플링 설정
#[derive(Debug, Clone)]
pub struct SamplingConfig {
    /// 정상 쿼리 샘플링 비율 (0.0 ~ 1.0, 1.0 = 100%)
    pub normal_query_rate: f64,
    /// 느린 쿼리는 항상 100% 기록
    pub always_record_slow: bool,
    /// 인덱스 미사용 쿼리 항상 기록
    pub always_record_no_index: bool,
    /// 샘플링 시드 (재현 가능한 테스트용)
    seed: u64,
    /// 현재 카운터 (결정적 샘플링용)
    counter: u64,
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            normal_query_rate: 1.0, // 기본값: 100% 기록
            always_record_slow: true,
            always_record_no_index: true,
            seed: 0,
            counter: 0,
        }
    }
}

impl SamplingConfig {
    /// 프로덕션 최적화 설정 (1% 샘플링)
    pub fn production() -> Self {
        Self {
            normal_query_rate: 0.01, // 1% 샘플링
            always_record_slow: true,
            always_record_no_index: true,
            seed: 0,
            counter: 0,
        }
    }

    /// 커스텀 샘플링 비율 설정
    pub fn with_rate(mut self, rate: f64) -> Self {
        self.normal_query_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// 샘플링 여부 결정 (결정적 알고리즘)
    pub fn should_sample(&mut self) -> bool {
        if self.normal_query_rate >= 1.0 {
            return true;
        }
        if self.normal_query_rate <= 0.0 {
            return false;
        }

        self.counter = self.counter.wrapping_add(1);
        let threshold = (self.normal_query_rate * u64::MAX as f64) as u64;

        // 간단한 해시 기반 결정적 샘플링
        let mut hasher = DefaultHasher::new();
        (self.seed ^ self.counter).hash(&mut hasher);
        hasher.finish() < threshold
    }
}

/// 쿼리 실행 프로파일
#[derive(Debug, Clone)]
pub struct QueryProfile {
    /// 쿼리 문자열 (Development 모드에서만 저장)
    pub query: String,
    /// 쿼리 해시 (Production 모드에서 사용)
    pub query_hash: u64,
    /// 총 실행 시간 (마이크로초)
    pub total_time_us: u64,
    /// 파싱 시간 (마이크로초)
    pub parse_time_us: u64,
    /// 계획 생성 시간 (마이크로초)
    pub plan_time_us: u64,
    /// 실행 시간 (마이크로초)
    pub execution_time_us: u64,
    /// 스캔한 노드 수
    pub nodes_scanned: usize,
    /// 스캔한 엣지 수
    pub edges_scanned: usize,
    /// 반환된 행 수
    pub rows_returned: usize,
    /// 인덱스 사용 여부
    pub index_used: bool,
    /// 캐시 히트 수
    pub cache_hits: usize,
    /// 캐시 미스 수
    pub cache_misses: usize,
    /// 타임스탬프
    pub timestamp: u64,
}

/// 쿼리 해시 계산 헬퍼
fn compute_query_hash(query: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    hasher.finish()
}

impl QueryProfile {
    /// 새 QueryProfile 생성 (Development 모드용)
    pub fn new(query: String) -> Self {
        let query_hash = compute_query_hash(&query);
        Self {
            query,
            query_hash,
            total_time_us: 0,
            parse_time_us: 0,
            plan_time_us: 0,
            execution_time_us: 0,
            nodes_scanned: 0,
            edges_scanned: 0,
            rows_returned: 0,
            index_used: false,
            cache_hits: 0,
            cache_misses: 0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Production 모드용 생성 (해시만 저장, 쿼리 문자열 생략)
    pub fn new_production(query: &str) -> Self {
        Self {
            query: String::new(), // 메모리 절약
            query_hash: compute_query_hash(query),
            total_time_us: 0,
            parse_time_us: 0,
            plan_time_us: 0,
            execution_time_us: 0,
            nodes_scanned: 0,
            edges_scanned: 0,
            rows_returned: 0,
            index_used: false,
            cache_hits: 0,
            cache_misses: 0,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// 빌더 패턴: 총 시간 설정
    pub fn with_total_time(mut self, time_us: u64) -> Self {
        self.total_time_us = time_us;
        self
    }

    /// 빌더 패턴: 파싱 시간 설정
    pub fn with_parse_time(mut self, time_us: u64) -> Self {
        self.parse_time_us = time_us;
        self
    }

    /// 빌더 패턴: 계획 시간 설정
    pub fn with_plan_time(mut self, time_us: u64) -> Self {
        self.plan_time_us = time_us;
        self
    }

    /// 빌더 패턴: 실행 시간 설정
    pub fn with_execution_time(mut self, time_us: u64) -> Self {
        self.execution_time_us = time_us;
        self
    }

    /// 빌더 패턴: 노드 스캔 수 설정
    pub fn with_nodes_scanned(mut self, count: usize) -> Self {
        self.nodes_scanned = count;
        self
    }

    /// 빌더 패턴: 엣지 스캔 수 설정
    pub fn with_edges_scanned(mut self, count: usize) -> Self {
        self.edges_scanned = count;
        self
    }

    /// 빌더 패턴: 반환 행 수 설정
    pub fn with_rows_returned(mut self, count: usize) -> Self {
        self.rows_returned = count;
        self
    }

    /// 빌더 패턴: 인덱스 사용 여부 설정
    pub fn with_index_used(mut self, used: bool) -> Self {
        self.index_used = used;
        self
    }

    /// 빌더 패턴: 캐시 통계 설정
    pub fn with_cache_stats(mut self, hits: usize, misses: usize) -> Self {
        self.cache_hits = hits;
        self.cache_misses = misses;
        self
    }

    /// 캐시 히트율 계산
    pub fn cache_hit_ratio(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            1.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }

    /// 스캔 효율성 (반환 행 / 스캔 행)
    pub fn scan_efficiency(&self) -> f64 {
        let total_scanned = self.nodes_scanned + self.edges_scanned;
        if total_scanned == 0 {
            1.0
        } else {
            self.rows_returned as f64 / total_scanned as f64
        }
    }
}

/// 쿼리 통계
#[derive(Debug, Clone, Default)]
pub struct QueryStats {
    /// 총 쿼리 수
    pub total_queries: usize,
    /// 느린 쿼리 수
    pub slow_queries: usize,
    /// 평균 실행 시간 (마이크로초)
    pub avg_time_us: u64,
    /// 최대 실행 시간 (마이크로초)
    pub max_time_us: u64,
    /// 인덱스 사용 비율
    pub index_usage_ratio: f64,
    /// 평균 캐시 히트율
    pub avg_cache_hit_ratio: f64,
}

/// 런타임 통계 (캐싱)
#[derive(Debug, Clone, Default)]
struct CachedStats {
    /// 총 기록된 쿼리 수 (샘플링 전)
    total_seen: u64,
    /// 총 기록된 쿼리 수 (샘플링 후)
    total_recorded: u64,
    /// 느린 쿼리 수
    slow_count: u64,
    /// 인덱스 미사용 쿼리 수
    no_index_count: u64,
    /// 총 실행 시간 합계
    total_time_sum: u64,
    /// 최대 실행 시간
    max_time: u64,
    /// 캐시 히트 합계
    cache_hits_sum: u64,
    /// 캐시 미스 합계
    cache_misses_sum: u64,
    /// 인덱스 사용 쿼리 수
    index_used_count: u64,
    /// 통계 유효 여부
    dirty: bool,
}

/// 쿼리 프로파일러
///
/// v1.61.0 개선사항:
/// - VecDeque 기반 원형 버퍼 (O(1) 삽입/삭제)
/// - 샘플링 기반 프로파일링
/// - 느린 쿼리 전용 저장소
/// - 런타임 통계 캐싱
pub struct QueryProfiler {
    /// 프로파일 기록 (원형 버퍼)
    profiles: VecDeque<QueryProfile>,
    /// 느린 쿼리 전용 저장소
    slow_profiles: VecDeque<QueryProfile>,
    /// 최대 일반 프로파일 보관 개수
    max_profiles: usize,
    /// 최대 느린 쿼리 보관 개수
    max_slow_profiles: usize,
    /// 느린 쿼리 임계값 (마이크로초)
    slow_query_threshold_us: u64,
    /// 프로파일링 모드
    mode: ProfilingMode,
    /// 샘플링 설정
    sampling: SamplingConfig,
    /// 캐싱된 통계
    stats_cache: CachedStats,
}

impl QueryProfiler {
    /// 새 QueryProfiler 생성
    pub fn new(slow_query_threshold_us: u64) -> Self {
        Self {
            profiles: VecDeque::with_capacity(1000),
            slow_profiles: VecDeque::with_capacity(100),
            max_profiles: 1000,
            max_slow_profiles: 100,
            slow_query_threshold_us,
            mode: ProfilingMode::Development,
            sampling: SamplingConfig::default(),
            stats_cache: CachedStats::default(),
        }
    }

    /// 기본 설정으로 생성 (1초 = 1,000,000 마이크로초)
    pub fn with_defaults() -> Self {
        Self::new(1_000_000) // 1초
    }

    /// 프로덕션 모드로 생성 (최적화된 설정)
    pub fn production(slow_query_threshold_us: u64) -> Self {
        Self {
            profiles: VecDeque::with_capacity(100),
            slow_profiles: VecDeque::with_capacity(1000),
            max_profiles: 100,
            max_slow_profiles: 1000,
            slow_query_threshold_us,
            mode: ProfilingMode::Production,
            sampling: SamplingConfig::production(),
            stats_cache: CachedStats::default(),
        }
    }

    /// 최대 보관 개수 설정
    pub fn with_max_profiles(mut self, max: usize) -> Self {
        self.max_profiles = max;
        self
    }

    /// 최대 느린 쿼리 보관 개수 설정
    pub fn with_max_slow_profiles(mut self, max: usize) -> Self {
        self.max_slow_profiles = max;
        self
    }

    /// 프로파일링 모드 설정
    pub fn with_mode(mut self, mode: ProfilingMode) -> Self {
        self.mode = mode;
        self
    }

    /// 샘플링 설정
    pub fn with_sampling(mut self, sampling: SamplingConfig) -> Self {
        self.sampling = sampling;
        self
    }

    /// 현재 모드 조회
    pub fn mode(&self) -> ProfilingMode {
        self.mode
    }

    /// 프로파일 기록 (샘플링 및 모드 적용)
    pub fn record(&mut self, profile: QueryProfile) {
        // 비활성 모드면 즉시 반환
        if self.mode == ProfilingMode::Disabled {
            return;
        }

        // 통계 업데이트 (샘플링 전)
        self.stats_cache.total_seen += 1;
        self.stats_cache.dirty = true;

        let is_slow = profile.total_time_us >= self.slow_query_threshold_us;
        let is_no_index = !profile.index_used;

        // 샘플링 결정
        let should_record = (is_slow && self.sampling.always_record_slow)
            || (is_no_index && self.sampling.always_record_no_index)
            || self.sampling.should_sample();

        if !should_record {
            return;
        }

        // 프로덕션 모드에서 쿼리 문자열 제거
        let profile = if self.mode == ProfilingMode::Production && !profile.query.is_empty() {
            QueryProfile {
                query: String::new(),
                ..profile
            }
        } else {
            profile
        };

        // 통계 업데이트 (기록된 쿼리)
        self.stats_cache.total_recorded += 1;
        self.stats_cache.total_time_sum += profile.total_time_us;
        if profile.total_time_us > self.stats_cache.max_time {
            self.stats_cache.max_time = profile.total_time_us;
        }
        self.stats_cache.cache_hits_sum += profile.cache_hits as u64;
        self.stats_cache.cache_misses_sum += profile.cache_misses as u64;
        if profile.index_used {
            self.stats_cache.index_used_count += 1;
        }

        // 느린 쿼리는 별도 저장소에 저장
        if is_slow {
            self.stats_cache.slow_count += 1;
            self.slow_profiles.push_back(profile.clone());
            if self.slow_profiles.len() > self.max_slow_profiles {
                self.slow_profiles.pop_front(); // O(1)
            }
        }

        if is_no_index {
            self.stats_cache.no_index_count += 1;
        }

        // 일반 저장소에 저장
        self.profiles.push_back(profile);
        if self.profiles.len() > self.max_profiles {
            self.profiles.pop_front(); // O(1) - Vec.remove(0)은 O(n)이었음
        }
    }

    /// 모든 프로파일 조회 (슬라이스 호환)
    pub fn profiles(&self) -> Vec<&QueryProfile> {
        self.profiles.iter().collect()
    }

    /// 프로파일을 슬라이스로 조회 (make_contiguous 필요시)
    pub fn profiles_slice(&mut self) -> &mut [QueryProfile] {
        self.profiles.make_contiguous()
    }

    /// 프로파일 개수
    pub fn count(&self) -> usize {
        self.profiles.len()
    }

    /// 느린 쿼리 개수
    pub fn slow_count(&self) -> usize {
        self.slow_profiles.len()
    }

    /// 느린 쿼리 조회 (전용 저장소에서 O(1) 접근)
    pub fn slow_queries(&self) -> Vec<&QueryProfile> {
        self.slow_profiles.iter().collect()
    }

    /// 가장 느린 쿼리 N개 (느린 쿼리 저장소 우선 탐색)
    pub fn top_slow_queries(&self, n: usize) -> Vec<&QueryProfile> {
        let mut sorted: Vec<_> = self.slow_profiles.iter().collect();
        sorted.sort_by_key(|profile| std::cmp::Reverse(profile.total_time_us));
        sorted.into_iter().take(n).collect()
    }

    /// 인덱스 미사용 쿼리
    pub fn queries_without_index(&self) -> Vec<&QueryProfile> {
        self.profiles.iter().filter(|p| !p.index_used).collect()
    }

    /// 평균 실행 시간 (캐시 기반 O(1))
    pub fn average_execution_time(&self) -> f64 {
        if self.stats_cache.total_recorded == 0 {
            return 0.0;
        }
        self.stats_cache.total_time_sum as f64 / self.stats_cache.total_recorded as f64
    }

    /// 최대 실행 시간 (캐시 기반 O(1))
    pub fn max_execution_time(&self) -> u64 {
        self.stats_cache.max_time
    }

    /// 인덱스 사용 비율 (캐시 기반 O(1))
    pub fn index_usage_ratio(&self) -> f64 {
        if self.stats_cache.total_recorded == 0 {
            return 1.0;
        }
        self.stats_cache.index_used_count as f64 / self.stats_cache.total_recorded as f64
    }

    /// 평균 캐시 히트율 (캐시 기반 O(1))
    pub fn average_cache_hit_ratio(&self) -> f64 {
        let total = self.stats_cache.cache_hits_sum + self.stats_cache.cache_misses_sum;
        if total == 0 {
            return 1.0;
        }
        self.stats_cache.cache_hits_sum as f64 / total as f64
    }

    /// 샘플링 비율 (실제 기록된 비율)
    pub fn sampling_ratio(&self) -> f64 {
        if self.stats_cache.total_seen == 0 {
            return 1.0;
        }
        self.stats_cache.total_recorded as f64 / self.stats_cache.total_seen as f64
    }

    /// 총 관측된 쿼리 수 (샘플링 전)
    pub fn total_seen(&self) -> u64 {
        self.stats_cache.total_seen
    }

    /// 총 기록된 쿼리 수 (샘플링 후)
    pub fn total_recorded(&self) -> u64 {
        self.stats_cache.total_recorded
    }

    /// 쿼리 통계 생성 (캐시 기반 O(1))
    pub fn stats(&self) -> QueryStats {
        QueryStats {
            total_queries: self.stats_cache.total_recorded as usize,
            slow_queries: self.stats_cache.slow_count as usize,
            avg_time_us: self.average_execution_time() as u64,
            max_time_us: self.max_execution_time(),
            index_usage_ratio: self.index_usage_ratio(),
            avg_cache_hit_ratio: self.average_cache_hit_ratio(),
        }
    }

    /// 확장 통계 (샘플링 정보 포함)
    pub fn extended_stats(&self) -> ExtendedQueryStats {
        ExtendedQueryStats {
            base: self.stats(),
            total_seen: self.stats_cache.total_seen,
            total_recorded: self.stats_cache.total_recorded,
            sampling_ratio: self.sampling_ratio(),
            slow_query_count: self.stats_cache.slow_count,
            no_index_query_count: self.stats_cache.no_index_count,
            mode: self.mode,
        }
    }

    /// 프로파일 클리어
    pub fn clear(&mut self) {
        self.profiles.clear();
        self.slow_profiles.clear();
        self.stats_cache = CachedStats::default();
    }

    /// 느린 쿼리 임계값 조회
    pub fn slow_query_threshold(&self) -> u64 {
        self.slow_query_threshold_us
    }

    /// 느린 쿼리 임계값 설정
    pub fn set_slow_query_threshold(&mut self, threshold_us: u64) {
        self.slow_query_threshold_us = threshold_us;
    }
}

/// 확장 쿼리 통계 (샘플링 정보 포함)
#[derive(Debug, Clone)]
pub struct ExtendedQueryStats {
    /// 기본 통계
    pub base: QueryStats,
    /// 총 관측된 쿼리 수
    pub total_seen: u64,
    /// 총 기록된 쿼리 수
    pub total_recorded: u64,
    /// 샘플링 비율
    pub sampling_ratio: f64,
    /// 느린 쿼리 수
    pub slow_query_count: u64,
    /// 인덱스 미사용 쿼리 수
    pub no_index_query_count: u64,
    /// 프로파일링 모드
    pub mode: ProfilingMode,
}
