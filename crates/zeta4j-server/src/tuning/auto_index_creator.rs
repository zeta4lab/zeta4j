//! 자동 인덱스 생성 모듈
//!
//! IndexAdvisor의 추천을 실제 인덱스로 자동 생성합니다.
//!
//! # 기능
//!
//! - 자동 모드: 추천 즉시 적용
//! - 확인 모드: 사용자 승인 후 적용
//! - 우선순위 기반 선택적 적용
//! - 생성 결과 보고
//!
//! # 사용 예시
//!
//! ```ignore
//! use zeta4j_server::tuning::{AutoIndexCreator, CreationMode, IndexAdvisor};
//!
//! // 확인 모드로 생성
//! let mut creator = AutoIndexCreator::new(CreationMode::RequireApproval);
//!
//! // 추천 받기
//! let recommendations = advisor.recommend();
//!
//! // 승인 대기 큐에 추가
//! for rec in recommendations {
//!     creator.queue_for_approval(rec);
//! }
//!
//! // 승인된 추천만 적용
//! creator.approve_by_label("Person");
//! let report = creator.apply_approved(&index_manager);
//! ```

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::index_advisor::{IndexRecommendation, RecommendedIndexType};

/// 인덱스 생성 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CreationMode {
    /// 자동 생성 (추천 즉시 적용)
    Automatic,
    /// 승인 필요 (사용자 확인 후 적용)
    #[default]
    RequireApproval,
    /// 시뮬레이션 (실제 생성 없이 보고만)
    DryRun,
}

impl CreationMode {
    /// 모드 이름 반환
    pub fn name(&self) -> &str {
        match self {
            CreationMode::Automatic => "automatic",
            CreationMode::RequireApproval => "require_approval",
            CreationMode::DryRun => "dry_run",
        }
    }
}

/// 인덱스 생성 결과
#[derive(Debug, Clone)]
pub struct IndexCreationResult {
    /// 인덱스 이름
    pub index_name: String,
    /// 대상 라벨
    pub label: String,
    /// 대상 속성
    pub properties: Vec<String>,
    /// 인덱스 타입
    pub index_type: RecommendedIndexType,
    /// 성공 여부
    pub success: bool,
    /// 메시지
    pub message: String,
    /// 생성 시간 (타임스탬프)
    pub created_at: u64,
    /// 생성 소요 시간 (밀리초)
    pub duration_ms: u64,
}

impl IndexCreationResult {
    /// 성공 결과 생성
    pub fn success(
        index_name: String,
        label: String,
        properties: Vec<String>,
        index_type: RecommendedIndexType,
        duration_ms: u64,
    ) -> Self {
        Self {
            index_name,
            label,
            properties,
            index_type,
            success: true,
            message: "Index creation completed".to_string(),
            created_at: current_timestamp(),
            duration_ms,
        }
    }

    /// 실패 결과 생성
    pub fn failure(
        index_name: String,
        label: String,
        properties: Vec<String>,
        index_type: RecommendedIndexType,
        error: String,
    ) -> Self {
        Self {
            index_name,
            label,
            properties,
            index_type,
            success: false,
            message: error,
            created_at: current_timestamp(),
            duration_ms: 0,
        }
    }

    /// 건너뜀 결과 생성
    pub fn skipped(
        index_name: String,
        label: String,
        properties: Vec<String>,
        index_type: RecommendedIndexType,
        reason: String,
    ) -> Self {
        Self {
            index_name,
            label,
            properties,
            index_type,
            success: false,
            message: format!("Skipped: {}", reason),
            created_at: current_timestamp(),
            duration_ms: 0,
        }
    }
}

/// 승인 대기 중인 추천
#[derive(Debug, Clone)]
pub struct PendingRecommendation {
    /// 원본 추천
    pub recommendation: IndexRecommendation,
    /// 대기열 추가 시간
    pub queued_at: u64,
    /// 승인 여부
    pub approved: bool,
    /// 승인/거부 시간
    pub decision_at: Option<u64>,
    /// 거부 사유
    pub rejection_reason: Option<String>,
}

impl PendingRecommendation {
    /// 새 대기 추천 생성
    pub fn new(recommendation: IndexRecommendation) -> Self {
        Self {
            recommendation,
            queued_at: current_timestamp(),
            approved: false,
            decision_at: None,
            rejection_reason: None,
        }
    }

