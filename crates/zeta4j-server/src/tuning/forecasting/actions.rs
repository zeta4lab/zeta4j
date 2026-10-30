//! 자동 최적화 액션 엔진
//!
//! 예측 결과와 패턴을 기반으로 최적화 액션을 생성하고 실행합니다.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::collector::MetricType;
use super::patterns::RecognizedPattern;
use super::predictor::Prediction;

/// 최적화 액션 유형
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OptimizationAction {
    /// 캐시 사전 워밍업
    WarmCache {
        /// 워밍업할 쿼리 패턴
        query_patterns: Vec<String>,
        /// 우선순위
        priority: Priority,
    },
    /// 인덱스 사전 생성 권장
    PrepareIndex {
        /// 레이블
        label: String,
        /// 프로퍼티
        properties: Vec<String>,
        /// 예상 효과
        expected_improvement: f64,
    },
    /// 연결 풀 확장
    ScaleConnections {
        /// 현재 연결 수
        current_count: usize,
        /// 목표 연결 수
        target_count: usize,
    },
    /// 메모리 할당 조정
    AdjustMemory {
        /// 컴포넌트
        component: MemoryComponent,
        /// 현재 크기 (bytes)
        current_bytes: usize,
        /// 목표 크기 (bytes)
        target_bytes: usize,
    },
    /// 알림 발송
    SendAlert {
        /// 심각도
        severity: AlertSeverity,
        /// 메시지
        message: String,
        /// 관련 메트릭
        metric: Option<MetricType>,
    },
    /// 쿼리 캐시 크기 조정
    ResizeQueryCache {
        /// 현재 크기
        current_size: usize,
        /// 목표 크기
        target_size: usize,
    },
    /// 배치 작업 스케줄링
    ScheduleBatchJob {
        /// 작업 유형
        job_type: BatchJobType,
        /// 실행 시간
        scheduled_time: DateTime<Utc>,
    },
}

impl OptimizationAction {
    /// 액션 유형 반환
    pub fn action_type(&self) -> ActionType {
        match self {
            Self::WarmCache { .. } => ActionType::WarmCache,
            Self::PrepareIndex { .. } => ActionType::PrepareIndex,
            Self::ScaleConnections { .. } => ActionType::ScaleConnections,
            Self::AdjustMemory { .. } => ActionType::AdjustMemory,
            Self::SendAlert { .. } => ActionType::SendAlert,
            Self::ResizeQueryCache { .. } => ActionType::ResizeQueryCache,
            Self::ScheduleBatchJob { .. } => ActionType::ScheduleBatchJob,
        }
    }

    /// 액션 설명 생성
    pub fn description(&self) -> String {
        match self {
            Self::WarmCache {
                query_patterns,
                priority,
            } => {
                format!(
                    "Cache warmup: {} patterns, priority {:?}",
                    query_patterns.len(),
                    priority
                )
            }
            Self::PrepareIndex {
                label,
                properties,
                expected_improvement,
            } => {
                format!(
                    "Index creation recommended: {}({}) - expected improvement {:.1}%",
                    label,
                    properties.join(", "),
                    expected_improvement * 100.0
                )
            }
            Self::ScaleConnections {
                current_count,
                target_count,
            } => {
                format!(
                    "Connection pool adjustment: {} -> {}",
                    current_count, target_count
                )
            }
            Self::AdjustMemory {
                component,
                current_bytes,
                target_bytes,
            } => {
                format!(
                    "Memory adjustment {:?}: {} -> {} MB",
                    component,
                    current_bytes / 1024 / 1024,
                    target_bytes / 1024 / 1024
                )
            }
            Self::SendAlert {
                severity, message, ..
            } => {
                format!("Alert [{:?}]: {}", severity, message)
            }
            Self::ResizeQueryCache {
                current_size,
                target_size,
            } => {
                format!(
                    "Query cache adjustment: {} -> {}",
                    current_size, target_size
                )
            }
            Self::ScheduleBatchJob {
                job_type,
                scheduled_time,
            } => {
                format!("Batch job schedule: {:?} @ {}", job_type, scheduled_time)
            }
        }
    }
}

/// 액션 유형 (쿨다운 관리용)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionType {
    WarmCache,
    PrepareIndex,
    ScaleConnections,
    AdjustMemory,
    SendAlert,
    ResizeQueryCache,
    ScheduleBatchJob,
}

/// 우선순위
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

/// 알림 심각도
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// 메모리 컴포넌트
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MemoryComponent {
    /// 페이지 캐시
    PageCache,
    /// 쿼리 캐시
    QueryCache,
    /// 인덱스 캐시
    IndexCache,
    /// 트랜잭션 버퍼
    TransactionBuffer,
}

