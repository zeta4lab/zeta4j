// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # Zeta4j Server Crate
//!
//! Phase 2 모듈: Bolt/HTTP 서버, CLI, 드라이버, 보안, 모니터링
//!
//! ## Modules
//!
//! | 모듈 | 설명 | 권장 접근 경로 |
//! |------|------|---------------|
//! | `server` | Bolt/HTTP 프로토콜 서버 | `zeta4j_server::server::*` |
//! | `driver` | Rust 클라이언트 드라이버 | `zeta4j_server::driver::*` |
//! | `diagnostics` | 진단 및 문제해결 도구 | `zeta4j_server::diagnostics::*` |
//! | `tuning` | 자동 튜닝 시스템 | `zeta4j_server::tuning::*` |
//! | `io` | 데이터 Import/Export | `zeta4j_server::io::*` |
//!
//! ## 권장 사용 패턴
//!
//! ```rust,ignore
//! // ✅ 권장: 모듈 경로로 접근
//! use zeta4j_server::server::bolt::{BoltServer, BoltConfig};
//! use zeta4j_server::server::http::{UnifiedRouterBuilder, HttpConfig};
//! Server clients live in the edition-neutral `zeta4j-bolt-client` crate.
//! ```
//!
//! ## Server 모듈 구조
//!
//! ```text
//! server/
//! ├── common/     # 공통 설정/에러
//! ├── core/       # 네트워크 코어 (TCP, 연결 관리)
//! ├── monitor/    # Observability (메트릭, 로깅, 알림)
//! ├── tls/        # TLS 보안
//! ├── frame/      # 프레임 프로토콜
//! ├── http/       # HTTP REST API
//! ├── bolt/       # Bolt 프로토콜
//! ├── auth/       # 인증/권한
//! ├── session/    # 세션 관리
//! ├── tenant/     # 멀티 테넌시
//! ├── blocking/   # 블로킹 풀
//! ├── query/      # 쿼리 실행
//! └── system/     # 시스템 데이터베이스
//! ```

// =============================================================================
// Phase 2 Modules
// =============================================================================

pub mod diagnostics;
pub mod driver;
pub mod io;
pub mod prelude;
pub mod server;
pub mod tuning;

// =============================================================================
// Re-exports from zeta4j-graph (modules)
// =============================================================================

// Re-export entire modules for nested access (e.g., zeta4j_server::config::CliSettings)
pub use zeta4j_graph::config;
pub use zeta4j_graph::engine;

// =============================================================================
// Re-exports from zeta4j-graph (types)
// =============================================================================

pub use zeta4j_graph::{
    // Algorithm
    algorithm,

    // Config types
    config::{
        AuthServerConfig, BoltServerConfig, CliSettings, ConfigError, ConfigLoader, ConfigRegistry,
        Edition, HttpServerConfig, UnifiedServerConfig, VectorConfig, Zeta4jConfig,
    },

    // Core types
    core::{Edge, EdgeId, Node, NodeId, Property, Value},

    // Engine types
    engine::{EngineStats, GraphEngine, GraphEngineConfig, StorageType},

    // Error
    error::{GraphError, Result},

    // Index
    index,

    // Schema
    schema,
    // Storage
    storage,

    // Transaction
    transaction::{IsolationLevel, Transaction, TransactionId},
};

// =============================================================================
// Re-exports from zeta4j-cypher-executor
// =============================================================================

pub use zeta4j_cypher_executor::parse_query;

// =============================================================================
// Driver Module Exports
// =============================================================================

pub use driver::{
    AccessMode as DriverAccessMode, AuthToken as DriverAuthToken, BoltError as DriverBoltError,
    Bookmark as DriverBookmark, ConnectionPool as DriverConnectionPool,
    ConnectionState as DriverConnectionState, Counters, Driver, DriverConfig, DriverConfigBuilder,
    DriverError, DriverMetrics, DriverResult, Duration as DriverDuration, InputPosition,
    Node as DriverNode, Notification as DriverNotification, Path as DriverPath, Point,
    PoolConfig as DriverPoolConfig, PoolConfigBuilder as DriverPoolConfigBuilder, PoolMetrics,
    PooledConnection as DriverPooledConnection, Query, QueryResult as DriverQueryResult, QueryType,
    Record, RecordStream as DriverRecordStream, Relationship as DriverRelationship, ResultSummary,
    ServerAddress, ServerInfo as DriverServerInfo, Session, SessionConfig, SessionConfigBuilder,
    Transaction as DriverTransaction, TransactionConfig, TransactionState, TrustStrategy,
    Value as DriverValue,
};

