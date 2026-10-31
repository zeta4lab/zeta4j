// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Session Management Module
//!
//! Phase 2 Step 5: 확장 가능한 세션 및 연결 관리
//!
//! ## 기능
//! - M5.1: 서버 연결 풀링 (동적 조정, 유휴 연결 정리)
//! - M5.2: 세션 상태 관리 (메타데이터, 북마크, 트랜잭션 컨텍스트)
//! - M5.3: 클라이언트 추적 (ID, 에이전트, 요청 추적)
//! - M5.4: 타임아웃 관리 (연결, 트랜잭션, 쿼리 타임아웃)
//! - M5.5: 세션 클린업 (세션 만료 시 트랜잭션 자동 롤백)

pub mod bookmark;
pub mod cleanup;
pub mod client;
pub mod pool;
pub mod server_session;
pub mod timeout;
pub mod tracing;

// Re-exports
pub use pool::{
    AutoScaler, AutoScalerConfig, AutoScalerConfigBuilder, CleanerTask, ServerConnectionPool,
    ServerPoolConfig, ServerPoolConfigBuilder, ServerPoolStats, ServerPoolStatus,
};

pub use server_session::{
    ServerSession, ServerSessionId, SessionSettings, SessionSettingsBuilder, SessionState,
    SessionStore as ServerSessionStore, SessionStoreStats,
};

pub use bookmark::{Bookmark, BookmarkError, BookmarkManager, BookmarkWaitResult};

pub use client::{ClientId, ClientInfo, ConnectionStats, ConnectionStatsSnapshot, DriverInfo};

pub use tracing::{RequestTracer, SpanId, TraceContext, TraceId, TraceLog, TraceSeverity};

pub use timeout::{
    SessionTimeoutConfig, SessionTimeoutConfigBuilder, SessionTimeoutManager, SessionTimers,
    TimeoutEvent, TimeoutEventHandler,
};

pub use cleanup::{SessionCleanupConfig, SessionCleanupConfigBuilder, SessionCleanupTask};
