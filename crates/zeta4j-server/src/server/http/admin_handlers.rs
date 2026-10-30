//! Admin Handlers (M2.3)
//!
//! 관리 엔드포인트 핸들러

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::sync::Arc;
use std::time::Instant;

use zeta4j_graph::config::Edition;
use zeta4j_graph::engine::GraphEngine;

use super::admin_types::{
    CreateDatabaseRequest, CreateUserRequest, DatabaseInfo, JmxMetric, JmxMetrics,
    ServerConfigResponse, ServerInfoResponse, ServerMode, ServerStatus, UserInfo,
};
use super::db_manager::{DatabaseConfig, SharedDatabaseManager};
use super::error::{ApiError, HttpError};
use super::metrics::SharedMetrics;

use crate::server::system::{LocalSecurityService, SecurityManager};

/// Admin API 상태
#[derive(Clone)]
pub struct AdminAppState {
    pub db_manager: SharedDatabaseManager,
    pub metrics: SharedMetrics,
    pub start_time: Instant,
    pub address: String,
    pub server_id: String,
    pub engine: Arc<GraphEngine>,
    /// 서버 시간대 (utc, local)
    pub timezone: String,
    /// 서버 에디션
    pub edition: Edition,
    /// 보안 관리자
    pub security_manager: Arc<SecurityManager>,
    /// Canonical credential state used by readiness checks when auth is enabled.
    pub credential_security: Option<Arc<LocalSecurityService>>,
}

impl AdminAppState {
    pub fn new(
        db_manager: SharedDatabaseManager,
        metrics: SharedMetrics,
        address: String,
        engine: Arc<GraphEngine>,
        security_manager: Arc<SecurityManager>,
    ) -> Self {
        Self {
            db_manager,
            metrics,
            start_time: Instant::now(),
            address,
            server_id: uuid::Uuid::now_v7().to_string(),
            engine,
            timezone: "utc".to_string(),
            edition: Edition::Base,
            security_manager,
            credential_security: None,
        }
    }

    /// 시간대 설정
    pub fn with_timezone(mut self, timezone: impl Into<String>) -> Self {
        self.timezone = timezone.into();
        self
    }

    /// 에디션 설정
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.edition = edition;
        self
    }

    /// Adds the canonical credential state required for authenticated readiness.
    pub fn with_credential_security(
        mut self,
        credential_security: Option<Arc<LocalSecurityService>>,
    ) -> Self {
        self.credential_security = credential_security;
        self
    }
}

// ============================================================================
// Server Info Handlers
// ============================================================================

/// GET /admin/server/info - 서버 정보
pub async fn server_info(State(state): State<Arc<AdminAppState>>) -> Json<ServerInfoResponse> {
    let uptime = state.start_time.elapsed();

    Json(ServerInfoResponse {
        name: "zeta4j".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        edition: state.edition.name().to_string(),
        mode: ServerMode::Single,
        status: ServerStatus::Available,
        uptime_ms: uptime.as_millis() as u64,
    })
}

/// GET /admin/server/config - 서버 설정
pub async fn server_config(State(state): State<Arc<AdminAppState>>) -> Json<ServerConfigResponse> {
    let mut config = std::collections::HashMap::new();

    config.insert(
        "dbms.default_database".to_string(),
        serde_json::json!("zeta4j"),
    );
    config.insert(
        "dbms.security.auth_enabled".to_string(),
        serde_json::json!(true),
    );
    config.insert("server.bolt.enabled".to_string(), serde_json::json!(true));
    config.insert("server.http.enabled".to_string(), serde_json::json!(true));
    config.insert(
        "server.http.listen_address".to_string(),
        serde_json::json!(state.address.clone()),
    );

    Json(ServerConfigResponse { config })
}

/// GET /admin/server/jmx - JMX 호환 메트릭
pub async fn jmx_metrics(State(state): State<Arc<AdminAppState>>) -> Json<JmxMetrics> {
    // 기본 데이터베이스 통계 가져오기
    let db_info = state.db_manager.default_database().await;
    let (store_size, tx_count) = db_info
        .map(|info| (info.store_size, info.tx_count))
        .unwrap_or((0, 0));

    Json(JmxMetrics {
        store_size: JmxMetric::new("Store size in bytes", store_size),
        total_store_size: JmxMetric::new("Total store size in bytes", store_size),
        transaction_committed: JmxMetric::new("Committed transactions", tx_count),
        transaction_rolled_back: JmxMetric::new("Rolled back transactions", 0u64),
        transaction_active: JmxMetric::new("Active transactions", 0u64),
        bolt_connections_opened: JmxMetric::new("Bolt connections opened", 0u64),
        bolt_connections_closed: JmxMetric::new("Bolt connections closed", 0u64),
        bolt_connections_running: JmxMetric::new("Running Bolt connections", 0u64),
        bolt_messages_received: JmxMetric::new("Bolt messages received", 0u64),
        bolt_messages_started: JmxMetric::new("Bolt messages started", 0u64),
        bolt_messages_done: JmxMetric::new("Bolt messages completed", 0u64),
        bolt_messages_failed: JmxMetric::new("Bolt messages failed", 0u64),
    })
}

// ============================================================================
// Database Management Handlers
// ============================================================================

