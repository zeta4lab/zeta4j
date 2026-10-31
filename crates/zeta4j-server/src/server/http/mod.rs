// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # HTTP Server Module (M2.1 + M2.2 + M2.3 + M2.4)
//!
//! Phase 2 Step 2: HTTP REST API 서버
//!
//! ## Features
//! - axum 기반 HTTP/1.1 서버
//! - Tower 미들웨어 스택
//! - CORS 지원
//! - 요청/응답 JSON 처리
//! - Graceful Shutdown
//! - Cypher 쿼리 엔드포인트 (M2.2)
//! - HTTP 트랜잭션 관리
//! - 관리 엔드포인트 (M2.3)
//! - 스트리밍 응답 (M2.4)

mod config;
mod error;
mod middleware;
mod types;
mod unified_router;

// Cypher 엔드포인트 (M2.2)
mod cypher_handlers;
mod cypher_routes;
mod cypher_types;
mod executor;
mod http_transaction;

// 관리 엔드포인트 (M2.3)
mod admin_handlers;
mod admin_routes;
mod admin_types;
mod db_manager;
mod metrics;

// 스트리밍 응답 (M2.4)
pub mod streaming;
mod streaming_handlers;
mod streaming_routes;

// 인증 라우트 (M4.1, M4.2)
mod auth_routes;
mod public_surface;

// Public exports - Base HTTP (M2.1)
pub use config::{AuthHttpConfig, CorsConfig, HttpConfig, HttpConfigBuilder, RateLimitHttpConfig};
pub use error::HttpError;
pub use middleware::{
    RateLimitInfo, RateLimitLayer, RateLimitService, RateLimiterState, RequestId, RequestIdLayer,
    RequestLoggingLayer, TokenBucket,
};
pub use types::{
    ApiError, ApiResponse, BOLT_VERSION, ComponentHealth, HealthResponse, HealthStatus,
    ReadyResponse, ResponseMeta, ServerInfo,
};
pub use unified_router::{
    HttpRouterProtection, UnifiedRouterBuilder, create_unified_router,
    create_unified_router_with_config,
};

// Public exports - Cypher (M2.2)
pub use cypher_handlers::{
    CypherAppState, auto_commit, begin_transaction, commit_transaction, execute_in_transaction,
    query_v2, rollback_transaction,
};
pub use cypher_routes::cypher_routes;
pub use cypher_types::{
    CypherError, CypherRequest, CypherResponse, GraphData, NodeData, QueryStats, RelationshipData,
    ResultDataContent, ResultRow, Statement, StatementResult, TransactionInfo,
};
pub use executor::HttpQueryExecutor;
pub use http_transaction::{
    HttpTransaction, HttpTransactionConfig, HttpTransactionConfigBuilder, HttpTransactionManager,
    TransactionOwner, TransactionState,
};

// Public exports - Admin (M2.3)
pub use admin_handlers::AdminAppState;
pub use admin_routes::{admin_routes, metrics_routes};
pub use admin_types::{
    CreateDatabaseRequest, CreateUserRequest, DatabaseInfo, DatabaseOptions, DatabaseStatus,
    JmxMetric, JmxMetrics, ServerConfigResponse, ServerInfoResponse, ServerMode, ServerStatus,
    UserInfo,
};
pub use db_manager::{
    DatabaseConfig, DatabaseInstance, DatabaseManager, DatabaseManagerConfig, DatabaseStats,
    DbManagerError, SharedDatabaseManager,
};
pub use metrics::{HttpMetrics, SharedMetrics, create_metrics};

// Public exports - Streaming (M2.4)
pub use streaming::{
    BackpressureError, BufferState, CancellableStreamingResult, JsonLinesFormatter,
    JsonLinesRecord, JsonLinesStream, JsonLinesStreamBuilder, PollError, PollManager,
    PollManagerConfig, PollProgress, PollResponse, PollStatus, QueryState, RecordMeta,
    SharedBackpressure, SharedPollManager, SharedStreamingExecutor, SseEventBuilder, SseEventType,
    SseStream, SseStreamBuilder, StreamBackpressure, StreamError, StreamFooter, StreamFormat,
    StreamHeader, StreamPermit, StreamStats, StreamingConfig, StreamingExecutor,
    StreamingQueryRequest, StreamingResult, create_backpressure, create_poll_manager,
    create_streaming_executor,
};
pub use streaming_handlers::StreamingAppState;
pub use streaming_routes::streaming_routes;

// Public exports - Auth (M4.1, M4.2)
pub use auth_routes::{AuthAppState, LoginRequest, LoginResponse, WhoAmIResponse, auth_routes};
pub use public_surface::{PUBLIC_HTTP_SURFACES, PublicHttpSurface, is_public_http_surface};