/// 배치 작업 유형
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatchJobType {
    /// 통계 갱신
    UpdateStatistics,
    /// 인덱스 재구축
    RebuildIndex,
    /// 가비지 컬렉션
    GarbageCollection,
    /// 백업
    Backup,
}

/// 액션 실행 결과
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionResult {
    /// 성공
    Success { details: String },
    /// 스킵 (쿨다운 등)
    Skipped { reason: String },
    /// 실패
    Failed { error: String },
    /// 보류 (승인 필요)
    Pending { requires_approval: bool },
}

/// 액션 트리거
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionTrigger {
    /// 예측 기반
    Prediction {
        metric: MetricType,
        predicted_value: f64,
        threshold: f64,
    },
    /// 패턴 기반
    Pattern { pattern_description: String },
    /// 수동 요청
    Manual { requested_by: String },
    /// 스케줄 기반
    Scheduled { schedule_name: String },
}

/// 액션 실행 기록
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionExecution {
    /// 실행된 액션
    pub action: OptimizationAction,
    /// 실행 시간
    pub executed_at: DateTime<Utc>,
    /// 소요 시간
    pub duration: Duration,
    /// 결과
    pub result: ActionResult,
    /// 트리거
    pub trigger: ActionTrigger,
}

/// 액션 영향 평가
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionImpact {
    /// 메트릭 변화 (이전 → 이후)
    pub metric_changes: HashMap<MetricType, (f64, f64)>,
    /// 전체 효과 점수 (-1.0 ~ 1.0)
    pub effectiveness_score: f64,
    /// 평가 시간
    pub evaluated_at: DateTime<Utc>,
}

/// 액션 엔진 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionEngineConfig {
    /// 활성화된 액션 유형
    pub enabled_actions: HashSet<ActionType>,
    /// 쿨다운 설정 (초)
    pub cooldowns: HashMap<ActionType, u64>,
    /// 최대 이력 크기
    pub max_history_size: usize,
    /// 자동 실행 활성화
    pub auto_execute: bool,
    /// 알림 임계값
    pub alert_thresholds: HashMap<MetricType, f64>,
}

impl Default for ActionEngineConfig {
    fn default() -> Self {
        let mut enabled = HashSet::new();
        enabled.insert(ActionType::WarmCache);
        enabled.insert(ActionType::SendAlert);
        enabled.insert(ActionType::ResizeQueryCache);

        let mut cooldowns = HashMap::new();
        cooldowns.insert(ActionType::WarmCache, 3600); // 1시간
        cooldowns.insert(ActionType::PrepareIndex, 86400); // 24시간
        cooldowns.insert(ActionType::ScaleConnections, 300); // 5분
        cooldowns.insert(ActionType::AdjustMemory, 600); // 10분
        cooldowns.insert(ActionType::SendAlert, 1800); // 30분
        cooldowns.insert(ActionType::ResizeQueryCache, 1800);
        cooldowns.insert(ActionType::ScheduleBatchJob, 3600);

        Self {
            enabled_actions: enabled,
            cooldowns,
            max_history_size: 1000,
            auto_execute: false,
            alert_thresholds: HashMap::new(),
        }
    }
}

/// 액션 엔진
pub struct ActionEngine {
    /// 설정
    config: ActionEngineConfig,
    /// 실행 이력
    history: RwLock<VecDeque<ActionExecution>>,
    /// 마지막 실행 시간 (쿨다운용)
    last_execution: RwLock<HashMap<ActionType, DateTime<Utc>>>,
    /// 보류 중인 액션
    pending_actions: RwLock<Vec<(OptimizationAction, ActionTrigger)>>,
}

impl ActionEngine {
    /// 새 액션 엔진 생성
    pub fn new(config: ActionEngineConfig) -> Self {
        Self {
            config,
            history: RwLock::new(VecDeque::new()),
            last_execution: RwLock::new(HashMap::new()),
            pending_actions: RwLock::new(Vec::new()),
        }
    }

