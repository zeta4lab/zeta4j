// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 진단 및 문제해결 도구 모듈
//!
//! 그래프 DB 운영 시 발생하는 문제를 탐지하고 해결하는 도구를 제공합니다.
//!
//! # 구성요소
//!
//! - `profiler`: 쿼리 프로파일러 - 쿼리 실행 시간/비용 분석
//! - `monitor`: 시스템 상태 모니터 - 메모리, 캐시, 트랜잭션 상태
//! - `detector`: 문제 탐지기 - 잠재적 문제 탐지
//! - `manager`: 진단 관리자 - 통합 진단 도구
//! - `alert`: 알림 시스템 - 웹훅/Slack/이메일 알림 (v1.60.0)
//! - `visualization`: 시각화 도구 - ASCII 차트, 대시보드 (v1.74.0)

pub mod alert;
pub mod detector;
pub mod history;
pub mod manager;
pub mod monitor;
pub mod profiler;
pub mod visualization;

pub use profiler::{
    ExtendedQueryStats, ProfilingMode, QueryProfile, QueryProfiler, QueryStats, SamplingConfig,
};

pub use monitor::{
    BufferPoolStatus, IndexStatus, LockStatus, SystemMonitor, SystemStatus, TransactionStatus,
    TrendDirection,
};

pub use detector::{
    DetectedProblem, DetectorConfig, ProblemDetector, ProblemSeverity, ProblemType,
};

pub use manager::{DiagnosticManager, DiagnosticReport};

pub use history::{
    AnalyzerConfig, AnomalyResult, ForecastResult, HistoryAnalyzer, HistoryReport, MetricPoint,
    MetricType, TimeSeriesConfig, TimeSeriesStore, TrendAnalysis,
};

pub use alert::{
    Alert, AlertChannel, AlertManager, AlertResult, AlertRule, AlertStats, ChannelType,
    RuleCondition,
};

pub use visualization::{
    AsciiChart, Dashboard, MetricWidget, ProgressBar, Sparkline, StatusIndicator, Table,
};
