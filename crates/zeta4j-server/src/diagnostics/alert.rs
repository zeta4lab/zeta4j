//! 알림 시스템 모듈
//!
//! 진단 이벤트에 대한 알림을 다양한 채널로 전송합니다.
//!
//! # v1.60.0
//!
//! - AlertChannel: Webhook, Email, Slack 알림 채널
//! - AlertRule: 알림 규칙 정의
//! - AlertManager: 알림 관리 및 전송
//!
//! # 예시
//!
//! ```rust,ignore
//! use zeta4j_server::diagnostics::{AlertManager, AlertChannel, AlertRule, ProblemSeverity};
//!
//! let manager = AlertManager::new()
//!     .with_channel(AlertChannel::webhook("https://hooks.example.com/alert"))
//!     .with_channel(AlertChannel::slack("https://hooks.slack.com/...", "#alerts"))
//!     .with_rule(AlertRule::severity_threshold(ProblemSeverity::Warning));
//!
//! // 문제 탐지 시 자동 알림
//! manager.notify(&detected_problem).await?;
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use super::detector::{DetectedProblem, ProblemSeverity, ProblemType};

/// 알림 채널 타입
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChannelType {
    /// HTTP/HTTPS 웹훅
    Webhook,
    /// 이메일
    Email,
    /// Slack
    Slack,
    /// Discord
    Discord,
    /// Microsoft Teams
    Teams,
    /// 커스텀 채널
    Custom(String),
}

impl ChannelType {
    /// 채널 타입 이름
    pub fn name(&self) -> &str {
        match self {
            ChannelType::Webhook => "Webhook",
            ChannelType::Email => "Email",
            ChannelType::Slack => "Slack",
            ChannelType::Discord => "Discord",
            ChannelType::Teams => "Teams",
            ChannelType::Custom(name) => name,
        }
    }
}

/// 알림 채널 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertChannel {
    /// 채널 ID
    pub id: String,
    /// 채널 타입
    pub channel_type: ChannelType,
    /// 대상 URL (웹훅, Slack 등)
    pub url: Option<String>,
    /// 이메일 수신자 (Email 채널)
    pub recipients: Vec<String>,
    /// Slack 채널명 (#channel)
    pub slack_channel: Option<String>,
    /// 추가 헤더
    pub headers: HashMap<String, String>,
    /// 활성화 여부
    pub enabled: bool,
    /// 재시도 횟수
    pub retry_count: u32,
    /// 재시도 간격 (밀리초)
    pub retry_delay_ms: u64,
}

impl AlertChannel {
    /// 웹훅 채널 생성
    pub fn webhook(url: &str) -> Self {
        Self {
            id: format!("webhook_{}", generate_id()),
            channel_type: ChannelType::Webhook,
            url: Some(url.to_string()),
            recipients: Vec::new(),
            slack_channel: None,
            headers: HashMap::new(),
            enabled: true,
            retry_count: 3,
            retry_delay_ms: 1000,
        }
    }

    /// 이메일 채널 생성
    pub fn email(recipients: Vec<String>) -> Self {
        Self {
            id: format!("email_{}", generate_id()),
            channel_type: ChannelType::Email,
            url: None,
            recipients,
            slack_channel: None,
            headers: HashMap::new(),
            enabled: true,
            retry_count: 3,
            retry_delay_ms: 1000,
        }
    }

    /// Slack 채널 생성
    pub fn slack(webhook_url: &str, channel: &str) -> Self {
        Self {
            id: format!("slack_{}", generate_id()),
            channel_type: ChannelType::Slack,
            url: Some(webhook_url.to_string()),
            recipients: Vec::new(),
            slack_channel: Some(channel.to_string()),
            headers: HashMap::new(),
            enabled: true,
            retry_count: 3,
            retry_delay_ms: 1000,
        }
    }

    /// Discord 채널 생성
    pub fn discord(webhook_url: &str) -> Self {
        Self {
            id: format!("discord_{}", generate_id()),
            channel_type: ChannelType::Discord,
            url: Some(webhook_url.to_string()),
            recipients: Vec::new(),
            slack_channel: None,
            headers: HashMap::new(),
            enabled: true,
            retry_count: 3,
            retry_delay_ms: 1000,
        }
    }