    /// 예측 기반 액션 생성
    pub fn generate_actions(
        &self,
        predictions: &[Prediction],
        patterns: &[RecognizedPattern],
    ) -> Vec<(OptimizationAction, ActionTrigger)> {
        let mut actions = Vec::new();

        // 예측 기반 액션
        for pred in predictions {
            // 임계값 확인
            if let Some(&threshold) = self.config.alert_thresholds.get(&pred.metric)
                && pred.exceeds_threshold(threshold)
            {
                let action = OptimizationAction::SendAlert {
                    severity: self.determine_severity(pred.value, threshold),
                    message: format!(
                        "{:?} predicted value {:.1} is expected to exceed threshold {:.1} ({})",
                        pred.metric, pred.value, threshold, pred.timestamp
                    ),
                    metric: Some(pred.metric),
                };

                let trigger = ActionTrigger::Prediction {
                    metric: pred.metric,
                    predicted_value: pred.value,
                    threshold,
                };

                if self.is_action_enabled(&action) && !self.is_in_cooldown(&action) {
                    actions.push((action, trigger));
                }
            }

            // 높은 QPS 예측 시 캐시 워밍업
            if pred.metric == MetricType::QueriesPerSecond && pred.value > 1000.0 {
                let action = OptimizationAction::WarmCache {
                    query_patterns: vec!["MATCH (n) RETURN n".to_string()],
                    priority: if pred.value > 5000.0 {
                        Priority::High
                    } else {
                        Priority::Normal
                    },
                };

                let trigger = ActionTrigger::Prediction {
                    metric: pred.metric,
                    predicted_value: pred.value,
                    threshold: 1000.0,
                };

                if self.is_action_enabled(&action) && !self.is_in_cooldown(&action) {
                    actions.push((action, trigger));
                }
            }

            // 연결 수 급증 예측 시 풀 확장
            if pred.metric == MetricType::ActiveConnections && pred.value > 80.0 {
                let action = OptimizationAction::ScaleConnections {
                    current_count: 100,
                    target_count: (pred.value * 1.5) as usize,
                };

                let trigger = ActionTrigger::Prediction {
                    metric: pred.metric,
                    predicted_value: pred.value,
                    threshold: 80.0,
                };

                if self.is_action_enabled(&action) && !self.is_in_cooldown(&action) {
                    actions.push((action, trigger));
                }
            }
        }

        // 패턴 기반 액션
        for pattern in patterns {
            if pattern.confidence > 0.7 {
                // 피크 시간 전 캐시 워밍업
                if let super::patterns::PatternType::Hourly { peak_hours, .. } =
                    &pattern.pattern_type
                    && !peak_hours.is_empty()
                {
                    let action = OptimizationAction::WarmCache {
                        query_patterns: vec!["Peak hour warmup".to_string()],
                        priority: Priority::Normal,
                    };

                    let trigger = ActionTrigger::Pattern {
                        pattern_description: format!(
                            "Hourly pattern: peak at hour {}",
                            peak_hours[0]
                        ),
                    };

                    if self.is_action_enabled(&action) && !self.is_in_cooldown(&action) {
                        actions.push((action, trigger));
                    }
                }
            }
        }

        actions
    }

    /// 액션 실행
    pub fn execute(&self, action: OptimizationAction, trigger: ActionTrigger) -> ActionResult {
        let start = std::time::Instant::now();

        // 쿨다운 확인
        if self.is_in_cooldown(&action) {
            return ActionResult::Skipped {
                reason: "In cooldown period".to_string(),
            };
        }

        // 활성화 확인
        if !self.is_action_enabled(&action) {
            return ActionResult::Skipped {
                reason: "Disabled action".to_string(),
            };
        }

        // 자동 실행 비활성화 시 보류
        if !self.config.auto_execute {
            self.pending_actions
                .write()
                .push((action.clone(), trigger.clone()));
            return ActionResult::Pending {
                requires_approval: true,
            };
        }

        // 실제 실행 (여기서는 시뮬레이션)
        let result = self.execute_action_internal(&action);

        // 이력 기록
        let execution = ActionExecution {
            action: action.clone(),
            executed_at: Utc::now(),
            duration: start.elapsed(),
            result: result.clone(),
            trigger,
        };

        self.record_execution(execution);

        result
    }

    /// 내부 액션 실행 (실제 구현)
    fn execute_action_internal(&self, action: &OptimizationAction) -> ActionResult {
        match action {
            OptimizationAction::SendAlert {
                message, severity, ..
            } => {
                // 실제로는 알림 시스템과 연동
                tracing::info!("[{:?}] {}", severity, message);
                ActionResult::Success {
                    details: "Alert sent successfully".to_string(),
                }
            }
            OptimizationAction::WarmCache { query_patterns, .. } => {
                // 실제로는 캐시 워밍업 수행
                ActionResult::Success {
                    details: format!("{} patterns warmed up", query_patterns.len()),
                }
            }
            OptimizationAction::ScaleConnections { target_count, .. } => {
                // 실제로는 연결 풀 조정
                ActionResult::Success {
                    details: format!("Connection pool adjusted to {}", target_count),
                }
            }
            OptimizationAction::AdjustMemory {
                component,
                target_bytes,
                ..
            } => ActionResult::Success {
                details: format!(
                    "{:?} memory adjusted to {}MB",
                    component,
                    target_bytes / 1024 / 1024
                ),
            },
            OptimizationAction::PrepareIndex {
                label, properties, ..
            } => ActionResult::Success {
                details: format!(
                    "Index {}({}) creation recommended",
                    label,
                    properties.join(", ")
                ),
            },
            OptimizationAction::ResizeQueryCache { target_size, .. } => ActionResult::Success {
                details: format!("Query cache resized to {}", target_size),
            },
            OptimizationAction::ScheduleBatchJob {
                job_type,
                scheduled_time,
            } => ActionResult::Success {
                details: format!("{:?} job scheduled at {}", job_type, scheduled_time),
            },
        }
    }