    /// 승인
    pub fn approve(&mut self) {
        self.approved = true;
        self.decision_at = Some(current_timestamp());
        self.rejection_reason = None;
    }

    /// 거부
    pub fn reject(&mut self, reason: &str) {
        self.approved = false;
        self.decision_at = Some(current_timestamp());
        self.rejection_reason = Some(reason.to_string());
    }

    /// 인덱스 키 생성
    pub fn key(&self) -> String {
        format!(
            "{}_{}",
            self.recommendation.label,
            self.recommendation.properties.join("_")
        )
    }
}

/// 인덱스 생성 보고서
#[derive(Debug, Clone)]
pub struct CreationReport {
    /// 보고서 생성 시간
    pub timestamp: u64,
    /// 생성 모드
    pub mode: CreationMode,
    /// 전체 추천 수
    pub total_recommendations: usize,
    /// 생성 시도 수
    pub attempted: usize,
    /// 성공 수
    pub succeeded: usize,
    /// 실패 수
    pub failed: usize,
    /// 건너뜀 수
    pub skipped: usize,
    /// 상세 결과
    pub results: Vec<IndexCreationResult>,
}

impl CreationReport {
    /// 빈 보고서 생성
    pub fn new(mode: CreationMode) -> Self {
        Self {
            timestamp: current_timestamp(),
            mode,
            total_recommendations: 0,
            attempted: 0,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            results: Vec::new(),
        }
    }

    /// 결과 추가
    pub fn add_result(&mut self, result: IndexCreationResult) {
        if result.success {
            self.succeeded += 1;
            self.attempted += 1;
        } else if result.message.starts_with("Skipped") {
            self.skipped += 1;
        } else {
            self.failed += 1;
            self.attempted += 1;
        }
        self.results.push(result);
    }

    /// 성공률 (퍼센트)
    pub fn success_rate(&self) -> f64 {
        if self.attempted == 0 {
            100.0
        } else {
            (self.succeeded as f64 / self.attempted as f64) * 100.0
        }
    }

    /// 요약 문자열
    pub fn summary(&self) -> String {
        format!(
            "Index creation report ({}): total {} / attempted {} / succeeded {} / failed {} / skipped {} (success rate: {:.1}%)",
            self.mode.name(),
            self.total_recommendations,
            self.attempted,
            self.succeeded,
            self.failed,
            self.skipped,
            self.success_rate()
        )
    }
}

/// 자동 인덱스 생성 설정
#[derive(Debug, Clone)]
pub struct AutoIndexCreatorConfig {
    /// 생성 모드
    pub mode: CreationMode,
    /// 최소 우선순위 점수 (이 이상만 생성)
    pub min_priority_score: f64,
    /// 최대 동시 생성 수
    pub max_concurrent: usize,
    /// 동일 라벨 인덱스 최대 수
    pub max_per_label: usize,
    /// 승인 대기 만료 시간 (초)
    pub approval_timeout_secs: u64,
    /// 실패 시 재시도 횟수
    pub retry_count: usize,
}

impl Default for AutoIndexCreatorConfig {
    fn default() -> Self {
        Self {
            mode: CreationMode::RequireApproval,
            min_priority_score: 100.0,
            max_concurrent: 5,
            max_per_label: 3,
            approval_timeout_secs: 86400, // 24시간
            retry_count: 2,
        }
    }
}

impl AutoIndexCreatorConfig {
    /// 자동 모드 설정
    pub fn automatic() -> Self {
        Self {
            mode: CreationMode::Automatic,
            ..Default::default()
        }
    }

    /// 확인 필요 모드 설정
    pub fn require_approval() -> Self {
        Self {
            mode: CreationMode::RequireApproval,
            ..Default::default()
        }
    }

    /// 시뮬레이션 모드 설정
    pub fn dry_run() -> Self {
        Self {
            mode: CreationMode::DryRun,
            ..Default::default()
        }
    }
}

/// 인덱스 생성기 트레이트
///
/// 다양한 인덱스 매니저를 통일된 방식으로 사용하기 위한 추상화
pub trait IndexCreator {
    /// 인덱스 생성
    fn create_index(
        &mut self,
        name: &str,
        label: &str,
        properties: &[String],
    ) -> Result<(), String>;

