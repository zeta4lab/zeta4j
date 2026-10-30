//! 자동 튜닝 통합 모듈

use super::auto_stats::{AutoStatsConfig, AutoStatsRefresher, ChangeType, RefreshResult};
use super::index_advisor::{IndexAdvisor, IndexAdvisorConfig, IndexRecommendation, QueryPattern};
use super::memory_tuner::{MemoryAutoTuner, MemoryStats, MemoryTuningConfig, TuningAction};

/// 자동 튜닝 설정
#[derive(Debug, Clone)]
pub struct AutoTunerConfig {
    /// 통계 자동 갱신 활성화
    pub enable_auto_stats: bool,
    /// 인덱스 추천 활성화
    pub enable_index_advisor: bool,
    /// 메모리 자동 조정 활성화
    pub enable_memory_tuning: bool,
    /// 튜닝 주기 (초)
    pub tuning_interval_secs: u64,
    /// 통계 갱신 설정
    pub stats_config: AutoStatsConfig,
    /// 인덱스 추천 설정
    pub index_config: IndexAdvisorConfig,
    /// 메모리 튜닝 설정
    pub memory_config: MemoryTuningConfig,
}

impl Default for AutoTunerConfig {
    fn default() -> Self {
        Self {
            enable_auto_stats: true,
            enable_index_advisor: true,
            enable_memory_tuning: true,
            tuning_interval_secs: 300, // 5분
            stats_config: AutoStatsConfig::default(),
            index_config: IndexAdvisorConfig::default(),
            memory_config: MemoryTuningConfig::default(),
        }
    }
}

/// 시스템 상태
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// 최적 상태
    Optimal,
    /// 양호
    Good,
    /// 주의 필요
    NeedsAttention,
    /// 심각
    Critical,
}

impl HealthStatus {
    /// 상태 이름 반환
    pub fn name(&self) -> &str {
        match self {
            HealthStatus::Optimal => "Optimal",
            HealthStatus::Good => "Good",
            HealthStatus::NeedsAttention => "Needs Attention",
            HealthStatus::Critical => "Critical",
        }
    }

    /// 상태 점수 반환 (0-100)
    pub fn score(&self) -> u8 {
        match self {
            HealthStatus::Optimal => 100,
            HealthStatus::Good => 80,
            HealthStatus::NeedsAttention => 50,
            HealthStatus::Critical => 20,
        }
    }
}

/// 튜닝 보고서
#[derive(Debug, Clone)]
pub struct TuningReport {
    /// 타임스탬프
    pub timestamp: u64,
    /// 통계 갱신 여부
    pub stats_refreshed: bool,
    /// 통계 갱신 결과
    pub stats_result: Option<RefreshResult>,
    /// 인덱스 추천 목록
    pub index_recommendations: Vec<IndexRecommendation>,
    /// 메모리 튜닝 액션
    pub memory_actions: Vec<TuningAction>,
    /// 전체 시스템 상태
    pub overall_health: HealthStatus,
    /// 상태 설명
    pub health_details: String,
}

