//! 진단 관리자 모듈

use super::detector::{DetectedProblem, DetectorConfig, ProblemDetector, ProblemSeverity};
use super::monitor::{SystemMonitor, SystemStatus};
use super::profiler::{QueryProfile, QueryProfiler, QueryStats};

/// 진단 보고서
#[derive(Debug, Clone)]
pub struct DiagnosticReport {
    /// 보고서 ID
    pub id: String,
    /// 생성 시간
    pub generated_at: u64,
    /// 시스템 상태
    pub system_status: SystemStatus,
    /// 탐지된 문제들
    pub problems: Vec<DetectedProblem>,
    /// 쿼리 통계
    pub query_stats: QueryStats,
    /// 권장 사항
    pub recommendations: Vec<String>,
    /// 전체 상태 점수 (0-100)
    pub health_score: u8,
}

impl DiagnosticReport {
    /// 보고서 요약
    pub fn summary(&self) -> String {
        let critical = self
            .problems
            .iter()
            .filter(|p| p.severity == ProblemSeverity::Critical)
            .count();
        let warning = self
            .problems
            .iter()
            .filter(|p| p.severity == ProblemSeverity::Warning)
            .count();

        format!(
            "Health score: {}/100, Critical: {}, Warning: {}",
            self.health_score, critical, warning
        )
    }

    /// 심각한 문제가 있는지 확인
    pub fn has_critical_problems(&self) -> bool {
        self.problems
            .iter()
            .any(|p| p.severity == ProblemSeverity::Critical)
    }
}

/// 진단 관리자
pub struct DiagnosticManager {
    profiler: QueryProfiler,
    monitor: SystemMonitor,
    detector: ProblemDetector,
}

impl DiagnosticManager {
    /// 새 DiagnosticManager 생성
    pub fn new(
        slow_query_threshold_us: u64,
        max_history: usize,
        detector_config: DetectorConfig,
    ) -> Self {
        Self {
            profiler: QueryProfiler::new(slow_query_threshold_us),
            monitor: SystemMonitor::new(max_history),
            detector: ProblemDetector::new(detector_config),
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(
            1_000_000, // 1초
            100,
            DetectorConfig::default(),
        )
    }

    /// 쿼리 프로파일 기록
    pub fn record_query(&mut self, profile: QueryProfile) {
        self.profiler.record(profile);
    }

    /// 시스템 상태 스냅샷 기록
    pub fn record_status(&mut self, status: SystemStatus) {
        self.monitor.record(status);
    }

    /// 현재 시스템 상태 조회
    pub fn current_status(&self) -> Option<&SystemStatus> {
        self.monitor.current()
    }

    /// 문제 탐지
    pub fn detect_problems(&self) -> Vec<DetectedProblem> {
        let status = self.monitor.current().cloned().unwrap_or_default();

        self.detector
            .detect_all_refs(&status, self.profiler.profiles())
    }

    /// 느린 쿼리 조회
    pub fn slow_queries(&self) -> Vec<&QueryProfile> {
        self.profiler.slow_queries()
    }

    /// 가장 느린 쿼리 N개
    pub fn top_slow_queries(&self, n: usize) -> Vec<&QueryProfile> {
        self.profiler.top_slow_queries(n)
    }

    /// 인덱스 미사용 쿼리
    pub fn queries_without_index(&self) -> Vec<&QueryProfile> {
        self.profiler.queries_without_index()
    }

    /// 쿼리 통계
    pub fn query_stats(&self) -> QueryStats {
        self.profiler.stats()
    }

    /// 상태 점수 계산 (0-100)
    pub fn health_score(&self) -> u8 {
        let mut score = 100i32;

        // 시스템 상태 기반 감점
        if let Some(status) = self.monitor.current() {
            // 메모리 사용량
            let memory_ratio = status.memory_usage_ratio();
            if memory_ratio > 0.95 {
                score -= 30;
            } else if memory_ratio > 0.9 {
                score -= 20;
            } else if memory_ratio > 0.8 {
                score -= 10;
            }

            // 캐시 히트율
            if status.buffer_pool.hit_ratio < 0.5 {
                score -= 30;
            } else if status.buffer_pool.hit_ratio < 0.7 {
                score -= 20;
            } else if status.buffer_pool.hit_ratio < 0.9 {
                score -= 10;
            }

            // 락 경합
            if status.locks.waiting_count > 20 {
                score -= 15;
            } else if status.locks.waiting_count > 10 {
                score -= 10;
            }

            // 데드락
            if status.locks.deadlock_count > 10 {
                score -= 15;
            } else if status.locks.deadlock_count > 0 {
                score -= 5;
            }
        }

        // 쿼리 성능 기반 감점
        let stats = self.profiler.stats();

        // 느린 쿼리 비율
        if stats.total_queries > 0 {
            let slow_ratio = stats.slow_queries as f64 / stats.total_queries as f64;
            if slow_ratio > 0.1 {
                score -= 15;
            } else if slow_ratio > 0.05 {
                score -= 10;
            }
        }

        // 인덱스 미사용
        if stats.index_usage_ratio < 0.5 {
            score -= 15;
        } else if stats.index_usage_ratio < 0.8 {
            score -= 10;
        }

        score.clamp(0, 100) as u8
    }

    /// 권장 사항 생성
    pub fn get_recommendations(&self) -> Vec<String> {
        let mut recommendations = Vec::new();

        if let Some(status) = self.monitor.current() {
            // 메모리 관련
            if status.memory_usage_ratio() > 0.9 {
                recommendations.push(
                    "Memory usage is high. Consider cleaning unnecessary data or increasing memory limits."
                        .to_string(),
                );
            }

            // 캐시 관련
            if status.buffer_pool.hit_ratio < 0.7 {
                recommendations.push(
                    "Cache hit ratio is low. Consider increasing the buffer pool size.".to_string(),
                );
            }

            // 락 관련
            if status.locks.waiting_count > 10 {
                recommendations
                    .push("Lock contention is occurring. Reduce transaction scope.".to_string());
            }
        }

        // 쿼리 관련
        let stats = self.profiler.stats();

        if stats.slow_queries > 0 {
            recommendations.push(format!(
                "{} slow queries found. Analyze execution plans with EXPLAIN.",
                stats.slow_queries
            ));
        }

        if stats.index_usage_ratio < 0.8 && stats.total_queries > 10 {
            recommendations
                .push("Some queries are not using indexes. Consider adding indexes.".to_string());
        }

        recommendations
    }

    /// 전체 진단 보고서 생성
    pub fn generate_report(&self) -> DiagnosticReport {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let system_status = self.monitor.current().cloned().unwrap_or_default();

        DiagnosticReport {
            id: format!("report_{:x}", now),
            generated_at: now,
            system_status,
            problems: self.detect_problems(),
            query_stats: self.query_stats(),
            recommendations: self.get_recommendations(),
            health_score: self.health_score(),
        }
    }

    /// 프로파일러 클리어
    pub fn clear_profiles(&mut self) {
        self.profiler.clear();
    }

    /// 모니터 클리어
    pub fn clear_history(&mut self) {
        self.monitor.clear();
    }

    /// 전체 클리어
    pub fn clear_all(&mut self) {
        self.clear_profiles();
        self.clear_history();
    }
}