/// GET /admin/databases - 데이터베이스 목록
pub async fn list_databases(State(state): State<Arc<AdminAppState>>) -> Json<Vec<DatabaseInfo>> {
    let databases = state.db_manager.list().await;
    Json(databases)
}

/// GET /admin/databases/:name - 데이터베이스 조회
pub async fn get_database(
    Path(name): Path<String>,
    State(state): State<Arc<AdminAppState>>,
) -> Result<Json<DatabaseInfo>, ApiError> {
    let db = state.db_manager.get(&name).await?;
    Ok(Json(db))
}

/// POST /admin/databases/:name - 데이터베이스 생성
pub async fn create_database(
    Path(name): Path<String>,
    State(state): State<Arc<AdminAppState>>,
    Json(request): Json<Option<CreateDatabaseRequest>>,
) -> Result<(StatusCode, Json<DatabaseInfo>), ApiError> {
    // 요청에서 이름 사용 (있으면 덮어쓰기)
    let db_name = request.as_ref().map(|r| r.name.clone()).unwrap_or(name);

    state
        .db_manager
        .create(&db_name, DatabaseConfig::default())
        .await?;
    let db = state.db_manager.get(&db_name).await?;

    Ok((StatusCode::CREATED, Json(db)))
}

/// DELETE /admin/databases/:name - 데이터베이스 삭제
pub async fn delete_database(
    Path(name): Path<String>,
    State(state): State<Arc<AdminAppState>>,
) -> Result<StatusCode, ApiError> {
    state.db_manager.delete(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /admin/databases/:name/start - 데이터베이스 시작
pub async fn start_database(
    Path(name): Path<String>,
    State(state): State<Arc<AdminAppState>>,
) -> Result<Json<DatabaseInfo>, ApiError> {
    state.db_manager.start(&name).await?;
    let db = state.db_manager.get(&name).await?;
    Ok(Json(db))
}

/// POST /admin/databases/:name/stop - 데이터베이스 중지
pub async fn stop_database(
    Path(name): Path<String>,
    State(state): State<Arc<AdminAppState>>,
) -> Result<Json<DatabaseInfo>, ApiError> {
    state.db_manager.stop(&name).await?;
    let db = state.db_manager.get(&name).await?;
    Ok(Json(db))
}

// ============================================================================
// User Management Handlers
// ============================================================================

/// GET /admin/users - 사용자 목록
pub async fn list_users(
    State(state): State<Arc<AdminAppState>>,
) -> Result<Json<Vec<UserInfo>>, HttpError> {
    let users = state
        .security_manager
        .list_users()
        .map_err(|e| HttpError::InternalError(e.to_string()))?;

    let user_infos: Vec<UserInfo> = users
        .into_iter()
        .map(|u| UserInfo {
            username: u.username,
            password_change_required: u.password_change_required,
            suspended: u.suspended,
            home: u.home_database,
        })
        .collect();

    Ok(Json(user_infos))
}

/// GET /admin/users/:name - 사용자 조회
pub async fn get_user(
    Path(name): Path<String>,
    State(state): State<Arc<AdminAppState>>,
) -> Result<Json<UserInfo>, HttpError> {
    let user = state
        .security_manager
        .get_user(&name)
        .map_err(|e| HttpError::InternalError(e.to_string()))?
        .ok_or_else(|| HttpError::NotFound(format!("User not found: {}", name)))?;

    Ok(Json(UserInfo {
        username: user.username,
        password_change_required: user.password_change_required,
        suspended: user.suspended,
        home: user.home_database,
    }))
}

/// POST /admin/users - 사용자 생성
pub async fn create_user(
    State(state): State<Arc<AdminAppState>>,
    Json(request): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserInfo>), HttpError> {
    state
        .security_manager
        .create_user_with_requirement(
            &request.username,
            &request.password,
            request.require_password_change,
        )
        .map_err(|e| HttpError::BadRequest(e.to_string()))?;

    let created = state
        .security_manager
        .get_user(&request.username)
        .map_err(|e| HttpError::InternalError(e.to_string()))?
        .ok_or_else(|| HttpError::InternalError("Failed to create user".to_string()))?;

    Ok((
        StatusCode::CREATED,
        Json(UserInfo {
            username: created.username,
            password_change_required: created.password_change_required,
            suspended: created.suspended,
            home: created.home_database,
        }),
    ))
}

// ============================================================================
// Metrics Handlers
// ============================================================================

/// GET /metrics - Prometheus 메트릭
pub async fn prometheus_metrics(
    State(state): State<Arc<AdminAppState>>,
) -> Result<impl IntoResponse, HttpError> {
    if let Some(stats) = state.engine.memory_limiter_stats() {
        state.metrics.update_engine_memory_metrics(&stats);
    }
    let body = state
        .metrics
        .prometheus_format()
        .map_err(|error| HttpError::InternalError(format!("metrics encoding failed: {error}")))?;
    Ok((
        [(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        body,
    ))
}

/// GET /metrics/json - JSON 메트릭
pub async fn json_metrics(State(state): State<Arc<AdminAppState>>) -> Json<serde_json::Value> {
    if let Some(stats) = state.engine.memory_limiter_stats() {
        state.metrics.update_engine_memory_metrics(&stats);
    }
    Json(state.metrics.json_format())
}
