//! Unified Router (v1.8.1)
//!
//! 모든 HTTP 엔드포인트를 통합하는 라우터
//! - 기본 엔드포인트 (/, /health, /ready, /metrics)
//! - Cypher 쿼리 엔드포인트 (M2.2)
//! - 관리 엔드포인트 (M2.3)
//! - 스트리밍 엔드포인트 (M2.4)
//! - 인증 엔드포인트 (M4.1, M4.2)

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::server::auth::{ApiKeyLayer, AuthConfig, AuthLayer, CredentialTokenService};
use crate::server::system::{LocalSecurityService, SecurityManager};
use zeta4j_credential::SigningKeyStatus;
use zeta4j_cypher_executor::procedures::{FulltextSearchProvider, VectorSearchProvider};
use zeta4j_graph::config::Edition;
use zeta4j_graph::engine::GraphEngine;

use super::admin_handlers::AdminAppState;
use super::admin_routes::{admin_routes, metrics_routes};
use super::auth_routes::{AuthAppState, auth_routes};
use super::config::{CorsConfig, HttpConfig};
use super::cypher_handlers::CypherAppState;
use super::cypher_routes::cypher_routes;
use super::db_manager::{DatabaseManager, DatabaseManagerConfig};
use super::error::HttpError;
use super::executor::HttpQueryExecutor;
use super::http_transaction::HttpTransactionManager;
use super::metrics::create_metrics;
use super::middleware::{RateLimitLayer, RequestIdLayer};
use super::streaming::{StreamingConfig, create_streaming_executor};
use super::streaming_handlers::StreamingAppState;
use super::streaming_routes::streaming_routes;
use super::types::{ComponentHealth, HealthResponse, ReadyResponse, ServerInfo};
use crate::server::tenant::TenantDatabaseManager;
use zeta4j_cypher_executor::AdminService;

/// 통합 라우터 빌더
pub struct UnifiedRouterBuilder {
    engine: Arc<GraphEngine>,
    config: HttpConfig,
    db_manager_config: DatabaseManagerConfig,
    streaming_config: StreamingConfig,
    security_manager: Option<Arc<SecurityManager>>,
    credential_security: Option<Arc<LocalSecurityService>>,
    credential_tokens: Option<Arc<CredentialTokenService>>,
    /// 테넌트 데이터베이스 관리자 (멀티 DB 지원)
    tenant_db_manager: Option<Arc<TenantDatabaseManager>>,
    /// Fulltext 검색 Provider (#155)
    fulltext_provider: Option<Arc<dyn FulltextSearchProvider>>,
    vector_provider: Option<Arc<dyn VectorSearchProvider>>,
    admin_service: Option<Arc<dyn AdminService>>,
    /// 최대 결과 크기 (0 = 무제한)
    max_result_size: usize,
    /// 쿼리 타임아웃 (밀리초, 0 = 무제한)
    timeout_ms: u64,
    /// 서버 에디션
    edition: Edition,
}

/// Final HTTP middleware stack.
///
/// Edition-specific routes must be merged into the unprotected router before this
/// stack is applied. Keeping the pending stack as a value makes that ordering
/// explicit and prevents routes added by an edition from bypassing authentication.
pub struct HttpRouterProtection {
    auth_enabled: bool,
    compression: bool,
    request_timeout: Duration,
    max_body_size: usize,
    cors: CorsLayer,
    auth_layer: AuthLayer,
    api_key_layer: Option<ApiKeyLayer>,
    rate_limit_layer: Option<RateLimitLayer>,
}

impl HttpRouterProtection {
    /// Apply the complete HTTP middleware stack exactly once.
    pub fn apply(self, app: Router) -> Router {
        #[expect(deprecated)]
        if self.compression {
            app.layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(TimeoutLayer::new(self.request_timeout))
                    .layer(CompressionLayer::new())
                    .layer(DefaultBodyLimit::max(self.max_body_size))
                    .layer(RequestIdLayer::new())
                    .option_layer(self.rate_limit_layer)
                    .option_layer(self.api_key_layer)
                    .option_layer(self.auth_enabled.then_some(self.auth_layer))
                    .layer(self.cors),
            )
        } else {
            app.layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(TimeoutLayer::new(self.request_timeout))
                    .layer(DefaultBodyLimit::max(self.max_body_size))
                    .layer(RequestIdLayer::new())
                    .option_layer(self.rate_limit_layer)
                    .option_layer(self.api_key_layer)
                    .option_layer(self.auth_enabled.then_some(self.auth_layer))
                    .layer(self.cors),
            )
        }
    }
}

