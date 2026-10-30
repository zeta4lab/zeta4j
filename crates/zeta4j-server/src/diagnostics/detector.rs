//! 문제 탐지기 모듈

use super::monitor::SystemStatus;
use super::profiler::QueryProfile;
use serde::{Deserialize, Serialize};

/// 문제 심각도
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProblemSeverity {
    /// 참고 사항
    Info,
    /// 주의 필요
    Warning,
    /// 즉시 조치 필요
    Critical,
}

impl ProblemSeverity {
    /// 심각도 이름
    pub fn name(&self) -> &str {
        match self {
            ProblemSeverity::Info => "INFO",
            ProblemSeverity::Warning => "WARNING",
            ProblemSeverity::Critical => "CRITICAL",
        }
    }
}

/// 문제 유형
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProblemType {
    // === 시스템 문제 ===
    /// 높은 메모리 사용량
    HighMemoryUsage,
    /// 낮은 캐시 히트율
    LowCacheHitRatio,
    /// 느린 쿼리
    SlowQueries,
    /// 인덱스 미사용
    MissingIndex,
    /// 락 경합
    LockContention,
    /// 데드락 위험
    DeadlockRisk,
    /// 대규모 트랜잭션
    LargeTransaction,
    /// 높은 트랜잭션 중단율
    HighAbortRate,
    /// 빈번한 제거
    FrequentEviction,

    // === 보안 문제 (v1.62.0) ===
    /// 무단 접근 시도
    UnauthorizedAccess,
    /// 권한 상승 시도
    PermissionEscalation,
    /// 비밀번호 정책 위반
    PasswordViolation,
    /// 비정상 로그인 패턴
    AbnormalLoginPattern,
    /// 대량 접근 거부
    MassiveAccessDenials,
    /// 의심스러운 사용자 활동
    SuspiciousUserActivity,
    /// 인증 실패
    AuthenticationFailure,
    /// 세션 이상
    SessionAnomaly,
}

impl ProblemType {
    /// 문제 유형 이름
    pub fn name(&self) -> &str {
        match self {
            // 시스템 문제
            ProblemType::HighMemoryUsage => "High memory usage",
            ProblemType::LowCacheHitRatio => "Low cache hit ratio",
            ProblemType::SlowQueries => "Slow queries",
            ProblemType::MissingIndex => "Missing index",
            ProblemType::LockContention => "Lock contention",
            ProblemType::DeadlockRisk => "Deadlock risk",
            ProblemType::LargeTransaction => "Large transaction",
            ProblemType::HighAbortRate => "High transaction abort rate",
            ProblemType::FrequentEviction => "Frequent cache eviction",
            // 보안 문제
            ProblemType::UnauthorizedAccess => "Unauthorized access attempt",
            ProblemType::PermissionEscalation => "Permission escalation attempt",
            ProblemType::PasswordViolation => "Password policy violation",
            ProblemType::AbnormalLoginPattern => "Abnormal login pattern",
            ProblemType::MassiveAccessDenials => "Massive access denials",
            ProblemType::SuspiciousUserActivity => "Suspicious user activity",
            ProblemType::AuthenticationFailure => "Authentication failure",
            ProblemType::SessionAnomaly => "Session anomaly",
        }
    }

    /// 보안 관련 문제인지 확인
    pub fn is_security_problem(&self) -> bool {
        matches!(
            self,
            ProblemType::UnauthorizedAccess
                | ProblemType::PermissionEscalation
                | ProblemType::PasswordViolation
                | ProblemType::AbnormalLoginPattern
                | ProblemType::MassiveAccessDenials
                | ProblemType::SuspiciousUserActivity
                | ProblemType::AuthenticationFailure
                | ProblemType::SessionAnomaly
        )
    }
}

/// 탐지된 문제
#[derive(Debug, Clone)]
pub struct DetectedProblem {
    /// 문제 ID
    pub id: String,
    /// 심각도
    pub severity: ProblemSeverity,
    /// 문제 유형
    pub problem_type: ProblemType,
    /// 설명
    pub description: String,
    /// 영향
    pub impact: String,
    /// 권장 조치
    pub recommendation: String,
    /// 탐지 시간
    pub detected_at: u64,
}

