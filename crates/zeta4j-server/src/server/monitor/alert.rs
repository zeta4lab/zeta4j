//! M7.4: Alert System
//!
//! 임계값 기반 알림, 웹훅, 알림 채널 관리

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use super::health::{HealthChecker, HealthStatus};
use super::metrics::MetricsRegistry;

type EvaluatorFn = Arc<dyn Fn(&AlertContext) -> Option<AlertEvaluation> + Send + Sync>;

// ============================================================================
// AlertSeverity - 알림 심각도
// ============================================================================

/// 알림 심각도
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum AlertSeverity {
    Info,
    #[default]
    Warning,
    Critical,
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertSeverity::Info => write!(f, "INFO"),
            AlertSeverity::Warning => write!(f, "WARNING"),
            AlertSeverity::Critical => write!(f, "CRITICAL"),
        }
    }
}

// ============================================================================
// AlertCondition - 알림 조건
// ============================================================================

/// 알림 조건
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertCondition {
    /// 메트릭이 임계값 초과
    MetricAbove { metric: String, threshold: f64 },
    /// 메트릭이 임계값 미만
    MetricBelow { metric: String, threshold: f64 },
    /// 헬스 체크 실패
    HealthUnhealthy { component: String },
    /// 에러율 초과
    ErrorRateAbove { threshold: f64, window_secs: u64 },
    /// 슬로우 쿼리 비율 초과
    SlowQueryRateAbove { threshold: f64 },
    /// 커스텀 조건
    Custom { name: String, description: String },
}

impl AlertCondition {
    /// 조건 이름
    pub fn name(&self) -> String {
        match self {
            AlertCondition::MetricAbove { metric, .. } => format!("metric_above:{}", metric),
            AlertCondition::MetricBelow { metric, .. } => format!("metric_below:{}", metric),
            AlertCondition::HealthUnhealthy { component } => {
                format!("health_unhealthy:{}", component)
            }
            AlertCondition::ErrorRateAbove { .. } => "error_rate_above".to_string(),
            AlertCondition::SlowQueryRateAbove { .. } => "slow_query_rate_above".to_string(),
            AlertCondition::Custom { name, .. } => format!("custom:{}", name),
        }
    }
}

// ============================================================================
// Alert - 알림
// ============================================================================

/// 알림
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// 알림 ID
    pub id: String,
    /// 규칙 이름
    pub rule_name: String,
    /// 심각도
    pub severity: AlertSeverity,
    /// 메시지
    pub message: String,
    /// 발생 시간
    pub timestamp: DateTime<Utc>,
    /// 추가 레이블
    pub labels: HashMap<String, String>,
    /// 현재 값 (있는 경우)
    pub current_value: Option<f64>,
    /// 임계값 (있는 경우)
    pub threshold: Option<f64>,
}

impl Alert {
    /// JSON으로 변환
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

// ============================================================================
// AlertRule - 알림 규칙
// ============================================================================

/// 알림 규칙
pub struct AlertRule {
    /// 규칙 이름
    pub name: String,
    /// 조건
    pub condition: AlertCondition,
    /// 심각도
    pub severity: AlertSeverity,
    /// 메시지 템플릿
    pub message_template: String,
    /// 레이블
    pub labels: HashMap<String, String>,
    /// 활성화 여부
    pub enabled: bool,
    /// 평가 함수
    evaluator: EvaluatorFn,
}

/// 알림 평가 컨텍스트
#[derive(Default)]
pub struct AlertContext<'a> {
    pub metrics: Option<&'a MetricsRegistry>,
    pub health: Option<&'a HealthChecker>,
    pub custom_values: HashMap<String, f64>,
}

/// 알림 평가 결과
#[derive(Debug)]
pub struct AlertEvaluation {
    pub triggered: bool,
    pub current_value: Option<f64>,
    pub threshold: Option<f64>,
    pub message: String,
}