impl UnifiedRouterBuilder {
    /// 새 빌더 생성
    pub fn new(engine: Arc<GraphEngine>) -> Self {
        Self {
            engine,
            config: HttpConfig::default(),
            db_manager_config: DatabaseManagerConfig::default(),
            streaming_config: StreamingConfig::default(),
            security_manager: None,
            credential_security: None,
            credential_tokens: None,
            tenant_db_manager: None,
            fulltext_provider: None,
            vector_provider: None,
            admin_service: None,
            max_result_size: 0,
            timeout_ms: 0,
            edition: Edition::Base,
        }
    }

    /// 에디션 설정
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.edition = edition;
        self
    }

    /// Fulltext 검색 Provider 설정 (#155)
    pub fn fulltext_provider(mut self, provider: Arc<dyn FulltextSearchProvider>) -> Self {
        self.fulltext_provider = Some(provider);
        self
    }

    pub fn vector_provider(mut self, provider: Arc<dyn VectorSearchProvider>) -> Self {
        self.vector_provider = Some(provider);
        self
    }

    pub fn admin_service(mut self, service: Arc<dyn AdminService>) -> Self {
        self.admin_service = Some(service);
        self
    }

    /// 쿼리 설정 지정
    pub fn query_config(mut self, max_result_size: usize, timeout_ms: u64) -> Self {
        self.max_result_size = max_result_size;
        self.timeout_ms = timeout_ms;
        self
    }

    /// HTTP 설정 지정
    pub fn http_config(mut self, config: HttpConfig) -> Self {
        self.config = config;
        self
    }

    /// 데이터베이스 관리자 설정
    pub fn db_manager_config(mut self, config: DatabaseManagerConfig) -> Self {
        self.db_manager_config = config;
        self
    }

    /// 스트리밍 설정
    pub fn streaming_config(mut self, config: StreamingConfig) -> Self {
        self.streaming_config = config;
        self
    }

    /// SecurityManager 설정 (RocksDB 기반 인증 시스템)
    pub fn security_manager(mut self, security_manager: Arc<SecurityManager>) -> Self {
        self.security_manager = Some(security_manager);
        self
    }

    pub fn credential_runtime(
        mut self,
        credential_security: Option<Arc<LocalSecurityService>>,
        credential_tokens: Option<Arc<CredentialTokenService>>,
    ) -> Self {
        self.credential_security = credential_security;
        self.credential_tokens = credential_tokens;
        self
    }

    /// 테넌트 데이터베이스 관리자 설정 (멀티 DB 지원)
    pub fn tenant_db_manager(mut self, manager: Arc<TenantDatabaseManager>) -> Self {
        self.tenant_db_manager = Some(manager);
        self
    }

    /// 통합 라우터 빌드
    pub fn build(self) -> Result<Router, HttpError> {
        let (app, protection) = self.build_unprotected()?;
        Ok(protection.apply(app))
    }

    /// Build routes without middleware so an edition can merge its routes first.
    pub fn build_unprotected(self) -> Result<(Router, HttpRouterProtection), HttpError> {
        self.config.validate().map_err(|error| {
            HttpError::InternalError(format!("invalid HTTP configuration: {error}"))
        })?;
        let auth_admin_service = self.admin_service.clone();
        // SecurityManager 사용 (외부에서 반드시 제공해야 함)
        let security_manager = self.security_manager.ok_or_else(|| {
            HttpError::InternalError(
                "SecurityManager must be provided via security_manager()".to_string(),
            )
        })?;

        let credential_runtime = self
            .credential_security
            .clone()
            .zip(self.credential_tokens.clone());
        if self.config.auth.enabled && credential_runtime.is_none() {
            return Err(HttpError::InternalError(
                "canonical credential runtime is required when authentication is enabled"
                    .to_string(),
            ));
        }

        // Cypher 상태 생성 (query 설정 포함)
        let mut cypher_executor = HttpQueryExecutor::new(self.engine.clone())
            .with_query_config(self.max_result_size, self.timeout_ms);
        // Fulltext Provider 연결 (#155)
        if let Some(ref fulltext_provider) = self.fulltext_provider {
            cypher_executor = cypher_executor.with_fulltext_provider(fulltext_provider.clone());
        }
        if let Some(ref vector_provider) = self.vector_provider {
            cypher_executor = cypher_executor.with_vector_provider(vector_provider.clone());
        }
        if let Some(ref admin_service) = self.admin_service {
            cypher_executor = cypher_executor.with_admin_service(admin_service.clone());
        }
        let tx_manager = HttpTransactionManager::default();
        let cypher_state = if let Some(db_manager) = self.tenant_db_manager.clone() {
            // 멀티 DB 모드: TenantDatabaseManager 연결
            let mut state =
                CypherAppState::with_db_manager(cypher_executor, tx_manager, db_manager)
                    .with_query_config(self.max_result_size, self.timeout_ms);
            // 멀티 DB 모드에서 Provider들 공유 (#155)
            if let Some(fulltext_provider) = self.fulltext_provider {
                state = state.with_fulltext_provider(fulltext_provider);
            }
            if let Some(vector_provider) = self.vector_provider {
                state = state.with_vector_provider(vector_provider);
            }
            if let Some(admin_service) = self.admin_service {
                state = state.with_admin_service(admin_service);
            }
            state = state.with_security_manager(Arc::clone(&security_manager));
            Arc::new(state)
        } else {
            // 단일 DB 모드
            let mut state = CypherAppState::new(cypher_executor, tx_manager)
                .with_query_config(self.max_result_size, self.timeout_ms);
            if let Some(admin_service) = self.admin_service {
                state = state.with_admin_service(admin_service);
            }
            state = state.with_security_manager(Arc::clone(&security_manager));
            Arc::new(state)
        };

        // Admin 상태 생성
        let db_manager = Arc::new(DatabaseManager::with_engine(
            self.db_manager_config,
            self.engine.clone(),
        ));
        let metrics = create_metrics()
            .map_err(|error| HttpError::InternalError(format!("metrics setup failed: {error}")))?;
        let admin_state = Arc::new(
            AdminAppState::new(
                db_manager,
                metrics,
                self.config.bind_addr.to_string(),
                self.engine.clone(),
                Arc::clone(&security_manager),
            )
            .with_timezone(&self.config.timezone)
            .with_edition(self.edition)
            .with_credential_security(
                credential_runtime
                    .as_ref()
                    .map(|(credential_security, _)| credential_security.clone()),
            ),
        );

        // Streaming 상태 생성
        let streaming_executor =
            create_streaming_executor(self.engine.clone(), self.streaming_config.clone());
        let streaming_state = Arc::new(StreamingAppState::new(
            streaming_executor,
            self.streaming_config,
        ));

        // Auth 상태 생성
        let auth_routes = credential_runtime.as_ref().map_or_else(
            Router::new,
            |(credential_security, credential_tokens)| {
                auth_routes(Arc::new(AuthAppState::new(
                    Arc::clone(&security_manager),
                    credential_security.clone(),
                    credential_tokens.clone(),
                    auth_admin_service,
                )))
            },
        );

        // 기본 라우트
        let base_routes: Router<()> = Router::new()
            .route("/", get(root_handler))
            .route("/health", get(health_handler))
            .route("/ready", get(ready_handler))
            .fallback(not_found_handler)
            .with_state(admin_state.clone());

        // CORS 레이어
        let cors = create_cors_layer(&self.config.cors);

        // 라우터 결합 (각 라우터는 자체 상태를 가지고 stateless로 변환)
        let app: Router<()> = Router::new()
            // 기본 라우트
            .merge(base_routes)
            // Auth 라우트 (로그인/로그아웃)
            .merge(auth_routes)
            // Cypher 라우트 (쿼리 실행) - with_state로 상태 적용 후 stateless로 변환
            .merge(cypher_routes(cypher_state))
            // 스트리밍 라우트
            .merge(streaming_routes(streaming_state))
            // Admin 라우트 (/admin 프리픽스)
            .nest("/admin", admin_routes(admin_state.clone()))
            // 메트릭 라우트 (/metrics 프리픽스)
            .nest("/metrics", metrics_routes(admin_state));

        // 인증 미들웨어 설정
        let auth_config = AuthConfig {
            required: self.config.auth.enabled,
        };

        // 인증 레이어 생성
        let auth_layer = credential_runtime
            .as_ref()
            .map_or_else(
                || AuthLayer::new(security_manager.clone()),
                |(credential_security, credential_tokens)| {
                    AuthLayer::with_credentials(
                        security_manager.clone(),
                        credential_security.clone(),
                        credential_tokens.clone(),
                    )
                },
            )
            .with_config(auth_config);

        let api_key_layer = credential_runtime
            .as_ref()
            .map(|(credential_security, _)| ApiKeyLayer::new(credential_security.clone()));

        // Rate Limit 레이어 생성
        let rate_limit_layer = if self.config.rate_limit.enabled {
            Some(RateLimitLayer::new(self.config.rate_limit.clone()))
        } else {
            None
        };

        let protection = HttpRouterProtection {
            auth_enabled: self.config.auth.enabled,
            compression: self.config.compression,
            request_timeout: self.config.request_timeout,
            max_body_size: self.config.max_body_size,
            cors,
            auth_layer,
            api_key_layer,
            rate_limit_layer,
        };
        Ok((app, protection))
    }
}