    /// 인덱스 존재 확인
    fn has_index(&self, name: &str) -> bool;

    /// 인덱스 삭제
    fn drop_index(&mut self, name: &str) -> Result<(), String>;
}

/// 자동 인덱스 생성기
pub struct AutoIndexCreator {
    /// 설정
    config: AutoIndexCreatorConfig,
    /// 승인 대기 큐
    pending: HashMap<String, PendingRecommendation>,
    /// 생성된 인덱스 목록
    created_indexes: Vec<String>,
    /// 라벨별 인덱스 수
    indexes_per_label: HashMap<String, usize>,
    /// 마지막 보고서
    last_report: Option<CreationReport>,
}

impl AutoIndexCreator {
    /// 새 자동 인덱스 생성기
    pub fn new(config: AutoIndexCreatorConfig) -> Self {
        Self {
            config,
            pending: HashMap::new(),
            created_indexes: Vec::new(),
            indexes_per_label: HashMap::new(),
            last_report: None,
        }
    }

    /// 기본 설정으로 생성 (확인 모드)
    pub fn with_defaults() -> Self {
        Self::new(AutoIndexCreatorConfig::default())
    }

    /// 자동 모드로 생성
    pub fn automatic() -> Self {
        Self::new(AutoIndexCreatorConfig::automatic())
    }

    /// 시뮬레이션 모드로 생성
    pub fn dry_run() -> Self {
        Self::new(AutoIndexCreatorConfig::dry_run())
    }

    /// 현재 모드 반환
    pub fn mode(&self) -> CreationMode {
        self.config.mode
    }

    /// 추천을 승인 대기 큐에 추가
    pub fn queue_for_approval(&mut self, recommendation: IndexRecommendation) {
        let pending = PendingRecommendation::new(recommendation);
        let key = pending.key();
        self.pending.insert(key, pending);
    }

    /// 여러 추천을 승인 대기 큐에 추가
    pub fn queue_all(&mut self, recommendations: Vec<IndexRecommendation>) {
        for rec in recommendations {
            self.queue_for_approval(rec);
        }
    }

    /// 특정 인덱스 승인
    pub fn approve(&mut self, key: &str) -> bool {
        if let Some(pending) = self.pending.get_mut(key) {
            pending.approve();
            true
        } else {
            false
        }
    }

    /// 라벨의 모든 인덱스 승인
    pub fn approve_by_label(&mut self, label: &str) -> usize {
        let mut count = 0;
        for pending in self.pending.values_mut() {
            if pending.recommendation.label == label && !pending.approved {
                pending.approve();
                count += 1;
            }
        }
        count
    }

    /// 모든 대기 중인 인덱스 승인
    pub fn approve_all(&mut self) -> usize {
        let mut count = 0;
        for pending in self.pending.values_mut() {
            if !pending.approved {
                pending.approve();
                count += 1;
            }
        }
        count
    }

    /// 특정 인덱스 거부
    pub fn reject(&mut self, key: &str, reason: &str) -> bool {
        if let Some(pending) = self.pending.get_mut(key) {
            pending.reject(reason);
            true
        } else {
            false
        }
    }

    /// 라벨의 모든 인덱스 거부
    pub fn reject_by_label(&mut self, label: &str, reason: &str) -> usize {
        let mut count = 0;
        for pending in self.pending.values_mut() {
            if pending.recommendation.label == label && pending.decision_at.is_none() {
                pending.reject(reason);
                count += 1;
            }
        }
        count
    }

    /// 대기 중인 추천 목록
    pub fn pending_recommendations(&self) -> Vec<&PendingRecommendation> {
        self.pending
            .values()
            .filter(|p| p.decision_at.is_none())
            .collect()
    }

    /// 승인된 추천 목록
    pub fn approved_recommendations(&self) -> Vec<&PendingRecommendation> {
        self.pending
            .values()
            .filter(|p| p.approved && p.decision_at.is_some())
            .collect()
    }

    /// 거부된 추천 목록
    pub fn rejected_recommendations(&self) -> Vec<&PendingRecommendation> {
        self.pending
            .values()
            .filter(|p| !p.approved && p.decision_at.is_some())
            .collect()
    }