impl AlertRule {
    /// 새 알림 규칙 생성
    pub fn new(
        name: impl Into<String>,
        condition: AlertCondition,
        severity: AlertSeverity,
        message_template: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let message_template = message_template.into();
        let condition_clone = condition.clone();

        let evaluator: EvaluatorFn = match condition_clone {
            AlertCondition::MetricAbove {
                ref metric,
                threshold,
            } => {
                let metric = metric.clone();
                Arc::new(move |ctx: &AlertContext| {
                    let value = ctx.custom_values.get(&metric).copied()?;
                    Some(AlertEvaluation {
                        triggered: value > threshold,
                        current_value: Some(value),
                        threshold: Some(threshold),
                        message: format!("{} is {} (threshold: {})", metric, value, threshold),
                    })
                })
            }
            AlertCondition::MetricBelow {
                ref metric,
                threshold,
            } => {
                let metric = metric.clone();
                Arc::new(move |ctx: &AlertContext| {
                    let value = ctx.custom_values.get(&metric).copied()?;
                    Some(AlertEvaluation {
                        triggered: value < threshold,
                        current_value: Some(value),
                        threshold: Some(threshold),
                        message: format!("{} is {} (threshold: {})", metric, value, threshold),
                    })
                })
            }
            AlertCondition::HealthUnhealthy { ref component } => {
                let component = component.clone();
                Arc::new(move |ctx: &AlertContext| {
                    let health = ctx.health?;
                    let status = health.check_component(&component)?;
                    let triggered = matches!(status.status, HealthStatus::Unhealthy(_));
                    Some(AlertEvaluation {
                        triggered,
                        current_value: None,
                        threshold: None,
                        message: format!("Component {} is {:?}", component, status.status),
                    })
                })
            }
            AlertCondition::ErrorRateAbove { threshold, .. } => {
                Arc::new(move |ctx: &AlertContext| {
                    let error_rate = ctx.custom_values.get("error_rate").copied()?;
                    Some(AlertEvaluation {
                        triggered: error_rate > threshold,
                        current_value: Some(error_rate),
                        threshold: Some(threshold),
                        message: format!(
                            "Error rate is {:.2}% (threshold: {:.2}%)",
                            error_rate * 100.0,
                            threshold * 100.0
                        ),
                    })
                })
            }
            AlertCondition::SlowQueryRateAbove { threshold } => {
                Arc::new(move |ctx: &AlertContext| {
                    let rate = ctx.custom_values.get("slow_query_rate").copied()?;
                    Some(AlertEvaluation {
                        triggered: rate > threshold,
                        current_value: Some(rate),
                        threshold: Some(threshold),
                        message: format!(
                            "Slow query rate is {:.2}% (threshold: {:.2}%)",
                            rate * 100.0,
                            threshold * 100.0
                        ),
                    })
                })
            }
            AlertCondition::Custom { .. } => Arc::new(|_: &AlertContext| None),
        };

        Self {
            name,
            condition,
            severity,
            message_template,
            labels: HashMap::new(),
            enabled: true,
            evaluator,
        }
    }

    /// 레이블 추가
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// 규칙 평가
    pub fn evaluate(&self, ctx: &AlertContext) -> Option<Alert> {
        if !self.enabled {
            return None;
        }

        let eval = (self.evaluator)(ctx)?;

        if eval.triggered {
            Some(Alert {
                id: uuid::Uuid::now_v7().to_string(),
                rule_name: self.name.clone(),
                severity: self.severity,
                message: eval.message,
                timestamp: Utc::now(),
                labels: self.labels.clone(),
                current_value: eval.current_value,
                threshold: eval.threshold,
            })
        } else {
            None
        }
    }
}

// ============================================================================
// AlertChannel - 알림 채널 트레이트
// ============================================================================

/// 알림 채널 에러
#[derive(Debug, Clone)]
pub struct AlertError {
    pub message: String,
}

impl std::fmt::Display for AlertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AlertError: {}", self.message)
    }
}

impl std::error::Error for AlertError {}

/// 알림 채널 트레이트
pub trait AlertChannel: Send + Sync {
    /// 채널 이름
    fn name(&self) -> &str;

    /// 알림 전송
    fn send(&self, alert: &Alert) -> Result<(), AlertError>;
}

// ============================================================================
// WebhookChannel - 웹훅 채널
// ============================================================================

/// 웹훅 알림 채널
#[expect(dead_code)]
pub struct WebhookChannel {
    name: String,
    url: String,
    headers: HashMap<String, String>,
    /// 전송된 알림 기록 (테스트용)
    sent_alerts: RwLock<Vec<Alert>>,
}