/// CORS 레이어 생성
fn create_cors_layer(config: &CorsConfig) -> CorsLayer {
    use http::Method;
    use http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
    use tower_http::cors::Any;

    let mut cors = CorsLayer::new();

    // 허용된 오리진
    if config.allowed_origins.iter().any(|o| o == "*") {
        cors = cors.allow_origin(Any);
    } else {
        let origins: Vec<_> = config
            .allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        cors = cors.allow_origin(origins);
    }

    // 허용된 메서드
    let methods: Vec<Method> = config
        .allowed_methods
        .iter()
        .filter_map(|m| m.parse().ok())
        .collect();
    cors = cors.allow_methods(methods);

    // 허용된 헤더
    if config.allowed_headers.iter().any(|h| h == "*") {
        cors = cors.allow_headers(Any);
    } else {
        cors = cors.allow_headers([CONTENT_TYPE, AUTHORIZATION, ACCEPT]);
    }

    // 자격 증명
    if config.allow_credentials {
        cors = cors.allow_credentials(true);
    }

    // 최대 캐시 시간
    cors = cors.max_age(config.max_age);

    cors
}

// ============================================================================
// Stateless Handlers (통합 라우터용)
// ============================================================================

/// GET / - 서버 정보
async fn root_handler(State(state): State<Arc<AdminAppState>>) -> Json<ServerInfo> {
    Json(ServerInfo {
        edition: state.edition.name().to_string(),
        ..ServerInfo::default()
    })
}