    /// 인덱스 생성 가능 여부 확인
    fn can_create(&self, recommendation: &IndexRecommendation) -> Result<(), String> {
        // 우선순위 점수 확인
        if recommendation.priority_score < self.config.min_priority_score {
            return Err(format!(
                "Insufficient priority score ({:.1} < {:.1})",
                recommendation.priority_score, self.config.min_priority_score
            ));
        }

        // 라벨당 최대 인덱스 수 확인
        let current = self
            .indexes_per_label
            .get(&recommendation.label)
            .copied()
            .unwrap_or(0);
        if current >= self.config.max_per_label {
            return Err(format!(
                "Index count per label exceeded (current: {}, max: {})",
                current, self.config.max_per_label
            ));
        }

        // 이미 생성된 인덱스인지 확인
        let index_name = Self::generate_index_name(recommendation);
        if self.created_indexes.contains(&index_name) {
            return Err("Index already created".to_string());
        }

        Ok(())
    }

    /// 인덱스 이름 생성
    pub fn generate_index_name(recommendation: &IndexRecommendation) -> String {
        format!(
            "idx_{}_{}",
            recommendation.label.to_lowercase(),
            recommendation.properties.join("_").to_lowercase()
        )
    }

    /// 승인된 인덱스 적용
    pub fn apply_approved<C: IndexCreator>(&mut self, creator: &mut C) -> CreationReport {
        let mut report = CreationReport::new(self.config.mode);

        // 승인된 추천 수집
        let approved: Vec<_> = self
            .pending
            .values()
            .filter(|p| p.approved)
            .cloned()
            .collect();

        report.total_recommendations = approved.len();

        for pending in approved {
            let recommendation = &pending.recommendation;
            let index_name = Self::generate_index_name(recommendation);

            // 생성 가능 여부 확인
            if let Err(reason) = self.can_create(recommendation) {
                report.add_result(IndexCreationResult::skipped(
                    index_name,
                    recommendation.label.clone(),
                    recommendation.properties.clone(),
                    recommendation.index_type.clone(),
                    reason,
                ));
                continue;
            }

            // 시뮬레이션 모드
            if self.config.mode == CreationMode::DryRun {
                report.add_result(IndexCreationResult::success(
                    index_name.clone(),
                    recommendation.label.clone(),
                    recommendation.properties.clone(),
                    recommendation.index_type.clone(),
                    0,
                ));
                self.created_indexes.push(index_name);
                *self
                    .indexes_per_label
                    .entry(recommendation.label.clone())
                    .or_insert(0) += 1;
                continue;
            }

            // 실제 인덱스 생성
            let start = SystemTime::now();
            let result = self.create_index_with_retry(creator, &index_name, recommendation);
            let duration_ms = start.elapsed().map(|d| d.as_millis() as u64).unwrap_or(0);

            match result {
                Ok(()) => {
                    report.add_result(IndexCreationResult::success(
                        index_name.clone(),
                        recommendation.label.clone(),
                        recommendation.properties.clone(),
                        recommendation.index_type.clone(),
                        duration_ms,
                    ));
                    self.created_indexes.push(index_name);
                    *self
                        .indexes_per_label
                        .entry(recommendation.label.clone())
                        .or_insert(0) += 1;
                }
                Err(e) => {
                    report.add_result(IndexCreationResult::failure(
                        index_name,
                        recommendation.label.clone(),
                        recommendation.properties.clone(),
                        recommendation.index_type.clone(),
                        e,
                    ));
                }
            }
        }

        // 적용된 추천 제거
        self.pending.retain(|_, p| !p.approved);

        self.last_report = Some(report.clone());
        report
    }