    /// Teams 채널 생성
    pub fn teams(webhook_url: &str) -> Self {
        Self {
            id: format!("teams_{}", generate_id()),
            channel_type: ChannelType::Teams,
            url: Some(webhook_url.to_string()),
            recipients: Vec::new(),
            slack_channel: None,
            headers: HashMap::new(),
            enabled: true,
            retry_count: 3,
            retry_delay_ms: 1000,
        }
    }

    /// 헤더 추가
    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }

    /// 재시도 설정
    pub fn with_retry(mut self, count: u32, delay_ms: u64) -> Self {
        self.retry_count = count;
        self.retry_delay_ms = delay_ms;
        self
    }

    /// 비활성화
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// 알림 규칙 조건
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleCondition {
    /// 심각도 기준 (해당 심각도 이상)
    SeverityThreshold(ProblemSeverity),
    /// 특정 문제 유형
    ProblemTypes(Vec<ProblemType>),
    /// 모든 문제
    All,
    /// 커스텀 조건 (문자열 표현)
    Custom(String),
}

/// 알림 규칙
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    /// 규칙 ID
    pub id: String,
    /// 규칙 이름
    pub name: String,
    /// 조건
    pub condition: RuleCondition,
    /// 적용할 채널 ID 목록 (비어있으면 모든 채널)
    pub channel_ids: Vec<String>,
    /// 쿨다운 (동일 문제 재알림 방지, 초)
    pub cooldown_secs: u64,
    /// 활성화 여부
    pub enabled: bool,
}

impl AlertRule {
    /// 심각도 기준 규칙 생성
    pub fn severity_threshold(severity: ProblemSeverity) -> Self {
        Self {
            id: format!("rule_{}", generate_id()),
            name: format!("{} or higher alert", severity.name()),
            condition: RuleCondition::SeverityThreshold(severity),
            channel_ids: Vec::new(),
            cooldown_secs: 300, // 5분
            enabled: true,
        }
    }

    /// 특정 문제 유형 규칙 생성
    pub fn problem_types(types: Vec<ProblemType>) -> Self {
        Self {
            id: format!("rule_{}", generate_id()),
            name: "Specific problem type alert".to_string(),
            condition: RuleCondition::ProblemTypes(types),
            channel_ids: Vec::new(),
            cooldown_secs: 300,
            enabled: true,
        }
    }

    /// 모든 문제 알림 규칙
    pub fn all() -> Self {
        Self {
            id: format!("rule_{}", generate_id()),
            name: "All problems alert".to_string(),
            condition: RuleCondition::All,
            channel_ids: Vec::new(),
            cooldown_secs: 60,
            enabled: true,
        }
    }

    /// 규칙 이름 설정
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// 특정 채널에만 적용
    pub fn for_channels(mut self, channel_ids: Vec<String>) -> Self {
        self.channel_ids = channel_ids;
        self
    }

    /// 쿨다운 설정
    pub fn with_cooldown(mut self, secs: u64) -> Self {
        self.cooldown_secs = secs;
        self
    }

    /// 문제가 규칙에 매칭되는지 확인
    pub fn matches(&self, problem: &DetectedProblem) -> bool {
        if !self.enabled {
            return false;
        }

        match &self.condition {
            RuleCondition::All => true,
            RuleCondition::SeverityThreshold(threshold) => problem.severity >= *threshold,
            RuleCondition::ProblemTypes(types) => types.contains(&problem.problem_type),
            RuleCondition::Custom(_) => true, // 커스텀은 항상 매칭 (외부에서 처리)
        }
    }
}

/// 알림 객체
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    /// 알림 ID
    pub id: String,
    /// 문제 ID
    pub problem_id: String,
    /// 심각도
    pub severity: ProblemSeverity,
    /// 문제 유형
    pub problem_type: ProblemType,
    /// 제목
    pub title: String,
    /// 설명
    pub description: String,
    /// 영향
    pub impact: String,
    /// 권장 조치
    pub recommendation: String,
    /// 생성 시간 (Unix timestamp)
    pub created_at: u64,
    /// 소스 (서버 이름 등)
    pub source: String,
}

