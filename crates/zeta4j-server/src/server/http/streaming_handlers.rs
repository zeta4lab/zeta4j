// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Streaming Handlers (M2.4)
//!
//! 스트리밍 엔드포인트 핸들러

use axum::body::Body;
use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::sse::Sse;
use std::sync::Arc;
use std::time::Instant;

use super::error::HttpError;
use super::streaming::{
    JsonLinesStream, PollProgress, PollResponse, PollStatus, SharedStreamingExecutor, SseStream,
    StreamingConfig, StreamingQueryRequest,
};

/// 스트리밍 API 상태
#[derive(Clone)]
pub struct StreamingAppState {
    pub executor: SharedStreamingExecutor,
    pub config: StreamingConfig,
    pub start_time: Instant,
}

impl StreamingAppState {
    pub fn new(executor: SharedStreamingExecutor, config: StreamingConfig) -> Self {
        Self {
            executor,
            config,
            start_time: Instant::now(),
        }
    }
}

/// POST /db/{db_name}/query/stream - JSON Lines 스트리밍
pub async fn query_stream(
    Path(_db_name): Path<String>,
    State(state): State<Arc<StreamingAppState>>,
    Json(request): Json<StreamingQueryRequest>,
) -> Result<impl IntoResponse, HttpError> {
    let _batch_size = request
        .batch_size
        .unwrap_or(state.config.default_batch_size);

    // 스트리밍 쿼리 실행
    let result = state
        .executor
        .execute_streaming(&request.statement, &request.parameters)
        .await
        .map_err(|e| HttpError::InternalError(e.to_string()))?;

    // JSON Lines 스트림 생성
    let stream = JsonLinesStream::new(result.columns, result.query_id, result.receiver);

    Ok((
        StatusCode::OK,
        [
            ("content-type", "application/x-ndjson"),
            ("transfer-encoding", "chunked"),
            ("x-stream-format", "jsonlines"),
        ],
        Body::from_stream(stream),
    ))
}

/// POST /db/{db_name}/query/sse - Server-Sent Events
pub async fn query_sse(
    Path(_db_name): Path<String>,
    State(state): State<Arc<StreamingAppState>>,
    Json(request): Json<StreamingQueryRequest>,
) -> Result<Sse<SseStream>, HttpError> {
    // 스트리밍 쿼리 실행
    let result = state
        .executor
        .execute_streaming(&request.statement, &request.parameters)
        .await
        .map_err(|e| HttpError::InternalError(e.to_string()))?;

    // SSE 스트림 생성
    let stream = SseStream::new(result.columns, result.query_id, result.receiver)
        .with_memory_reservation(result.memory_reservation);

    Ok(Sse::new(stream))
}

/// POST /db/{db_name}/query/poll - Long Polling 시작
pub async fn query_poll_start(
    Path(db_name): Path<String>,
    State(state): State<Arc<StreamingAppState>>,
    Json(request): Json<StreamingQueryRequest>,
) -> Result<Json<PollResponse>, HttpError> {
    let batch_size = request
        .batch_size
        .unwrap_or(state.config.default_batch_size);

    // Poll 쿼리 시작
    let query_id = state
        .executor
        .start_poll_query(&request.statement, &request.parameters)
        .await
        .map_err(|e| HttpError::InternalError(e.to_string()))?;

    // 잠시 대기 후 첫 배치 가져오기
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // 첫 배치 가져오기
    let poll_manager = state.executor.poll_manager();
    match poll_manager.fetch_batch(&query_id, Some(batch_size)).await {
        Ok((results, progress, has_more)) => {
            let columns = poll_manager
                .get_query(&query_id)
                .await
                .map(|q| q.columns)
                .unwrap_or_default();

            let status = if has_more {
                PollStatus::Running
            } else {
                PollStatus::Completed
            };

            let next_url = if has_more {
                Some(format!("/db/{}/query/poll/{}", db_name, query_id))
            } else {
                None
            };

            Ok(Json(PollResponse {
                query_id,
                status,
                columns,
                results: Some(results),
                next_url,
                progress,
                error: None,
            }))
        }
        Err(_) => {
            // 아직 결과가 없음 - Running 상태 반환
            Ok(Json(PollResponse {
                query_id: query_id.clone(),
                status: PollStatus::Running,
                columns: vec![],
                results: Some(vec![]),
                next_url: Some(format!("/db/{}/query/poll/{}", db_name, query_id)),
                progress: PollProgress::default(),
                error: None,
            }))
        }
    }
}

/// GET /db/{db_name}/query/poll/{query_id} - Poll 계속
pub async fn query_poll_continue(
    Path((db_name, query_id)): Path<(String, String)>,
    State(state): State<Arc<StreamingAppState>>,
) -> Result<Json<PollResponse>, HttpError> {
    let batch_size = state.config.default_batch_size;

    let poll_manager = state.executor.poll_manager();

    match poll_manager.fetch_batch(&query_id, Some(batch_size)).await {
        Ok((results, progress, has_more)) => {
            // 쿼리 상태 확인
            let query_state = poll_manager
                .get_query(&query_id)
                .await
                .map_err(|e| HttpError::NotFound(e.to_string()))?;

            let status = match query_state.status {
                PollStatus::Running if has_more => PollStatus::Running,
                PollStatus::Running => PollStatus::Completed,
                other => other,
            };

            let next_url = if status == PollStatus::Running {
                Some(format!("/db/{}/query/poll/{}", db_name, query_id))
            } else {
                None
            };

            Ok(Json(PollResponse {
                query_id,
                status,
                columns: query_state.columns,
                results: Some(results),
                next_url,
                progress,
                error: query_state.error,
            }))
        }
        Err(e) => Err(HttpError::InternalError(e.to_string())),
    }
}

/// DELETE /db/{db_name}/query/poll/{query_id} - Poll 취소
pub async fn query_poll_cancel(
    Path((_db_name, query_id)): Path<(String, String)>,
    State(state): State<Arc<StreamingAppState>>,
) -> Result<StatusCode, HttpError> {
    let poll_manager = state.executor.poll_manager();

    poll_manager
        .cancel_query(&query_id)
        .await
        .map_err(|e| HttpError::NotFound(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// GET /streaming/status - 스트리밍 상태
pub async fn streaming_status(
    State(state): State<Arc<StreamingAppState>>,
) -> Json<StreamingStatus> {
    let backpressure = state.executor.backpressure();
    let poll_manager = state.executor.poll_manager();

    Json(StreamingStatus {
        active_streams: backpressure.active_streams(),
        available_slots: backpressure.available_slots(),
        max_concurrent: backpressure.max_concurrent(),
        pending_polls: poll_manager.query_count().await,
        uptime_ms: state.start_time.elapsed().as_millis() as u64,
    })
}

/// 스트리밍 상태 응답
#[derive(serde::Serialize)]
pub struct StreamingStatus {
    pub active_streams: usize,
    pub available_slots: usize,
    pub max_concurrent: usize,
    pub pending_polls: usize,
    pub uptime_ms: u64,
}