impl WebhookChannel {
    /// 새 웹훅 채널 생성
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
            headers: HashMap::new(),
            sent_alerts: RwLock::new(Vec::new()),
        }
    }

    /// 헤더 추가
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// 전송된 알림 가져오기 (테스트용)
    pub fn sent_alerts(&self) -> Vec<Alert> {
        self.sent_alerts
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl AlertChannel for WebhookChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn send(&self, alert: &Alert) -> Result<(), AlertError> {
        // 실제로는 HTTP POST 요청을 보내야 하지만,
        // 여기서는 기록만 함 (의존성 최소화)
        let mut sent = self.sent_alerts.write().unwrap_or_else(|e| e.into_inner());
        sent.push(alert.clone());

        // 실제 구현에서는:
        // let client = reqwest::blocking::Client::new();
        // let mut request = client.post(&self.url);
        // for (k, v) in &self.headers {
        //     request = request.header(k, v);
        // }
        // request.json(alert).send()?;

        Ok(())
    }
}

/// 로그 알림 채널 (로깅으로 알림)
pub struct LogChannel {
    name: String,
    /// 기록된 알림 (테스트용)
    alerts: RwLock<Vec<Alert>>,
}

impl LogChannel {
    /// 새 로그 채널 생성
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alerts: RwLock::new(Vec::new()),
        }
    }

    /// 기록된 알림 가져오기
    pub fn alerts(&self) -> Vec<Alert> {
        self.alerts
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl AlertChannel for LogChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn send(&self, alert: &Alert) -> Result<(), AlertError> {
        // 로그로 기록 (실제로는 tracing/log 사용)
        eprintln!(
            "[ALERT] {} - {} - {}",
            alert.severity, alert.rule_name, alert.message
        );

        let mut alerts = self.alerts.write().unwrap_or_else(|e| e.into_inner());
        alerts.push(alert.clone());

        Ok(())
    }
}

// ============================================================================
// AlertThrottle - 알림 중복 방지
// ============================================================================

/// 알림 쓰로틀
pub struct AlertThrottle {
    /// 규칙별 마지막 알림 시간
    last_alerts: RwLock<HashMap<String, Instant>>,
    /// 쓰로틀 기간
    throttle_duration: Duration,
}

impl AlertThrottle {
    /// 새 쓰로틀 생성
    pub fn new(throttle_duration: Duration) -> Self {
        Self {
            last_alerts: RwLock::new(HashMap::new()),
            throttle_duration,
        }
    }

    /// 알림 허용 여부 확인 및 기록
    pub fn should_alert(&self, rule_name: &str) -> bool {
        let mut last = self.last_alerts.write().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();

        if let Some(last_time) = last.get(rule_name)
            && now.duration_since(*last_time) < self.throttle_duration
        {
            return false;
        }

        last.insert(rule_name.to_string(), now);
        true
    }

    /// 특정 규칙의 쓰로틀 리셋
    pub fn reset(&self, rule_name: &str) {
        let mut last = self.last_alerts.write().unwrap_or_else(|e| e.into_inner());
        last.remove(rule_name);
    }

    /// 모든 쓰로틀 리셋
    pub fn reset_all(&self) {
        let mut last = self.last_alerts.write().unwrap_or_else(|e| e.into_inner());
        last.clear();
    }
}

// ============================================================================
// AlertManager - 알림 관리자
// ============================================================================

/// 알림 관리자
pub struct AlertManager {
    /// 등록된 규칙들
    rules: RwLock<Vec<AlertRule>>,
    /// 알림 채널들
    channels: RwLock<Vec<Arc<dyn AlertChannel>>>,
    /// 쓰로틀
    throttle: AlertThrottle,
    /// 알림 히스토리
    history: RwLock<Vec<Alert>>,
    /// 히스토리 최대 크기
    max_history: usize,
}

impl AlertManager {
    /// 새 알림 관리자 생성
    pub fn new(throttle_duration: Duration) -> Self {
        Self {
            rules: RwLock::new(Vec::new()),
            channels: RwLock::new(Vec::new()),
            throttle: AlertThrottle::new(throttle_duration),
            history: RwLock::new(Vec::new()),
            max_history: 1000,
        }
    }

    /// 규칙 추가
    pub fn add_rule(&self, rule: AlertRule) {
        let mut rules = self.rules.write().unwrap_or_else(|e| e.into_inner());
        rules.push(rule);
    }

