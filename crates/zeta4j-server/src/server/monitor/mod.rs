//! Server Monitoring Module (M7.1 ~ M7.4)
//!
//! 서버 모니터링 및 관측가능성(Observability) 시스템
//!
//! ## Features
//! - 기본 메트릭 (server_metrics): Counter, Gauge, Histogram, ServerMetrics
//! - M7.1: 확장 메트릭 수집 (metrics): MetricsRegistry
//! - M7.2: 헬스 체크 (health)
//! - M7.3: 로깅 시스템 (logging)
//! - M7.4: 알림 시스템 (alert)
//! - 프로파일러 (profiler): 성능 프로파일링 및 병목 분석

mod alert;
mod health;
mod logging;
mod metrics;
pub mod profiler;
pub mod server_metrics;

pub use metrics::{
    DatabaseMetrics, MetricsRegistry, MetricsSnapshot, QueryMetrics, QueryMetricsSnapshot,
    SystemMetrics, SystemMetricsSnapshot, SystemStats,
};

pub use health::{
    ComponentHealth, ComponentStatus, DatabaseHealthCheck, DiskSpaceHealthCheck, HealthCheckResult,
    HealthChecker, HealthStatus, LivenessProbe, MemoryHealthCheck, ReadinessProbe,
};

pub use logging::{
    LogEntry, LogLevel, LogOutput, LogSource, LogSpan, MemoryOutput, SlowQueryEntry, SlowQueryLog,
    StdoutOutput, StructuredLogger,
};

pub use alert::{
    Alert, AlertChannel, AlertCondition, AlertContext, AlertError, AlertEvaluation, AlertManager,
    AlertRule, AlertSeverity, AlertThrottle, LogChannel, WebhookChannel, default_alert_rules,
};

// Server Metrics (기본 메트릭 타입)
pub use server_metrics::{Counter, Gauge, Histogram, ServerMetrics, ServerMetricsSnapshot};

// Profiler
pub use profiler::{
    Bottleneck, BottleneckSeverity, BottleneckType, ConnectionProfile, FunctionProfile, IoProfile,
    LoadTestConfig, LoadTestResult, MemoryProfile, ProfileGuard, ProfilerConfig, ProfilerSnapshot,
    QueryProfile, ServerProfiler,
};