impl Alert {
    /// DetectedProblem에서 Alert 생성
    pub fn from_problem(problem: &DetectedProblem, source: &str) -> Self {
        Self {
            id: format!("alert_{}", generate_id()),
            problem_id: problem.id.clone(),
            severity: problem.severity.clone(),
            problem_type: problem.problem_type.clone(),
            title: format!(
                "[{}] {}",
                problem.severity.name(),
                problem.problem_type.name()
            ),
            description: problem.description.clone(),
            impact: problem.impact.clone(),
            recommendation: problem.recommendation.clone(),
            created_at: current_timestamp(),
            source: source.to_string(),
        }
    }

    /// JSON 직렬화
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Slack 메시지 포맷
    pub fn to_slack_message(&self) -> String {
        let color = match self.severity {
            ProblemSeverity::Info => "#36a64f",
            ProblemSeverity::Warning => "#ff9800",
            ProblemSeverity::Critical => "#f44336",
        };

        serde_json::json!({
            "attachments": [{
                "color": color,
                "title": self.title,
                "text": self.description,
                "fields": [
                    {
                        "title": "Impact",
                        "value": self.impact,
                        "short": true
                    },
                    {
                        "title": "Recommendation",
                        "value": self.recommendation,
                        "short": true
                    },
                    {
                        "title": "Source",
                        "value": self.source,
                        "short": true
                    }
                ],
                "footer": "Zeta4j Alert System",
                "ts": self.created_at
            }]
        })
        .to_string()
    }

    /// Discord 메시지 포맷
    pub fn to_discord_message(&self) -> String {
        let color = match self.severity {
            ProblemSeverity::Info => 0x36a64f,
            ProblemSeverity::Warning => 0xff9800,
            ProblemSeverity::Critical => 0xf44336,
        };

        serde_json::json!({
            "embeds": [{
                "title": self.title,
                "description": self.description,
                "color": color,
                "fields": [
                    {
                        "name": "Impact",
                        "value": self.impact,
                        "inline": true
                    },
                    {
                        "name": "Recommendation",
                        "value": self.recommendation,
                        "inline": true
                    },
                    {
                        "name": "Source",
                        "value": self.source,
                        "inline": true
                    }
                ],
                "footer": {
                    "text": "Zeta4j Alert System"
                },
                "timestamp": format_timestamp(self.created_at)
            }]
        })
        .to_string()
    }

    /// Teams 메시지 포맷
    pub fn to_teams_message(&self) -> String {
        let theme_color = match self.severity {
            ProblemSeverity::Info => "36a64f",
            ProblemSeverity::Warning => "ff9800",
            ProblemSeverity::Critical => "f44336",
        };

        serde_json::json!({
            "@type": "MessageCard",
            "@context": "http://schema.org/extensions",
            "themeColor": theme_color,
            "summary": self.title,
            "sections": [{
                "activityTitle": self.title,
                "facts": [
                    {
                        "name": "Description",
                        "value": self.description
                    },
                    {
                        "name": "Impact",
                        "value": self.impact
                    },
                    {
                        "name": "Recommendation",
                        "value": self.recommendation
                    },
                    {
                        "name": "Source",
                        "value": self.source
                    }
                ],
                "markdown": true
            }]
        })
        .to_string()
    }
}

/// 알림 전송 결과
#[derive(Debug, Clone)]
pub struct AlertResult {
    /// 채널 ID
    pub channel_id: String,
    /// 성공 여부
    pub success: bool,
    /// 에러 메시지
    pub error: Option<String>,
    /// 시도 횟수
    pub attempts: u32,
    /// 응답 시간 (밀리초)
    pub response_time_ms: u64,
}

/// 알림 히스토리
#[derive(Debug, Clone)]
struct AlertHistory {
    /// 마지막 알림 시간 (문제 ID -> timestamp)
    last_alert: HashMap<String, u64>,
}

impl AlertHistory {
    fn new() -> Self {
        Self {
            last_alert: HashMap::new(),
        }
    }

    fn can_alert(&self, problem_id: &str, cooldown_secs: u64) -> bool {
        if let Some(last) = self.last_alert.get(problem_id) {
            let now = current_timestamp();
            (now - last) >= cooldown_secs
        } else {
            true
        }
    }