/// GET /health - 헬스 체크 (Neo4j 스타일 상세 정보)
async fn health_handler(State(state): State<Arc<AdminAppState>>) -> Json<HealthResponse> {
    use sysinfo::{Disks, Pid, System};
    use zeta4j_graph::storage::memory::MemoryStatus;

    // 기본 응답 생성 (시작 시간으로 uptime 자동 계산, 시간대 적용, 에디션)
    let mut response = HealthResponse::healthy()
        .with_edition(state.edition.name())
        .with_uptime(state.start_time)
        .with_timezone(&state.timezone);

    // 서버 컴포넌트
    let server_details = serde_json::json!({
        "server_id": state.server_id,
        "address": state.address,
    });
    response = response.with_component("server", ComponentHealth::up_with_details(server_details));

    // 데이터베이스 컴포넌트 (멀티테넌시)
    let databases = state.db_manager.list().await;
    let total_count = databases.len();
    let online_count = databases.iter().filter(|d| d.status == "online").count();
    let offline_count = total_count - online_count;

    let db_list: Vec<serde_json::Value> = databases
        .iter()
        .map(|db| {
            serde_json::json!({
                "name": db.name,
                "status": db.status,
                "default": db.default,
                "store_size": db.store_size,
                "tx_count": db.tx_count,
            })
        })
        .collect();

    let databases_details = serde_json::json!({
        "total_count": total_count,
        "online_count": online_count,
        "offline_count": offline_count,
        "databases": db_list,
    });

    let databases_status = if offline_count == total_count && total_count > 0 {
        ComponentHealth::down_with_details(databases_details)
    } else if offline_count > 0 {
        ComponentHealth::degraded_with_details(databases_details)
    } else {
        ComponentHealth::up_with_details(databases_details)
    };
    response = response.with_component("databases", databases_status);

    // 메모리 컴포넌트 (실제 메모리 추적)
    // 1. 프로세스 메모리 (sysinfo)
    let mut sys = System::new();
    let pid = Pid::from_u32(std::process::id());
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);

    let (process_memory, process_virtual) = sys
        .process(pid)
        .map(|p| (p.memory(), p.virtual_memory()))
        .unwrap_or((0, 0));

    // 2. GraphEngine 메모리 리미터 통계
    let memory_component = if let Some(stats) = state.engine.memory_limiter_stats() {
        let status_str = match stats.status {
            MemoryStatus::Normal => "normal",
            MemoryStatus::Warning => "warning",
            MemoryStatus::Critical => "critical",
            MemoryStatus::Exceeded => "exceeded",
        };

        let details = serde_json::json!({
            "process_memory": {
                "rss_bytes": process_memory,
                "virtual_bytes": process_virtual,
                "scope": "operating_system_process"
            },
            "engine_accounted_memory": {
                "enabled": true,
                "current_bytes": stats.current_usage,
                "limit_bytes": stats.max_allowed,
                "peak_bytes": stats.peak_usage,
                "usage_percent": (stats.usage_ratio() * 100.0).round(),
                "available_bytes": stats.available(),
                "status": status_str,
                "categories": stats.categories.iter().map(|category| serde_json::json!({
                    "category": category.category.map(|value| value.as_str()),
                    "current_bytes": category.current_usage,
                    "peak_bytes": category.peak_usage,
                })).collect::<Vec<_>>(),
                "reconciliation_drift_bytes": stats.reconciliation_drift,
            },
            "admission": {
                "total": stats.allocation_count,
                "released": stats.deallocation_count,
                "rejected_reservations": stats.rejected_allocations,
                "rejected_bytes": stats.rejected_bytes,
                "cleanup_attempted": stats.auto_cleanup_count,
                "cleanup_succeeded": stats.cleanup_succeeded_count,
                "cleanup_reclaimed_bytes": stats.cleanup_reclaimed_bytes,
            },
        });

        match stats.status {
            MemoryStatus::Normal | MemoryStatus::Warning => {
                ComponentHealth::up_with_details(details)
            }
            MemoryStatus::Critical => ComponentHealth::degraded_with_details(details),
            MemoryStatus::Exceeded => ComponentHealth::down_with_details(details),
        }
    } else {
        // 메모리 리미터 미설정 시 프로세스 메모리만 반환
        let details = serde_json::json!({
            "process_memory": {
                "rss_bytes": process_memory,
                "virtual_bytes": process_virtual,
                "scope": "operating_system_process"
            },
            "engine_accounted_memory": {
                "enabled": false,
                "status": "disabled"
            },
        });
        ComponentHealth::up_with_details(details)
    };

    response = response.with_component("memory", memory_component);

    // 디스크 컴포넌트 (데이터 디렉토리)
    let data_dir = std::path::PathBuf::from(&state.engine.config().data_path);
    let disk_component = match std::fs::metadata(&data_dir) {
        Ok(_metadata) => {
            // 디렉토리 총 크기 계산 (간단히 - 실제로는 재귀적 계산 필요)
            let dir_size = calculate_dir_size(&data_dir).unwrap_or(0);

            // 파일시스템 사용 가능 공간 (sysinfo 사용)
            let disks = Disks::new_with_refreshed_list();

            let (total_space, available_space) = disks
                .iter()
                .find(|d| data_dir.starts_with(d.mount_point()))
                .map(|d| (d.total_space(), d.available_space()))
                .unwrap_or((0, 0));

            let usage_percent = if total_space > 0 {
                ((total_space - available_space) as f64 / total_space as f64 * 100.0).round()
            } else {
                0.0
            };

            let used_bytes = total_space.saturating_sub(available_space);
            let details = serde_json::json!({
                "data_dir": data_dir.to_string_lossy(),
                "data_size": format_bytes(dir_size),
                "total": format_bytes(total_space),
                "available": format_bytes(available_space),
                "used": format_bytes(used_bytes),
                "usage_percent": usage_percent,
            });

            // 남은 공간 10% 미만이면 DEGRADED, 5% 미만이면 DOWN
            if total_space > 0 {
                let free_ratio = available_space as f64 / total_space as f64;
                if free_ratio < 0.05 {
                    ComponentHealth::down_with_details(details)
                } else if free_ratio < 0.10 {
                    ComponentHealth::degraded_with_details(details)
                } else {
                    ComponentHealth::up_with_details(details)
                }
            } else {
                ComponentHealth::up_with_details(details)
            }
        }
        Err(e) => {
            let details = serde_json::json!({
                "data_dir": data_dir.to_string_lossy(),
                "error": e.to_string(),
            });
            ComponentHealth::down_with_details(details)
        }
    };
    response = response.with_component("disk", disk_component);

    // 전체 상태 계산
    response.compute_status();
    Json(response)
}