impl TuningReport {
    /// 빈 보고서 생성
    pub fn empty() -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            stats_refreshed: false,
            stats_result: None,
            index_recommendations: Vec::new(),
            memory_actions: Vec::new(),
            overall_health: HealthStatus::Good,
            health_details: String::new(),
        }
    }

    /// 요약 문자열 반환
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();

        if self.stats_refreshed
            && let Some(ref result) = self.stats_result
        {
            parts.push(format!(
                "Stats refreshed: {} labels, {} relation types",
                result.labels_refreshed, result.rel_types_refreshed
            ));
        }

        if !self.index_recommendations.is_empty() {
            parts.push(format!(
                "Index recommendations: {}",
                self.index_recommendations.len()
            ));
        }

        if !self.memory_actions.is_empty() {
            parts.push(format!("Memory adjustments: {}", self.memory_actions.len()));
        }

        if parts.is_empty() {
            "No changes".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// 자동 튜닝 통합 관리자
pub struct AutoTuner {
    config: AutoTunerConfig,
    stats_refresher: AutoStatsRefresher,
    index_advisor: IndexAdvisor,
    memory_tuner: MemoryAutoTuner,
    last_tune_time: u64,
}

impl AutoTuner {
    /// 새 AutoTuner 생성
    pub fn new(config: AutoTunerConfig) -> Self {
        Self {
            stats_refresher: AutoStatsRefresher::new(config.stats_config.clone()),
            index_advisor: IndexAdvisor::new(config.index_config.clone()),
            memory_tuner: MemoryAutoTuner::new(config.memory_config.clone()),
            config,
            last_tune_time: 0,
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(AutoTunerConfig::default())
    }

    /// 설정 반환
    pub fn config(&self) -> &AutoTunerConfig {
        &self.config
    }

    /// 변경 사항 기록
    pub fn record_change(&self, change: ChangeType) {
        if self.config.enable_auto_stats {
            self.stats_refresher.record_change(change);
        }
    }

    /// 쿼리 패턴 기록
    pub fn record_query(&mut self, pattern: QueryPattern) {
        if self.config.enable_index_advisor {
            self.index_advisor.record_query(pattern);
        }
    }

    /// 메모리 통계 기록
    pub fn record_memory_stats(&mut self, stats: &MemoryStats) {
        if self.config.enable_memory_tuning {
            self.memory_tuner.record_hit_ratio(stats.hit_ratio);
        }
    }

    /// 인덱스 사용 기록
    pub fn record_index_usage(&mut self, index_name: &str) {
        if self.config.enable_index_advisor {
            self.index_advisor.record_index_usage(index_name);
        }
    }

    /// 기존 인덱스 등록
    pub fn register_existing_index(&mut self, index_name: String) {
        if self.config.enable_index_advisor {
            self.index_advisor.register_existing_index(index_name);
        }
    }

    /// 통계 갱신 필요 여부 확인
    pub fn needs_stats_refresh(&self) -> bool {
        self.config.enable_auto_stats && self.stats_refresher.needs_refresh()
    }

    /// 튜닝 실행
    pub fn tune(&mut self, memory_stats: Option<&MemoryStats>) -> TuningReport {
        let mut report = TuningReport::empty();

        // 1. 통계 갱신
        if self.config.enable_auto_stats && self.stats_refresher.needs_refresh() {
            let result = self.stats_refresher.refresh();
            report.stats_refreshed = true;
            report.stats_result = Some(result);
        }

        // 2. 인덱스 추천
        if self.config.enable_index_advisor {
            report.index_recommendations = self.index_advisor.recommend();
        }

        // 3. 메모리 튜닝
        if self.config.enable_memory_tuning
            && let Some(stats) = memory_stats
            && let Ok(actions) = self.memory_tuner.auto_tune(stats)
        {
            report.memory_actions = actions;
        }

        // 4. 전체 상태 평가
        let (health, details) = self.evaluate_health(&report, memory_stats);
        report.overall_health = health;
        report.health_details = details;

        self.last_tune_time = report.timestamp;
        report
    }

    /// 시스템 상태 평가
    fn evaluate_health(
        &self,
        report: &TuningReport,
        memory_stats: Option<&MemoryStats>,
    ) -> (HealthStatus, String) {
        let mut issues = Vec::new();
        let mut score = 100i32;

        // 인덱스 추천이 많으면 점수 감소
        if report.index_recommendations.len() > 5 {
            score -= 20;
            issues.push(format!(
                "{} index recommendations pending",
                report.index_recommendations.len()
            ));
        } else if !report.index_recommendations.is_empty() {
            score -= 10;
        }

        // 메모리 상태 확인
        if let Some(stats) = memory_stats {
            if stats.hit_ratio < 0.7 {
                score -= 30;
                issues.push(format!(
                    "Low cache hit rate: {:.1}%",
                    stats.hit_ratio * 100.0
                ));
            } else if stats.hit_ratio < 0.9 {
                score -= 15;
            }

            if stats.memory_pressure > 0.9 {
                score -= 25;
                issues.push(format!(
                    "High memory pressure: {:.1}%",
                    stats.memory_pressure * 100.0
                ));
            } else if stats.memory_pressure > 0.7 {
                score -= 10;
            }

            if stats.eviction_count > 10000 {
                score -= 15;
                issues.push(format!(
                    "Frequent cache evictions: {} times",
                    stats.eviction_count
                ));
            }
        }

        let health = if score >= 90 {
            HealthStatus::Optimal
        } else if score >= 70 {
            HealthStatus::Good
        } else if score >= 40 {
            HealthStatus::NeedsAttention
        } else {
            HealthStatus::Critical
        };

        let details = if issues.is_empty() {
            "System is operating optimally".to_string()
        } else {
            issues.join("; ")
        };

        (health, details)
    }

    /// 인덱스 추천 조회
    pub fn get_index_recommendations(&self) -> Vec<IndexRecommendation> {
        if self.config.enable_index_advisor {
            self.index_advisor.recommend()
        } else {
            Vec::new()
        }
    }

    /// 미사용 인덱스 조회
    pub fn get_unused_indexes(&self) -> Vec<String> {
        if self.config.enable_index_advisor {
            self.index_advisor.find_unused_indexes()
        } else {
            Vec::new()
        }
    }

    /// 현재 버퍼 풀 크기 반환
    pub fn current_buffer_pool_size(&self) -> usize {
        self.memory_tuner.current_buffer_pool_size()
    }

    /// 변경된 라벨 목록 반환
    pub fn dirty_labels(&self) -> Vec<String> {
        self.stats_refresher.dirty_labels()
    }

    /// 변경된 관계 타입 목록 반환
    pub fn dirty_rel_types(&self) -> Vec<String> {
        self.stats_refresher.dirty_rel_types()
    }

    /// 시스템 상태 조회
    pub fn get_health_status(&self, memory_stats: Option<&MemoryStats>) -> HealthStatus {
        let report = TuningReport::empty();
        let (health, _) = self.evaluate_health(&report, memory_stats);
        health
    }
}