    /// 규칙 제거
    pub fn remove_rule(&self, name: &str) {
        let mut rules = self.rules.write().unwrap_or_else(|e| e.into_inner());
        rules.retain(|r| r.name != name);
    }

    /// 채널 추가
    pub fn add_channel(&self, channel: Arc<dyn AlertChannel>) {
        let mut channels = self.channels.write().unwrap_or_else(|e| e.into_inner());
        channels.push(channel);
    }

    /// 모든 규칙 평가
    pub fn evaluate(&self, ctx: &AlertContext) -> Vec<Alert> {
        let rules = self.rules.read().unwrap_or_else(|e| e.into_inner());
        let mut triggered_alerts = Vec::new();

        for rule in rules.iter() {
            if let Some(alert) = rule.evaluate(ctx)
                && self.throttle.should_alert(&rule.name)
            {
                triggered_alerts.push(alert);
            }
        }

        // 알림 전송 및 히스토리 기록
        if !triggered_alerts.is_empty() {
            let channels = self.channels.read().unwrap_or_else(|e| e.into_inner());
            let mut history = self.history.write().unwrap_or_else(|e| e.into_inner());

            for alert in &triggered_alerts {
                // 모든 채널에 전송
                for channel in channels.iter() {
                    if let Err(e) = channel.send(alert) {
                        eprintln!("Failed to send alert to {}: {}", channel.name(), e);
                    }
                }

                // 히스토리에 추가
                history.push(alert.clone());
                if history.len() > self.max_history {
                    history.remove(0);
                }
            }
        }

        triggered_alerts
    }

    /// 알림 히스토리 가져오기
    pub fn history(&self, count: usize) -> Vec<Alert> {
        let history = self.history.read().unwrap_or_else(|e| e.into_inner());
        history.iter().rev().take(count).cloned().collect()
    }

    /// 심각도별 히스토리 필터링
    pub fn history_by_severity(&self, severity: AlertSeverity) -> Vec<Alert> {
        let history = self.history.read().unwrap_or_else(|e| e.into_inner());
        history
            .iter()
            .filter(|a| a.severity == severity)
            .cloned()
            .collect()
    }

    /// 등록된 규칙 목록
    pub fn list_rules(&self) -> Vec<String> {
        let rules = self.rules.read().unwrap_or_else(|e| e.into_inner());
        rules.iter().map(|r| r.name.clone()).collect()
    }

    /// 등록된 채널 목록
    pub fn list_channels(&self) -> Vec<String> {
        let channels = self.channels.read().unwrap_or_else(|e| e.into_inner());
        channels.iter().map(|c| c.name().to_string()).collect()
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new(Duration::from_secs(300)) // 5분 기본 쓰로틀
    }
}

// ============================================================================
// Built-in Alert Rules
// ============================================================================

/// 기본 알림 규칙들 생성
pub fn default_alert_rules() -> Vec<AlertRule> {
    vec![
        AlertRule::new(
            "high_cpu",
            AlertCondition::MetricAbove {
                metric: "cpu_usage".to_string(),
                threshold: 0.9,
            },
            AlertSeverity::Warning,
            "CPU usage is above 90%",
        ),
        AlertRule::new(
            "critical_cpu",
            AlertCondition::MetricAbove {
                metric: "cpu_usage".to_string(),
                threshold: 0.95,
            },
            AlertSeverity::Critical,
            "CPU usage is critically high (>95%)",
        ),
        AlertRule::new(
            "high_memory",
            AlertCondition::MetricAbove {
                metric: "memory_usage".to_string(),
                threshold: 0.85,
            },
            AlertSeverity::Warning,
            "Memory usage is above 85%",
        ),
        AlertRule::new(
            "low_disk_space",
            AlertCondition::MetricAbove {
                metric: "disk_usage".to_string(),
                threshold: 0.9,
            },
            AlertSeverity::Critical,
            "Disk space is critically low (<10% free)",
        ),
        AlertRule::new(
            "high_error_rate",
            AlertCondition::ErrorRateAbove {
                threshold: 0.05,
                window_secs: 300,
            },
            AlertSeverity::Warning,
            "Error rate exceeds 5%",
        ),
        AlertRule::new(
            "slow_queries",
            AlertCondition::SlowQueryRateAbove { threshold: 0.1 },
            AlertSeverity::Warning,
            "Slow query rate exceeds 10%",
        ),
    ]
}
