//! Admin Routes (M2.3)
//!
//! 관리 엔드포인트 라우트 정의

use axum::Router;
use axum::routing::{delete, get, post};
use std::sync::Arc;

use super::admin_handlers::{self, AdminAppState};

/// 관리 라우트 생성
pub fn admin_routes(state: Arc<AdminAppState>) -> Router {
    Router::new()
        // Server info
        .route("/server/info", get(admin_handlers::server_info))
        .route("/server/config", get(admin_handlers::server_config))
        .route("/server/jmx", get(admin_handlers::jmx_metrics))
        // Database management
        .route("/databases", get(admin_handlers::list_databases))
        .route("/databases/{name}", get(admin_handlers::get_database))
        .route("/databases/{name}", post(admin_handlers::create_database))
        .route("/databases/{name}", delete(admin_handlers::delete_database))
        .route(
            "/databases/{name}/start",
            post(admin_handlers::start_database),
        )
        .route(
            "/databases/{name}/stop",
            post(admin_handlers::stop_database),
        )
        // User management
        .route("/users", get(admin_handlers::list_users))
        .route("/users", post(admin_handlers::create_user))
        .route("/users/{name}", get(admin_handlers::get_user))
        .with_state(state)
}

/// 메트릭 라우트 생성
pub fn metrics_routes(state: Arc<AdminAppState>) -> Router {
    Router::new()
        .route("/", get(admin_handlers::prometheus_metrics))
        .route("/json", get(admin_handlers::json_metrics))
        .with_state(state)
}