    /// 추천 직접 적용 (자동 모드)
    pub fn apply_recommendations<C: IndexCreator>(
        &mut self,
        creator: &mut C,
        recommendations: Vec<IndexRecommendation>,
    ) -> CreationReport {
        let mut report = CreationReport::new(self.config.mode);
        report.total_recommendations = recommendations.len();

        for recommendation in recommendations {
            let index_name = Self::generate_index_name(&recommendation);

            // 생성 가능 여부 확인
            if let Err(reason) = self.can_create(&recommendation) {
                report.add_result(IndexCreationResult::skipped(
                    index_name,
                    recommendation.label.clone(),
                    recommendation.properties.clone(),
                    recommendation.index_type.clone(),
                    reason,
                ));
                continue;
            }

            // 시뮬레이션 모드
            if self.config.mode == CreationMode::DryRun {
                report.add_result(IndexCreationResult::success(
                    index_name.clone(),
                    recommendation.label.clone(),
                    recommendation.properties.clone(),
                    recommendation.index_type.clone(),
                    0,
                ));
                self.created_indexes.push(index_name);
                *self
                    .indexes_per_label
                    .entry(recommendation.label.clone())
                    .or_insert(0) += 1;
                continue;
            }

            // 확인 모드에서는 큐에 추가
            if self.config.mode == CreationMode::RequireApproval {
                self.queue_for_approval(recommendation.clone());
                report.add_result(IndexCreationResult::skipped(
                    index_name,
                    recommendation.label,
                    recommendation.properties,
                    recommendation.index_type,
                    "Pending approval".to_string(),
                ));
                continue;
            }

            // 자동 모드: 실제 인덱스 생성
            let start = SystemTime::now();
            let result = self.create_index_with_retry(creator, &index_name, &recommendation);
            let duration_ms = start.elapsed().map(|d| d.as_millis() as u64).unwrap_or(0);

            match result {
                Ok(()) => {
                    report.add_result(IndexCreationResult::success(
                        index_name.clone(),
                        recommendation.label.clone(),
                        recommendation.properties.clone(),
                        recommendation.index_type.clone(),
                        duration_ms,
                    ));
                    self.created_indexes.push(index_name);
                    *self
                        .indexes_per_label
                        .entry(recommendation.label.clone())
                        .or_insert(0) += 1;
                }
                Err(e) => {
                    report.add_result(IndexCreationResult::failure(
                        index_name,
                        recommendation.label.clone(),
                        recommendation.properties.clone(),
                        recommendation.index_type.clone(),
                        e,
                    ));
                }
            }
        }

        self.last_report = Some(report.clone());
        report
    }

    /// 재시도 포함 인덱스 생성
    fn create_index_with_retry<C: IndexCreator>(
        &self,
        creator: &mut C,
        index_name: &str,
        recommendation: &IndexRecommendation,
    ) -> Result<(), String> {
        let mut last_error = String::new();

        for attempt in 0..=self.config.retry_count {
            if attempt > 0 {
                // 재시도 전 짧은 대기
                std::thread::sleep(std::time::Duration::from_millis(100 * attempt as u64));
            }

            match creator.create_index(
                index_name,
                &recommendation.label,
                &recommendation.properties,
            ) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_error = e;
                }
            }
        }

        Err(last_error)
    }

    /// 생성된 인덱스 목록
    pub fn created_indexes(&self) -> &[String] {
        &self.created_indexes
    }

    /// 라벨별 인덱스 수
    pub fn indexes_count_by_label(&self, label: &str) -> usize {
        self.indexes_per_label.get(label).copied().unwrap_or(0)
    }

    /// 마지막 보고서
    pub fn last_report(&self) -> Option<&CreationReport> {
        self.last_report.as_ref()
    }

    /// 만료된 승인 대기 정리
    pub fn cleanup_expired(&mut self) -> usize {
        let now = current_timestamp();
        let timeout = self.config.approval_timeout_secs;

        let expired: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, p)| p.decision_at.is_none() && now - p.queued_at > timeout)
            .map(|(k, _)| k.clone())
            .collect();

        let count = expired.len();
        for key in expired {
            self.pending.remove(&key);
        }

        count
    }

    /// 상태 초기화
    pub fn reset(&mut self) {
        self.pending.clear();
        self.created_indexes.clear();
        self.indexes_per_label.clear();
        self.last_report = None;
    }

    /// 설정 변경
    pub fn set_mode(&mut self, mode: CreationMode) {
        self.config.mode = mode;
    }

    /// 최소 우선순위 설정
    pub fn set_min_priority(&mut self, score: f64) {
        self.config.min_priority_score = score;
    }

    /// 라벨당 최대 인덱스 수 설정
    pub fn set_max_per_label(&mut self, max: usize) {
        self.config.max_per_label = max;
    }
}

impl Default for AutoIndexCreator {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// 현재 타임스탬프 반환
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
