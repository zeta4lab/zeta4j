// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # Zeta4j TCP Server Module
//!
//! Phase 2 Step 1: 고성능 비동기 TCP 서버
//! Phase 2 Step 2: HTTP REST API 서버
//! Phase 2 Step 4: 인증/권한 시스템
//! Phase 2 Step 5: 세션 관리
//! Phase 2 Step 6: 멀티 테넌시
//! Phase 2 Step 7: 서버 모니터링
//!
//! ## Features
//! - Tokio 기반 비동기 서버
//! - 10,000+ 동시 연결 지원
//! - 백프레셔 제어
//! - Graceful Shutdown
//! - Prometheus 메트릭
//! - TLS 1.2/1.3 지원 (M1.2)
//! - 연결 관리 (M1.3)
//! - 프레임 프로토콜 (M1.4)
//! - HTTP REST API (M2.1)
//! - 인증/권한 시스템 (M4.1)
//! - 세션 관리 (M5.1~M5.4)
//! - 멀티 테넌시 (M6.1~M6.4)
//! - 서버 모니터링 (M7.1~M7.4)

pub mod access;
pub mod auth;
pub mod blocking;
pub mod bolt;
pub mod common;
pub mod core;
pub mod frame;
pub mod fulltext;
pub mod grpc;
pub mod http;
pub mod monitor;
pub mod query;
pub mod session;
pub mod system;
pub mod tenant;
pub mod vector;

// Public exports (from common module)
pub use common::{ConfigError, ConnectionError, ServerError, TrackerError};
pub use common::{ServerConfig, ServerConfigBuilder, TcpKeepalive};

// Server Metrics (from monitor module)
pub use monitor::{Counter, Gauge, Histogram, ServerMetrics, ServerMetricsSnapshot};

// Core Network Layer exports
pub use core::{BackpressureController, BackpressureUsage, RateLimiter};
pub use core::{Connection, ConnectionId, ConnectionInfo, ConnectionState, ConnectionTracker};
pub use core::{ServerState, TcpServer};
pub use core::{ShutdownController, ShutdownGuard, ShutdownReason, ShutdownState};

// Connection Management exports (M1.3) - from core module
pub use core::{ConnState, ConnectionStateMachine, StateGuard, TransitionResult};
pub use core::{
    ConnectionPool, PoolConfig, PoolConfigBuilder, PoolError, PoolStats, PoolStatsSnapshot,
    PoolStatus, Poolable, PooledConnection,
};
pub use core::{
    KeepAliveConfig, KeepAliveConfigBuilder, KeepAliveEvent, KeepAliveManager, KeepAliveState,
};
pub use core::{
    TimeoutConfig, TimeoutConfigBuilder, TimeoutEvent, TimeoutGuard, TimeoutManager, TimeoutType,
};

// Frame Protocol exports (M1.4)
pub use frame::{
    ChunkConfig, ChunkMetadata, ChunkProgress, ChunkedReader, ChunkedWriter, CompressionLevel,
    CompressionStats, Compressor, DEFAULT_MAX_FRAME_SIZE, Decompressor, Frame, FrameCodec,
    FrameConfig, FrameConfigBuilder, FrameDecoder, FrameEncoder, FrameError, FrameFlags,
    FrameHeader, FrameType, HEADER_SIZE, MAGIC,
};

// HTTP Server exports (M2.1)
pub use http::{
    ApiError, ApiResponse, ComponentHealth, HealthResponse, HealthStatus, HttpConfig,
    HttpConfigBuilder, HttpError, ReadyResponse, RequestIdLayer, RequestLoggingLayer, ResponseMeta,
    ServerInfo, UnifiedRouterBuilder, create_unified_router, create_unified_router_with_config,
};

// Bolt Protocol exports (M3.1, M3.2, M3.3, M3.4)
pub use bolt::{
    // Message types (M3.3)
    AccessMode,
    AuthToken,
    BeginMessage,
    // Connection & Server (M3.4)
    BoltCodec,
    BoltConfig,
    BoltConfigBuilder,
    BoltConnection,
    BoltError,
    BoltErrorCode,
    // State machine (M3.3)
    BoltEvent,
    BoltRequest,
    BoltRequestCodec,
    BoltResponse,
    BoltResult,
    BoltServer,
    BoltServerBuilder,
    BoltServerState,
    BoltServerStats,
    BoltServerStatsSnapshot,
    BoltSession,
    BoltState,
    BoltStateMachine,
    BoltVersion,
    DiscardMessage,
    FailureMessage,
    Handshake,
    HandshakeError,
    HelloMessage,
    LogonMessage,
    Notification,
    NotificationSeverity,
    PackStreamDecoder,
    PackStreamEncoder,
    PackStreamError,
    PackStreamNode,
    PackStreamRelationship,
    PackStreamStructure,
    PackStreamValue,
    PullMessage,
    QueryPlan,
    QueryStats,
    RecordMessage,
    RouteMessage,
    RoutingTable,
    RunMessage,
    SessionId,
    SuccessMessage,
};

// Authentication exports
pub use auth::{
    // Middleware
    AuthConfig,
    // Error types
    AuthError,
    AuthLayer,
    AuthResult,
    AuthService,
    AuthenticatedUser,
    // Basic Auth
    BasicCredentials,
    extract_basic_auth,
    // Password hashing
    hash_password,
    is_basic_auth,
    verify_password,
};