// =============================================================================
// Diagnostics Module Exports
// =============================================================================

pub use diagnostics::{
    Alert as DiagAlert, AlertChannel as DiagAlertChannel, AlertManager as DiagAlertManager,
    AlertResult as DiagAlertResult, AlertRule as DiagAlertRule, AlertStats, AnalyzerConfig,
    AnomalyResult, AsciiChart, BufferPoolStatus, ChannelType, Dashboard, DetectedProblem,
    DetectorConfig, DiagnosticManager, DiagnosticReport, ExtendedQueryStats, ForecastResult,
    HistoryAnalyzer, HistoryReport, IndexStatus, LockStatus, MetricPoint, MetricType, MetricWidget,
    ProblemDetector, ProblemSeverity, ProblemType, ProfilingMode, ProgressBar,
    QueryProfile as DiagQueryProfile, QueryProfiler, QueryStats as DiagQueryStats, RuleCondition,
    SamplingConfig, Sparkline, StatusIndicator, SystemMonitor, SystemStatus, Table,
    TimeSeriesConfig, TimeSeriesStore, TransactionStatus, TrendAnalysis as DiagTrendAnalysis,
    TrendDirection as DiagTrendDirection,
};

// =============================================================================
// Tuning Module Exports
// =============================================================================

pub use tuning::{
    AnalyzerConfig as TuningAnalyzerConfig, AutoIndexCreator, AutoIndexCreatorConfig,
    AutoStatsConfig, AutoStatsRefresher, AutoTuner, AutoTunerConfig, ChangeType, CreationMode,
    CreationReport, HealthStatus as TuningHealthStatus, HistoryAnalyzerConfig, IndexAdvisor,
    IndexCreationResult, IndexCreator, IndexRecommendation, IssueSeverity, IssueType,
    MemoryAutoTuner, MemoryStats, MemoryTuningConfig, OperationCost, OptimizationSuggestion,
    PendingRecommendation, PeriodType, PeriodicPattern, PlanAnalysis, PlanIssue, Prediction,
    ProactiveRecommendation, QueryPattern, QueryPlanAnalyzer, RecommendationType,
    RecommendedIndexType, RefreshResult, SuggestionType, TrendAnalysis as TuningTrendAnalysis,
    TrendDirection as TuningTrendDirection, TuningAction, TuningActionType, TuningHistoryAnalyzer,
    TuningMetricPoint, TuningMetricType, TuningReport,
};

// =============================================================================
// IO Module Exports
// =============================================================================

pub use io::{
    BatchBuilder, BatchLoader, BatchOptions, BatchStats, CompressedReader, CompressedWriter,
    CompressionFormat, CompressionOptions, CompressionStats as IoCompressionStats, CsvExporter,
    CsvImporter, CsvOptions, EdgeJson, ExportStats, GexfAttribute, GexfExportStats, GexfExporter,
    GexfImportStats, GexfImporter, GexfType, GraphJson, GraphMLExporter, GraphMLImporter,
    GraphMLKey, GraphMLType, GraphMetadata, GzipLevel, ImportStats, JsonExporter, JsonImporter,
    KeyTarget, Lz4Level, NdjsonEdge, NdjsonExporter, NdjsonImporter, NdjsonNode, NodeJson,
    PendingEdge, PendingNode, ProgressCallback, RouterApiError, StateManager, StreamingCsvExporter,
    StreamingEdgeIterator, StreamingNodeIterator, StreamingOptions, ValueConverter, compress_data,
    compress_file, decompress_data, decompress_file, load_edges_from_csv_async, load_json,
    load_json_or_default, load_nodes_from_csv_async, save_json,
};