impl DetectedProblem {
    /// 새 DetectedProblem 생성
    pub fn new(
        problem_type: ProblemType,
        severity: ProblemSeverity,
        description: String,
        impact: String,
        recommendation: String,
    ) -> Self {
        let id = format!(
            "{:?}_{:x}",
            problem_type,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64
        );

        Self {
            id,
            severity,
            problem_type,
            description,
            impact,
            recommendation,
            detected_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// 문제 탐지기 설정
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// 메모리 사용량 임계값 (0.0 ~ 1.0)
    pub memory_threshold: f64,
    /// 캐시 히트율 임계값 (0.0 ~ 1.0)
    pub cache_hit_threshold: f64,
    /// 느린 쿼리 임계값 (마이크로초)
    pub slow_query_threshold_us: u64,
    /// 락 대기 임계값
    pub lock_wait_threshold: usize,
    /// 트랜잭션 중단율 임계값
    pub abort_rate_threshold: f64,
    /// 제거 횟수 임계값
    pub eviction_threshold: usize,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            memory_threshold: 0.9,              // 90%
            cache_hit_threshold: 0.7,           // 70%
            slow_query_threshold_us: 1_000_000, // 1초
            lock_wait_threshold: 10,
            abort_rate_threshold: 0.1, // 10%
            eviction_threshold: 1000,
        }
    }
}

/// 문제 탐지기
pub struct ProblemDetector {
    config: DetectorConfig,
}

impl ProblemDetector {
    /// 새 ProblemDetector 생성
    pub fn new(config: DetectorConfig) -> Self {
        Self { config }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(DetectorConfig::default())
    }

    /// 설정 반환
    pub fn config(&self) -> &DetectorConfig {
        &self.config
    }

    /// 시스템 상태에서 문제 탐지
    pub fn detect(&self, status: &SystemStatus) -> Vec<DetectedProblem> {
        let mut problems = Vec::new();

        // 높은 메모리 사용량
        let memory_ratio = status.memory_usage_ratio();
        if memory_ratio >= self.config.memory_threshold {
            let severity = if memory_ratio >= 0.95 {
                ProblemSeverity::Critical
            } else {
                ProblemSeverity::Warning
            };

            problems.push(DetectedProblem::new(
                ProblemType::HighMemoryUsage,
                severity,
                format!("Memory usage is at {:.1}%", memory_ratio * 100.0),
                "Possible performance degradation or OOM due to insufficient memory".to_string(),
                "Consider cleaning unnecessary data, increasing memory limits, or optimizing queries".to_string(),
            ));
        }

        // 낮은 캐시 히트율
        if status.buffer_pool.hit_ratio < self.config.cache_hit_threshold {
            let severity = if status.buffer_pool.hit_ratio < 0.5 {
                ProblemSeverity::Critical
            } else {
                ProblemSeverity::Warning
            };

            problems.push(DetectedProblem::new(
                ProblemType::LowCacheHitRatio,
                severity,
                format!(
                    "Cache hit ratio is at {:.1}%",
                    status.buffer_pool.hit_ratio * 100.0
                ),
                "Query performance degradation due to increased disk I/O".to_string(),
                "Consider increasing buffer pool size or analyzing frequently accessed data patterns".to_string(),
            ));
        }

        // 락 경합
        if status.locks.waiting_count >= self.config.lock_wait_threshold {
            problems.push(DetectedProblem::new(
                ProblemType::LockContention,
                ProblemSeverity::Warning,
                format!(
                    "{} transactions are waiting for locks",
                    status.locks.waiting_count
                ),
                "Possible transaction processing delays and timeouts".to_string(),
                "Consider reducing transaction scope or optimizing access patterns".to_string(),
            ));
        }

        // 데드락 위험
        if status.locks.deadlock_count > 0 {
            problems.push(DetectedProblem::new(
                ProblemType::DeadlockRisk,
                ProblemSeverity::Warning,
                format!("{} deadlocks have occurred", status.locks.deadlock_count),
                "Performance degradation due to transaction aborts and retries".to_string(),
                "Consider unifying lock acquisition order or adjusting transaction isolation level"
                    .to_string(),
            ));
        }

        // 높은 트랜잭션 중단율
        if status.transactions.abort_ratio() >= self.config.abort_rate_threshold {
            problems.push(DetectedProblem::new(
                ProblemType::HighAbortRate,
                ProblemSeverity::Warning,
                format!(
                    "Transaction abort rate is at {:.1}%",
                    status.transactions.abort_ratio() * 100.0
                ),
                "Resource waste and throughput degradation".to_string(),
                "Consider analyzing conflict causes and reviewing transaction design".to_string(),
            ));
        }

        // 빈번한 캐시 제거
        if status.buffer_pool.eviction_count >= self.config.eviction_threshold {
            problems.push(DetectedProblem::new(
                ProblemType::FrequentEviction,
                ProblemSeverity::Info,
                format!(
                    "{} cache evictions have occurred",
                    status.buffer_pool.eviction_count
                ),
                "Reduced cache efficiency".to_string(),
                "Consider increasing buffer pool size".to_string(),
            ));
        }

        problems
    }