// Session Management exports (M5.1, M5.2, M5.3, M5.4)
pub use session::{
    AutoScaler,
    AutoScalerConfig,
    AutoScalerConfigBuilder,
    // Bookmark (M5.2)
    Bookmark,
    BookmarkError,
    BookmarkManager,
    BookmarkWaitResult,
    CleanerTask,
    // Client Tracking (M5.3)
    ClientId,
    ClientInfo,
    ConnectionStats,
    ConnectionStatsSnapshot,
    DriverInfo,
    RequestTracer,
    // Server Connection Pool (M5.1)
    ServerConnectionPool,
    ServerPoolConfig,
    ServerPoolConfigBuilder,
    ServerPoolStats,
    ServerPoolStatus,
    // Server Session (M5.2)
    ServerSession,
    ServerSessionId,
    ServerSessionStore,
    SessionSettings,
    SessionSettingsBuilder,
    SessionState,
    SessionStoreStats,
    SessionTimeoutConfig,
    SessionTimeoutConfigBuilder,
    // Timeout Management (M5.4)
    SessionTimeoutManager,
    SessionTimers,
    SpanId,
    TimeoutEvent as SessionTimeoutEvent,
    TimeoutEventHandler,
    TraceContext,
    // Request Tracing (M5.3)
    TraceId,
    TraceLog,
    TraceSeverity,
};

// Multi-Tenancy exports (M6.1, M6.2, M6.3, M6.4)
pub use tenant::{
    DatabaseId,
    // Database Lifecycle (M6.4)
    DatabaseLifecycle,
    DatabaseMetadata,
    DatabaseState,
    DatabaseStatistics,
    IsolationLevel,
    LifecycleError,
    MigrationError,
    Namespace,
    NamespaceManager,
    ResourceAction,
    ResourceEnforcer,
    // Resource Isolation (M6.2)
    ResourceLimits,
    ResourceLimitsBuilder,
    ResourceMonitor,
    ResourceType,
    ResourceUsage,
    SchemaMigration,
    SchemaVersion,
    // Namespace (M6.3)
    Tenant,
    TenantConfig,
    TenantConfigBuilder,
    // Multi-Database (M6.1)
    TenantDatabase,
    TenantDatabaseConfig,
    TenantDatabaseConfigBuilder,
    TenantDatabaseManager,
    TenantDbError,
    TenantError,
    TenantId,
    TenantStatus,
};

// Server Monitoring exports (M7.1, M7.2, M7.3, M7.4)
pub use monitor::{
    Alert,
    AlertChannel,
    AlertCondition,
    AlertContext,
    AlertError,
    AlertEvaluation,
    AlertManager,
    AlertRule,
    // Alerts (M7.4)
    AlertSeverity,
    AlertThrottle,
    ComponentStatus,
    DatabaseHealthCheck,
    DatabaseMetrics,
    DiskSpaceHealthCheck,
    HealthCheckResult,
    // Health Check (M7.2)
    HealthChecker,
    LivenessProbe,
    LogChannel,
    LogEntry,
    // Note: ComponentHealth trait available via monitor::ComponentHealth
    // Note: HealthStatus available via monitor::HealthStatus (different from http::HealthStatus)
    // Logging (M7.3)
    LogLevel,
    LogOutput,
    LogSource,
    LogSpan,
    MemoryHealthCheck,
    MemoryOutput,
    // Metrics Registry (M7.1)
    MetricsRegistry,
    MetricsSnapshot,
    QueryMetrics,
    QueryMetricsSnapshot,
    ReadinessProbe,
    SlowQueryEntry,
    SlowQueryLog,
    StdoutOutput,
    StructuredLogger,
    SystemMetrics,
    SystemMetricsSnapshot,
    SystemStats,
    WebhookChannel,
    default_alert_rules,
};

// Profiler exports (Performance Optimization) - from monitor module
pub use monitor::{
    Bottleneck, BottleneckSeverity, BottleneckType, ConnectionProfile, FunctionProfile, IoProfile,
    LoadTestConfig, LoadTestResult, MemoryProfile, ProfileGuard, ProfilerConfig, ProfilerSnapshot,
    QueryProfile, ServerProfiler,
};

// Blocking Pool exports (v1.80.0 - rayon 기반 분리 풀)
pub use blocking::{
    BlockingError, BlockingMetrics, BlockingPoolConfig, BlockingPoolConfigBuilder,
    BlockingPoolError, BlockingPoolManager, BlockingPoolStats, PoolType, SinglePoolConfig,
};

// Unified Query Interface exports (102 Phase 2, Phase 3)
pub use query::{
    ConnectionInfo as QueryConnectionInfo, DefaultQueryExecutor, QueryError, QueryStatistics,
    RecordStream, SharedQueryExecutor, StreamRecord, StreamingQueryResult, UnifiedQueryExecutor,
    UnifiedQueryResult,
    convert::{json_to_value, packstream_to_value, value_to_json, value_to_packstream},
    create_query_executor,
};

// System Database exports (112)
pub use system::{
    CatalogDatabaseState, CatalogManager, CatalogStorageType, ConstraintDef, ConstraintType,
    DatabaseEntry, DatabaseLimits, DatabaseOptions, DatabaseSchema, EntityType, IndexConfig,
    IndexDef, IndexState, IndexType, SchemaManager, SecurityManager, SharedSystemDatabaseService,
    SharedSystemStorage, Similarity, SystemDatabaseService, SystemError, SystemResult,
    SystemStorage, SystemSummary, SystemWriteBatch, User, ValueType, cf as system_cf,
};

// Fulltext exports (#155)
pub use fulltext::{DatabaseFulltextSearchProvider, SharedFulltextProvider};
pub use vector::{DatabaseVectorSearchProvider, SharedVectorProvider};

// gRPC Service exports
pub use grpc::{AdminAppState, AdminGrpcService};