/// GET /ready - 준비 상태 (실제 시스템 상태 기반)
async fn ready_handler(
    State(state): State<Arc<AdminAppState>>,
) -> Result<Json<ReadyResponse>, (StatusCode, Json<ReadyResponse>)> {
    match state.security_manager.has_pending_bootstrap() {
        Ok(true) => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ReadyResponse::not_ready(
                    "Initial administrator credential change is pending",
                )),
            ));
        }
        Err(error) => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ReadyResponse::not_ready(format!(
                    "Security state is not ready: {error}"
                ))),
            ));
        }
        Ok(false) => {}
    }

    if let Some(credential_security) = &state.credential_security {
        match credential_security.local_signer_status() {
            Ok(SigningKeyStatus::Active) => {}
            Ok(_) | Err(_) => {
                return Err((
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ReadyResponse::not_ready(
                        "Credential signing state is not ready",
                    )),
                ));
            }
        }
    }

    // 기본 데이터베이스(zeta4j)가 온라인인지 확인
    if state.db_manager.is_online("zeta4j").await {
        Ok(Json(ReadyResponse::ready()))
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse::not_ready("Default database is not ready")),
        ))
    }
}

/// 404 핸들러
async fn not_found_handler() -> HttpError {
    HttpError::NotFound("Resource not found".to_string())
}

// ============================================================================
// Public Functions
// ============================================================================

/// 간단한 통합 라우터 생성 (기본 설정)
pub fn create_unified_router(engine: Arc<GraphEngine>) -> Result<Router, HttpError> {
    UnifiedRouterBuilder::new(engine).build()
}

/// 설정과 함께 통합 라우터 생성
pub fn create_unified_router_with_config(
    engine: Arc<GraphEngine>,
    config: HttpConfig,
) -> Result<Router, HttpError> {
    UnifiedRouterBuilder::new(engine)
        .http_config(config)
        .build()
}

/// 바이트를 휴먼 리더블 포맷으로 변환
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// 디렉토리 크기 계산 (재귀)
fn calculate_dir_size(path: &std::path::Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                total += calculate_dir_size(&path).unwrap_or(0);
            } else {
                total += entry.metadata()?.len();
            }
        }
    }
    Ok(total)
}