    /// 쿼리 프로파일에서 문제 탐지
    pub fn detect_query_problems(&self, profiles: &[QueryProfile]) -> Vec<DetectedProblem> {
        self.detect_query_problems_iter(profiles.iter())
    }

    /// 쿼리 프로파일 참조에서 문제 탐지 (Vec<&QueryProfile> 지원)
    pub fn detect_query_problems_refs(&self, profiles: Vec<&QueryProfile>) -> Vec<DetectedProblem> {
        self.detect_query_problems_iter(profiles.into_iter())
    }

    /// 내부 구현: Iterator를 사용한 쿼리 문제 탐지
    fn detect_query_problems_iter<'a>(
        &self,
        profiles: impl Iterator<Item = &'a QueryProfile>,
    ) -> Vec<DetectedProblem> {
        let mut problems = Vec::new();
        let profiles: Vec<_> = profiles.collect();

        // 느린 쿼리 탐지
        let slow_queries: Vec<_> = profiles
            .iter()
            .filter(|p| p.total_time_us >= self.config.slow_query_threshold_us)
            .collect();

        if !slow_queries.is_empty() {
            let max_time = slow_queries
                .iter()
                .map(|p| p.total_time_us)
                .max()
                .unwrap_or(0);
            let severity = if max_time >= 10_000_000 {
                ProblemSeverity::Critical
            } else {
                ProblemSeverity::Warning
            };

            problems.push(DetectedProblem::new(
                ProblemType::SlowQueries,
                severity,
                format!(
                    "{} slow queries detected (max {:.2}s)",
                    slow_queries.len(),
                    max_time as f64 / 1_000_000.0
                ),
                "Degraded user experience and increased system load".to_string(),
                "Consider analyzing query execution plans and adding indexes".to_string(),
            ));
        }

        // 인덱스 미사용 쿼리
        let no_index_queries: Vec<_> = profiles
            .iter()
            .filter(|p| !p.index_used && p.nodes_scanned > 1000)
            .collect();

        if !no_index_queries.is_empty() {
            problems.push(DetectedProblem::new(
                ProblemType::MissingIndex,
                ProblemSeverity::Warning,
                format!(
                    "{} queries are scanning many nodes without using an index",
                    no_index_queries.len()
                ),
                "Performance degradation due to unnecessary full scans".to_string(),
                "Consider creating indexes on the relevant properties".to_string(),
            ));
        }

        problems
    }

    /// 모든 문제 탐지
    pub fn detect_all(
        &self,
        status: &SystemStatus,
        profiles: &[QueryProfile],
    ) -> Vec<DetectedProblem> {
        let mut problems = self.detect(status);
        problems.extend(self.detect_query_problems(profiles));

        // 심각도 순으로 정렬
        problems.sort_by(|a, b| b.severity.cmp(&a.severity));
        problems
    }

    /// 모든 문제 탐지 (Vec<&QueryProfile> 지원)
    pub fn detect_all_refs(
        &self,
        status: &SystemStatus,
        profiles: Vec<&QueryProfile>,
    ) -> Vec<DetectedProblem> {
        let mut problems = self.detect(status);
        problems.extend(self.detect_query_problems_refs(profiles));

        // 심각도 순으로 정렬
        problems.sort_by(|a, b| b.severity.cmp(&a.severity));
        problems
    }
}
