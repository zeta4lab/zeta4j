// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 자동 튜닝 모듈
//!
//! 그래프 DB 성능을 자동으로 최적화하는 시스템을 제공합니다.
//!
//! # 구성요소
//!
//! - `auto_stats`: 통계 자동 갱신
//! - `index_advisor`: 인덱스 추천
//! - `memory_tuner`: 메모리 자동 조정
//! - `auto_tuner`: 통합 자동 튜닝 관리자
//! - `query_plan_analyzer`: 쿼리 플랜 분석기
//! - `auto_index_creator`: 자동 인덱스 생성
//! - `history_analyzer`: 히스토리 기반 분석
//! - `forecasting`: 통계 기반 워크로드 예측

pub mod auto_index_creator;
pub mod auto_stats;
pub mod auto_tuner;
pub mod forecasting;
pub mod history_analyzer;
pub mod index_advisor;
pub mod memory_tuner;
pub mod query_plan_analyzer;

pub use auto_stats::{AutoStatsConfig, AutoStatsRefresher, ChangeType, RefreshResult};

pub use index_advisor::{IndexAdvisor, IndexRecommendation, QueryPattern, RecommendedIndexType};

pub use memory_tuner::{
    MemoryAutoTuner, MemoryStats, MemoryTuningConfig, TuningAction, TuningActionType,
};

pub use auto_tuner::{AutoTuner, AutoTunerConfig, HealthStatus, TuningReport};

pub use query_plan_analyzer::{
    AnalyzerConfig, IssueSeverity, IssueType, OperationCost, OptimizationSuggestion, PlanAnalysis,
    PlanIssue, QueryPlanAnalyzer, SuggestionType,
};

pub use auto_index_creator::{
    AutoIndexCreator, AutoIndexCreatorConfig, CreationMode, CreationReport, IndexCreationResult,
    IndexCreator, PendingRecommendation,
};

pub use history_analyzer::{
    HistoryAnalyzerConfig, PeriodType, PeriodicPattern, Prediction, ProactiveRecommendation,
    RecommendationType, TrendAnalysis, TrendDirection, TuningHistoryAnalyzer, TuningMetricPoint,
    TuningMetricType,
};