    fn record(&mut self, problem_id: &str) {
        self.last_alert
            .insert(problem_id.to_string(), current_timestamp());
    }

    fn cleanup(&mut self, max_age_secs: u64) {
        let now = current_timestamp();
        self.last_alert
            .retain(|_, &mut ts| (now - ts) < max_age_secs);
    }
}

/// 알림 통계
#[derive(Debug, Clone, Default)]
pub struct AlertStats {
    /// 전송된 알림 수
    pub alerts_sent: u64,
    /// 성공한 알림 수
    pub alerts_success: u64,
    /// 실패한 알림 수
    pub alerts_failed: u64,
    /// 쿨다운으로 억제된 수
    pub alerts_suppressed: u64,
    /// 채널별 전송 수
    pub by_channel: HashMap<String, u64>,
    /// 심각도별 전송 수
    pub by_severity: HashMap<String, u64>,
}

/// 알림 관리자
pub struct AlertManager {
    /// 채널 목록
    channels: Vec<AlertChannel>,
    /// 규칙 목록
    rules: Vec<AlertRule>,
    /// 알림 히스토리 (쿨다운 관리)
    history: Arc<RwLock<AlertHistory>>,
    /// 통계
    stats: Arc<RwLock<AlertStats>>,
    /// 소스 이름
    source: String,
    /// HTTP 클라이언트 (실제 전송용)
    #[cfg(feature = "alert-http")]
    client: reqwest::Client,
}