    /// 쿨다운 상태 확인
    fn is_in_cooldown(&self, action: &OptimizationAction) -> bool {
        let action_type = action.action_type();
        let last_exec = self.last_execution.read();

        if let Some(&last_time) = last_exec.get(&action_type)
            && let Some(&cooldown_secs) = self.config.cooldowns.get(&action_type)
        {
            let cooldown = chrono::Duration::seconds(cooldown_secs as i64);
            return Utc::now() - last_time < cooldown;
        }

        false
    }

    /// 액션 활성화 여부
    fn is_action_enabled(&self, action: &OptimizationAction) -> bool {
        self.config.enabled_actions.contains(&action.action_type())
    }

    /// 심각도 결정
    fn determine_severity(&self, value: f64, threshold: f64) -> AlertSeverity {
        let ratio = value / threshold;
        if ratio > 2.0 {
            AlertSeverity::Critical
        } else if ratio > 1.5 {
            AlertSeverity::Error
        } else if ratio > 1.2 {
            AlertSeverity::Warning
        } else {
            AlertSeverity::Info
        }
    }

    /// 실행 이력 기록
    fn record_execution(&self, execution: ActionExecution) {
        // 마지막 실행 시간 업데이트
        self.last_execution
            .write()
            .insert(execution.action.action_type(), execution.executed_at);

        // 이력 추가
        let mut history = self.history.write();
        history.push_back(execution);

        // 최대 크기 유지
        while history.len() > self.config.max_history_size {
            history.pop_front();
        }
    }

    /// 실행 이력 조회
    pub fn get_history(&self, since: Option<DateTime<Utc>>) -> Vec<ActionExecution> {
        let history = self.history.read();
        match since {
            Some(since_time) => history
                .iter()
                .filter(|e| e.executed_at >= since_time)
                .cloned()
                .collect(),
            None => history.iter().cloned().collect(),
        }
    }

    /// 보류 중인 액션 조회
    pub fn get_pending_actions(&self) -> Vec<(OptimizationAction, ActionTrigger)> {
        self.pending_actions.read().clone()
    }

    /// 보류 중인 액션 승인 및 실행
    pub fn approve_pending(&self, index: usize) -> Option<ActionResult> {
        let mut pending = self.pending_actions.write();
        if index < pending.len() {
            let (action, trigger) = pending.remove(index);
            drop(pending);

            // 자동 실행 임시 활성화
            let result = self.execute_action_internal(&action);

            let execution = ActionExecution {
                action,
                executed_at: Utc::now(),
                duration: Duration::from_millis(1),
                result: result.clone(),
                trigger,
            };
            self.record_execution(execution);

            Some(result)
        } else {
            None
        }
    }

    /// 보류 중인 액션 거부
    pub fn reject_pending(&self, index: usize) -> bool {
        let mut pending = self.pending_actions.write();
        if index < pending.len() {
            pending.remove(index);
            true
        } else {
            false
        }
    }

    /// 액션 영향 평가
    pub fn evaluate_impact(&self, _execution: &ActionExecution) -> ActionImpact {
        // 실제로는 실행 전후 메트릭 비교
        ActionImpact {
            metric_changes: HashMap::new(),
            effectiveness_score: 0.5, // 기본값
            evaluated_at: Utc::now(),
        }
    }

    /// 액션 유형 활성화
    pub fn enable_action(&mut self, action_type: ActionType) {
        self.config.enabled_actions.insert(action_type);
    }

    /// 액션 유형 비활성화
    pub fn disable_action(&mut self, action_type: ActionType) {
        self.config.enabled_actions.remove(&action_type);
    }

    /// 알림 임계값 설정
    pub fn set_alert_threshold(&mut self, metric: MetricType, threshold: f64) {
        self.config.alert_thresholds.insert(metric, threshold);
    }

    /// 자동 실행 설정
    pub fn set_auto_execute(&mut self, enabled: bool) {
        self.config.auto_execute = enabled;
    }
}

impl Default for ActionEngine {
    fn default() -> Self {
        Self::new(ActionEngineConfig::default())
    }
}
