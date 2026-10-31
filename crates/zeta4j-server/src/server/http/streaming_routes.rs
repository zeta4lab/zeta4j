// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Streaming Routes (M2.4)
//!
//! 스트리밍 엔드포인트 라우트 정의

use axum::Router;
use axum::routing::{delete, get, post};
use std::sync::Arc;

use super::streaming_handlers::{self, StreamingAppState};

/// 스트리밍 라우트 생성
pub fn streaming_routes(state: Arc<StreamingAppState>) -> Router {
    Router::new()
        // JSON Lines 스트리밍
        .route(
            "/db/{db_name}/query/stream",
            post(streaming_handlers::query_stream),
        )
        // Server-Sent Events
        .route(
            "/db/{db_name}/query/sse",
            post(streaming_handlers::query_sse),
        )
        // Long Polling
        .route(
            "/db/{db_name}/query/poll",
            post(streaming_handlers::query_poll_start),
        )
        .route(
            "/db/{db_name}/query/poll/{query_id}",
            get(streaming_handlers::query_poll_continue),
        )
        .route(
            "/db/{db_name}/query/poll/{query_id}",
            delete(streaming_handlers::query_poll_cancel),
        )
        // 스트리밍 상태
        .route(
            "/streaming/status",
            get(streaming_handlers::streaming_status),
        )
        .with_state(state)
}