impl AlertManager {
    /// 새 AlertManager 생성
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            rules: Vec::new(),
            history: Arc::new(RwLock::new(AlertHistory::new())),
            stats: Arc::new(RwLock::new(AlertStats::default())),
            source: "zeta4j".to_string(),
            #[cfg(feature = "alert-http")]
            client: reqwest::Client::new(),
        }
    }

    /// 소스 이름 설정
    pub fn with_source(mut self, source: &str) -> Self {
        self.source = source.to_string();
        self
    }

    /// 채널 추가
    pub fn with_channel(mut self, channel: AlertChannel) -> Self {
        self.channels.push(channel);
        self
    }

    /// 규칙 추가
    pub fn with_rule(mut self, rule: AlertRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// 채널 추가 (mutable)
    pub fn add_channel(&mut self, channel: AlertChannel) {
        self.channels.push(channel);
    }

    /// 규칙 추가 (mutable)
    pub fn add_rule(&mut self, rule: AlertRule) {
        self.rules.push(rule);
    }

    /// 채널 제거
    pub fn remove_channel(&mut self, channel_id: &str) {
        self.channels.retain(|c| c.id != channel_id);
    }

    /// 규칙 제거
    pub fn remove_rule(&mut self, rule_id: &str) {
        self.rules.retain(|r| r.id != rule_id);
    }

    /// 채널 목록 조회
    pub fn channels(&self) -> &[AlertChannel] {
        &self.channels
    }

    /// 규칙 목록 조회
    pub fn rules(&self) -> &[AlertRule] {
        &self.rules
    }

    /// 통계 조회
    pub async fn stats(&self) -> AlertStats {
        self.stats.read().await.clone()
    }

    /// 문제에 대한 알림 전송
    pub async fn notify(&self, problem: &DetectedProblem) -> Vec<AlertResult> {
        let alert = Alert::from_problem(problem, &self.source);
        self.send_alert(&alert, problem).await
    }

    /// 알림 전송 (내부)
    async fn send_alert(&self, alert: &Alert, problem: &DetectedProblem) -> Vec<AlertResult> {
        let mut results = Vec::new();

        // 매칭되는 규칙 찾기
        let matching_rules: Vec<&AlertRule> =
            self.rules.iter().filter(|r| r.matches(problem)).collect();

        if matching_rules.is_empty() {
            return results;
        }

        // 쿨다운 체크
        for rule in &matching_rules {
            let can_alert = {
                let history = self.history.read().await;
                history.can_alert(&problem.id, rule.cooldown_secs)
            };

            if !can_alert {
                // 쿨다운 중
                let mut stats = self.stats.write().await;
                stats.alerts_suppressed += 1;
                continue;
            }

            // 적용할 채널 결정
            let target_channels: Vec<&AlertChannel> = if rule.channel_ids.is_empty() {
                self.channels.iter().filter(|c| c.enabled).collect()
            } else {
                self.channels
                    .iter()
                    .filter(|c| c.enabled && rule.channel_ids.contains(&c.id))
                    .collect()
            };

            // 각 채널로 전송
            for channel in target_channels {
                let result = self.send_to_channel(alert, channel).await;

                // 통계 업데이트
                {
                    let mut stats = self.stats.write().await;
                    stats.alerts_sent += 1;
                    if result.success {
                        stats.alerts_success += 1;
                    } else {
                        stats.alerts_failed += 1;
                    }
                    *stats.by_channel.entry(channel.id.clone()).or_insert(0) += 1;
                    *stats
                        .by_severity
                        .entry(problem.severity.name().to_string())
                        .or_insert(0) += 1;
                }

                results.push(result);
            }

            // 히스토리 기록
            {
                let mut history = self.history.write().await;
                history.record(&problem.id);
            }
        }

        results
    }

    /// 채널로 알림 전송
    async fn send_to_channel(&self, alert: &Alert, channel: &AlertChannel) -> AlertResult {
        let start = std::time::Instant::now();

        // 메시지 포맷팅
        let _message = match channel.channel_type {
            ChannelType::Slack => alert.to_slack_message(),
            ChannelType::Discord => alert.to_discord_message(),
            ChannelType::Teams => alert.to_teams_message(),
            _ => alert.to_json(),
        };

        // 실제 HTTP 전송 (feature flag로 제어)
        #[cfg(feature = "alert-http")]
        {
            if let Some(url) = &channel.url {
                let mut attempts = 0;
                let mut last_error = None;

                while attempts < channel.retry_count {
                    attempts += 1;

                    let mut request = self
                        .client
                        .post(url)
                        .header("Content-Type", "application/json")
                        .body(message.clone());

                    for (key, value) in &channel.headers {
                        request = request.header(key, value);
                    }

                    match request.send().await {
                        Ok(response) if response.status().is_success() => {
                            return AlertResult {
                                channel_id: channel.id.clone(),
                                success: true,
                                error: None,
                                attempts,
                                response_time_ms: start.elapsed().as_millis() as u64,
                            };
                        }
                        Ok(response) => {
                            last_error = Some(format!("HTTP {}", response.status()));
                        }
                        Err(e) => {
                            last_error = Some(e.to_string());
                        }
                    }

                    if attempts < channel.retry_count {
                        tokio::time::sleep(Duration::from_millis(channel.retry_delay_ms)).await;
                    }
                }

                return AlertResult {
                    channel_id: channel.id.clone(),
                    success: false,
                    error: last_error,
                    attempts,
                    response_time_ms: start.elapsed().as_millis() as u64,
                };
            }
        }

        // HTTP 미지원 또는 URL 없음 - 시뮬레이션 모드
        AlertResult {
            channel_id: channel.id.clone(),
            success: true,
            error: None,
            attempts: 1,
            response_time_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// 테스트 알림 전송
    pub async fn send_test_alert(&self) -> Vec<AlertResult> {
        let test_problem = DetectedProblem::new(
            ProblemType::HighMemoryUsage,
            ProblemSeverity::Info,
            "This is a test alert.".to_string(),
            "None".to_string(),
            "If you see this message, the alert system is working correctly.".to_string(),
        );

        self.notify(&test_problem).await
    }

    /// 히스토리 정리
    pub async fn cleanup_history(&self, max_age_secs: u64) {
        let mut history = self.history.write().await;
        history.cleanup(max_age_secs);
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

// 헬퍼 함수

fn generate_id() -> String {
    format!("{:x}", current_timestamp() ^ rand::random::<u64>())
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn format_timestamp(ts: u64) -> String {
    // ISO 8601 형식으로 변환
    let duration = Duration::from_secs(ts);
    let datetime = UNIX_EPOCH + duration;
    format!("{:?}", datetime)
}

// ============================================================================
// 단위 테스트
