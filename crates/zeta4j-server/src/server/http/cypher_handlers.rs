// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Cypher HTTP Handlers
//!
//! Neo4j HTTP API 호환 Cypher 엔드포인트 핸들러

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::sync::Arc;

use super::cypher_types::{CypherError, CypherRequest, CypherResponse, Statement};
use super::error::HttpError;
use super::executor::{
    ExecutionConfig, HttpQueryExecutor, execute_and_record, execute_statement_with_config,
    rollback_and_return_error,
};
use super::http_transaction::HttpTransactionManager;
use super::http_transaction::TransactionOwner;
use crate::server::access::{PublicError, authorize_cypher_principal};
use crate::server::auth::AuthenticatedUser;
use crate::server::system::SecurityManager;
use crate::server::tenant::TenantDatabaseManager;
use zeta4j_common::error::ZetaError;
use zeta4j_cypher_executor::AdminService;
use zeta4j_cypher_executor::ast::DatabaseTarget;
use zeta4j_cypher_executor::parser::parse_use;
use zeta4j_cypher_executor::procedures::{FulltextSearchProvider, VectorSearchProvider};
use zeta4j_graph::engine::GraphEngine;

// ============================================================================
// App State (Cypher 확장)
// ============================================================================

/// Cypher 기능이 포함된 앱 상태
pub struct CypherAppState {
    /// 기본 쿼리 실행기 (단일 DB 모드용)
    pub executor: HttpQueryExecutor,

    /// 트랜잭션 매니저
    pub tx_manager: HttpTransactionManager,

    /// 기본 데이터베이스 이름
    pub default_database: String,

    /// 테넌트 데이터베이스 관리자 (멀티 DB 모드용)
    pub db_manager: Option<Arc<TenantDatabaseManager>>,

    /// 최대 결과 크기 (0 = 무제한)
    pub max_result_size: usize,

    /// 쿼리 타임아웃 (밀리초, 0 = 무제한)
    pub timeout_ms: u64,

    /// Fulltext 검색 Provider (#155)
    pub fulltext_provider: Option<Arc<dyn FulltextSearchProvider>>,
    pub vector_provider: Option<Arc<dyn VectorSearchProvider>>,

    /// Canonical admin service.
    pub admin_service: Option<Arc<dyn AdminService>>,
    /// Current canonical identity store.
    pub security_manager: Option<Arc<SecurityManager>>,
}

impl CypherAppState {
    /// 새 상태 생성 (단일 DB 모드)
    pub fn new(executor: HttpQueryExecutor, tx_manager: HttpTransactionManager) -> Self {
        Self {
            executor,
            tx_manager,
            default_database: "zeta4j".to_string(),
            db_manager: None,
            max_result_size: 0,
            timeout_ms: 0,
            fulltext_provider: None,
            vector_provider: None,
            admin_service: None,
            security_manager: None,
        }
    }

    /// 멀티 DB 모드 상태 생성
    pub fn with_db_manager(
        executor: HttpQueryExecutor,
        tx_manager: HttpTransactionManager,
        db_manager: Arc<TenantDatabaseManager>,
    ) -> Self {
        Self {
            executor,
            tx_manager,
            default_database: "zeta4j".to_string(),
            db_manager: Some(db_manager),
            max_result_size: 0,
            timeout_ms: 0,
            fulltext_provider: None,
            vector_provider: None,
            admin_service: None,
            security_manager: None,
        }
    }

    /// Fulltext Provider 설정 (빌더 패턴)
    pub fn with_fulltext_provider(mut self, provider: Arc<dyn FulltextSearchProvider>) -> Self {
        self.fulltext_provider = Some(provider);
        self
    }

    pub fn with_vector_provider(mut self, provider: Arc<dyn VectorSearchProvider>) -> Self {
        self.vector_provider = Some(provider);
        self
    }

    pub fn with_admin_service(mut self, service: Arc<dyn AdminService>) -> Self {
        self.admin_service = Some(service);
        self
    }

    pub fn with_security_manager(mut self, manager: Arc<SecurityManager>) -> Self {
        self.security_manager = Some(manager);
        self
    }

    /// 기본 데이터베이스 설정
    pub fn with_default_database(mut self, name: impl Into<String>) -> Self {
        self.default_database = name.into();
        self
    }

    /// 쿼리 설정 적용
    pub fn with_query_config(mut self, max_result_size: usize, timeout_ms: u64) -> Self {
        self.max_result_size = max_result_size;
        self.timeout_ms = timeout_ms;
        self
    }

    /// 멀티 DB 모드 여부 확인
    pub fn is_multi_db_mode(&self) -> bool {
        self.db_manager.is_some()
    }

    /// 실행 설정 생성 (Phase 2)
    pub fn build_execution_config(&self, principal: Option<&AuthenticatedUser>) -> ExecutionConfig {
        let mut config =
            ExecutionConfig::new().with_query_limits(self.max_result_size, self.timeout_ms);

        if let Some(ref provider) = self.fulltext_provider {
            config = config.with_fulltext(provider.clone());
        }
        if let Some(ref provider) = self.vector_provider {
            config = config.with_vector(provider.clone());
        }
        if let Some(ref service) = self.admin_service {
            config = config.with_admin_service(service.clone());
        }
        if let Some(user) = principal {
            config = config.with_current_user(user.username.clone());
        }
        config
    }

    /// 데이터베이스 엔진 조회 (존재하지 않으면 에러)
    async fn get_engine_or_error(&self, db_name: &str) -> Result<Arc<GraphEngine>, CypherError> {
        if let Some(ref db_manager) = self.db_manager {
            db_manager.get_engine(db_name).await.ok_or_else(|| {
                CypherError::new(
                    "Zeta.ClientError.Database.DatabaseNotFound",
                    format!("Database '{}' does not exist", db_name),
                )
            })
        } else {
            // 단일 DB 모드: db_manager가 없으면 에러
            Err(CypherError::new(
                "Zeta.ClientError.Database.DatabaseNotFound",
                format!(
                    "Database '{}' not available in single-database mode",
                    db_name
                ),
            ))
        }
    }

    /// USE 절을 고려하여 엔진 조회
    ///
    /// 우선순위:
    /// 1. 쿼리 내 USE 문의 데이터베이스 (있으면 필수 존재)
    /// 2. URL path의 db_name (멀티 DB 모드에서 필수 존재)
    /// 3. 단일 DB 모드: 기본 실행기의 엔진
    ///
    /// 에러:
    /// - USE 절의 데이터베이스가 존재하지 않으면 에러
    /// - 멀티 DB 모드에서 URL db_name이 존재하지 않으면 에러
    pub async fn resolve_engine(
        &self,
        query: &str,
        url_db_name: &str,
    ) -> Result<Arc<GraphEngine>, CypherError> {
        // USE 절에서 데이터베이스 이름 추출 시도
        if let Some(use_db) = extract_use_database(query) {
            // USE 절이 있으면 해당 DB가 반드시 존재해야 함
            return self.get_engine_or_error(&use_db).await;
        }

        // 멀티 DB 모드: URL path의 데이터베이스 필수 존재
        if self.is_multi_db_mode() {
            return self.get_engine_or_error(url_db_name).await;
        }

        // 단일 DB 모드: 기본 실행기 엔진 사용
        Ok(self.executor.engine())
    }

    /// Admits an authenticated principal to the target database.
    async fn admit_statements(
        &self,
        db_name: &str,
        principal: Option<&AuthenticatedUser>,
    ) -> Result<(), CypherErrorResponse> {
        let Some(principal) = principal else {
            return Ok(());
        };
        let database_manager = self
            .db_manager
            .as_ref()
            .ok_or_else(|| CypherErrorResponse::from_public_error(PublicError::PolicyInvariant))?;
        authorize_cypher_principal(database_manager, &principal.principal, db_name)
            .await
            .map_err(CypherErrorResponse::from_public_error)
    }

    async fn transaction_owner(
        &self,
        db_name: &str,
        principal: Option<&AuthenticatedUser>,
    ) -> Result<TransactionOwner, CypherErrorResponse> {
        let database_manager = self
            .db_manager
            .as_ref()
            .ok_or_else(|| CypherErrorResponse::from_public_error(PublicError::PolicyInvariant))?;
        let database = database_manager
            .get(db_name)
            .await
            .map_err(|_| CypherErrorResponse::from_public_error(PublicError::DatabaseNotFound))?;
        let tenant_id = database
            .tenant_id
            .map_or_else(|| "system".to_string(), |id| format!("tenant.{id}"));
        let (principal_id, security_generation) = principal.map_or_else(
            || ("local-test-anonymous".to_string(), 0),
            |user| {
                (
                    user.principal.id.as_str().to_string(),
                    user.principal.security_generation,
                )
            },
        );
        Ok(TransactionOwner {
            principal_id,
            security_generation,
            tenant_id,
            database_id: format!("database.{}", database.id),
        })
    }
}

/// 쿼리 문자열에서 USE 절의 데이터베이스 이름 추출
///
/// 쿼리가 "USE db_name ..." 형태로 시작하면 db_name 반환
/// USE graph.byName(...) 동적 형태는 런타임에 평가 불가하므로 None 반환
fn extract_use_database(query: &str) -> Option<String> {
    let trimmed = query.trim();

    // 대소문자 무시 체크
    if !trimmed.to_uppercase().starts_with("USE ") {
        return None;
    }

    // nom 파서로 USE 절 파싱
    match parse_use(trimmed) {
        Ok((_, use_clause)) => {
            match use_clause.database {
                DatabaseTarget::Named(name) => Some(name),
                DatabaseTarget::Dynamic(_) => {
                    // 동적 데이터베이스 선택은 파라미터 평가가 필요하므로
                    // 이 단계에서는 처리 불가
                    None
                }
            }
        }
        Err(_) => None,
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /db/{db_name}/tx/commit - Auto-commit 쿼리 실행
///
/// 단일 트랜잭션 내에서 쿼리 실행 후 자동 커밋
///
/// v1.77.0: spawn_blocking을 통해 워커 스레드 블로킹 방지
/// v2.x: URL path의 db_name 및 USE 절로 데이터베이스 엔진 조회
pub async fn auto_commit(
    Path(db_name): Path<String>,
    State(state): State<Arc<CypherAppState>>,
    principal: Option<Extension<AuthenticatedUser>>,
    Json(request): Json<CypherRequest>,
) -> Result<impl IntoResponse, CypherErrorResponse> {
    // 빈 요청 처리
    if request.statements.is_empty() {
        return Ok(Json(CypherResponse::success(vec![])));
    }
    state
        .admit_statements(&db_name, principal.as_ref().map(|user| &user.0))
        .await?;

    // 첫 번째 문장에서 USE 절 확인하여 엔진 결정
    let first_query = &request.statements[0].statement;
    let engine = state
        .resolve_engine(first_query, &db_name)
        .await
        .map_err(CypherErrorResponse::from_error)?;

    let results = HttpQueryExecutor::execute_statements_with_all_providers(
        engine,
        state.fulltext_provider.clone(),
        state.vector_provider.clone(),
        state.admin_service.clone(),
        principal.as_ref().map(|user| user.0.username.clone()),
        &request.statements,
        state.max_result_size,
        state.timeout_ms,
    )
    .await
    .map_err(CypherErrorResponse::from_error)?;

    Ok(Json(CypherResponse::success(results)))
}

/// POST /db/{db_name}/tx - 트랜잭션 시작
///
/// 새 트랜잭션을 시작하고 선택적으로 초기 쿼리 실행
///
/// v1.77.0: spawn_blocking을 통해 워커 스레드 블로킹 방지
/// v2.x: URL path의 db_name 및 USE 절로 데이터베이스 엔진 조회
pub async fn begin_transaction(
    Path(db_name): Path<String>,
    State(state): State<Arc<CypherAppState>>,
    principal: Option<Extension<AuthenticatedUser>>,
    Json(request): Json<Option<CypherRequest>>,
) -> Result<impl IntoResponse, CypherErrorResponse> {
    if request.is_some() {
        state
            .admit_statements(&db_name, principal.as_ref().map(|user| &user.0))
            .await?;
    }
    // 트랜잭션 시작
    let owner = state
        .transaction_owner(&db_name, principal.as_ref().map(|user| &user.0))
        .await?;
    let tx_id = state
        .tx_manager
        .begin(&db_name, owner)
        .map_err(CypherErrorResponse::from_http_error)?;

    // 초기 쿼리 실행 (있는 경우) - Phase 2 헬퍼 사용
    let mut results = vec![];
    if let Some(req) = request
        && !req.statements.is_empty()
    {
        let first_query = &req.statements[0].statement;
        let engine = state
            .resolve_engine(first_query, &db_name)
            .await
            .map_err(|e| {
                let _ = state.tx_manager.rollback(&tx_id);
                CypherErrorResponse::from_error(e)
            })?;

        results = execute_and_record(
            engine,
            &tx_id,
            &state.tx_manager,
            &req.statements,
            state.build_execution_config(principal.as_ref().map(|user| &user.0)),
        )
        .await
        .map_err(|e| {
            CypherErrorResponse::from_zeta_error(rollback_and_return_error(
                &state.tx_manager,
                &tx_id,
                e,
            ))
        })?;
    }

    // 만료 시간 조회
    let expires = state
        .tx_manager
        .touch(&tx_id)
        .map_err(CypherErrorResponse::from_http_error)?;

    let response = CypherResponse::success(results).with_transaction(&db_name, &tx_id, &expires);

    Ok(Json(response))
}

/// POST /db/{db_name}/tx/{tx_id} - 트랜잭션 내 쿼리 실행
///
/// 열린 트랜잭션에서 추가 쿼리 실행
///
/// v1.77.0: spawn_blocking을 통해 워커 스레드 블로킹 방지
/// v2.x: URL path의 db_name 및 USE 절로 데이터베이스 엔진 조회
pub async fn execute_in_transaction(
    Path((db_name, tx_id)): Path<(String, String)>,
    State(state): State<Arc<CypherAppState>>,
    principal: Option<Extension<AuthenticatedUser>>,
    Json(request): Json<CypherRequest>,
) -> Result<impl IntoResponse, CypherErrorResponse> {
    state
        .admit_statements(&db_name, principal.as_ref().map(|user| &user.0))
        .await?;
    let owner = state
        .transaction_owner(&db_name, principal.as_ref().map(|user| &user.0))
        .await?;
    state
        .tx_manager
        .verify_owner(&tx_id, &db_name, &owner)
        .map_err(CypherErrorResponse::from_http_error)?;

    // 트랜잭션 존재 확인
    let _ = state
        .tx_manager
        .get(&tx_id)
        .map_err(CypherErrorResponse::from_http_error)?;

    // 쿼리 실행 - Phase 2 헬퍼 사용
    let results = if request.statements.is_empty() {
        vec![]
    } else {
        let first_query = &request.statements[0].statement;
        let engine = state
            .resolve_engine(first_query, &db_name)
            .await
            .map_err(CypherErrorResponse::from_error)?;

        execute_and_record(
            engine,
            &tx_id,
            &state.tx_manager,
            &request.statements,
            state.build_execution_config(principal.as_ref().map(|user| &user.0)),
        )
        .await
        .map_err(|error| {
            CypherErrorResponse::from_zeta_error(rollback_and_return_error(
                &state.tx_manager,
                &tx_id,
                error,
            ))
        })?
    };

    // 만료 시간 갱신
    let expires = state
        .tx_manager
        .touch(&tx_id)
        .map_err(CypherErrorResponse::from_http_error)?;

    let response = CypherResponse::success(results).with_transaction(&db_name, &tx_id, &expires);

    Ok(Json(response))
}

/// POST /db/{db_name}/tx/{tx_id}/commit - 트랜잭션 커밋
///
/// 트랜잭션을 커밋하고 선택적으로 마지막 쿼리 실행
///
/// v1.77.0: spawn_blocking을 통해 워커 스레드 블로킹 방지
/// v2.x: URL path의 db_name 및 USE 절로 데이터베이스 엔진 조회
pub async fn commit_transaction(
    Path((db_name, tx_id)): Path<(String, String)>,
    State(state): State<Arc<CypherAppState>>,
    principal: Option<Extension<AuthenticatedUser>>,
    Json(request): Json<Option<CypherRequest>>,
) -> Result<impl IntoResponse, CypherErrorResponse> {
    if request.is_some() {
        state
            .admit_statements(&db_name, principal.as_ref().map(|user| &user.0))
            .await?;
    }
    let owner = state
        .transaction_owner(&db_name, principal.as_ref().map(|user| &user.0))
        .await?;
    state
        .tx_manager
        .verify_owner(&tx_id, &db_name, &owner)
        .map_err(CypherErrorResponse::from_http_error)?;

    // 트랜잭션 존재 확인
    let _ = state
        .tx_manager
        .get(&tx_id)
        .map_err(CypherErrorResponse::from_http_error)?;

    // 마지막 쿼리 실행 (있는 경우) - Phase 2 헬퍼 사용
    let mut results = vec![];
    if let Some(req) = request
        && !req.statements.is_empty()
    {
        let first_query = &req.statements[0].statement;
        let engine = state
            .resolve_engine(first_query, &db_name)
            .await
            .map_err(|e| {
                let _ = state.tx_manager.rollback(&tx_id);
                CypherErrorResponse::from_error(e)
            })?;

        results = execute_and_record(
            engine,
            &tx_id,
            &state.tx_manager,
            &req.statements,
            state.build_execution_config(principal.as_ref().map(|user| &user.0)),
        )
        .await
        .map_err(|e| {
            CypherErrorResponse::from_zeta_error(rollback_and_return_error(
                &state.tx_manager,
                &tx_id,
                e,
            ))
        })?;
    }

    // 커밋
    let _ = state
        .tx_manager
        .commit(&tx_id)
        .map_err(CypherErrorResponse::from_http_error)?;

    Ok(Json(CypherResponse::success(results)))
}

/// DELETE /db/{db_name}/tx/{tx_id} - 트랜잭션 롤백
///
/// 트랜잭션을 롤백하고 변경 사항 취소
pub async fn rollback_transaction(
    Path((db_name, tx_id)): Path<(String, String)>,
    State(state): State<Arc<CypherAppState>>,
    principal: Option<Extension<AuthenticatedUser>>,
) -> Result<impl IntoResponse, CypherErrorResponse> {
    let owner = state
        .transaction_owner(&db_name, principal.as_ref().map(|user| &user.0))
        .await?;
    state
        .tx_manager
        .verify_owner(&tx_id, &db_name, &owner)
        .map_err(CypherErrorResponse::from_http_error)?;
    state
        .tx_manager
        .rollback(&tx_id)
        .map_err(CypherErrorResponse::from_http_error)?;

    Ok(Json(CypherResponse::success(vec![])))
}

/// POST /db/{db_name}/query/v2 - Query API v2 (Neo4j 5.x 호환)
///
/// 단순화된 쿼리 API
///
/// v1.77.0: spawn_blocking을 통해 워커 스레드 블로킹 방지
/// v2.x: URL path의 db_name 및 USE 절로 데이터베이스 엔진 조회
pub async fn query_v2(
    Path(db_name): Path<String>,
    State(state): State<Arc<CypherAppState>>,
    principal: Option<Extension<AuthenticatedUser>>,
    Json(request): Json<QueryV2Request>,
) -> Result<impl IntoResponse, CypherErrorResponse> {
    let statement = Statement {
        statement: request.statement.clone(),
        parameters: request.parameters.unwrap_or_default(),
        result_data_contents: vec![],
        include_stats: request.include_stats.unwrap_or(false),
    };
    state
        .admit_statements(&db_name, principal.as_ref().map(|user| &user.0))
        .await?;

    // USE 절 및 URL db_name으로 엔진 결정
    let engine = state
        .resolve_engine(&request.statement, &db_name)
        .await
        .map_err(CypherErrorResponse::from_error)?;

    let result = execute_statement_with_config(
        engine,
        &statement,
        state.build_execution_config(principal.as_ref().map(|user| &user.0)),
    )
    .await
    .map_err(CypherErrorResponse::from_zeta_error)?;

    Ok(Json(CypherResponse::success(vec![result])))
}

/// Query API v2 요청
#[derive(Debug, serde::Deserialize)]
pub struct QueryV2Request {
    /// Cypher 쿼리
    pub statement: String,

    /// 파라미터
    #[serde(default)]
    pub parameters: Option<std::collections::HashMap<String, serde_json::Value>>,

    /// 통계 포함
    #[serde(rename = "includeStats")]
    pub include_stats: Option<bool>,
}

// ============================================================================
// Error Response
// ============================================================================

/// Cypher 에러 응답
pub struct CypherErrorResponse {
    status: StatusCode,
    response: CypherResponse,
}

impl CypherErrorResponse {
    fn from_public_error(error: PublicError) -> Self {
        let status = match error {
            PublicError::AuthenticationFailed
            | PublicError::TokenExpired
            | PublicError::PrincipalInactive => StatusCode::UNAUTHORIZED,
            PublicError::DatabaseNotFound => StatusCode::NOT_FOUND,
            PublicError::PolicyInvariant => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self {
            status,
            response: CypherResponse {
                results: Vec::new(),
                errors: vec![CypherError::new(
                    error.code(),
                    if status == StatusCode::FORBIDDEN {
                        "Permission denied"
                    } else if status == StatusCode::UNAUTHORIZED {
                        "Authentication failed"
                    } else {
                        "Authorization policy unavailable"
                    },
                )],
                commit: None,
                transaction: None,
            },
        }
    }

    /// CypherError로부터 생성
    pub fn from_error(error: CypherError) -> Self {
        let status = Self::error_to_status(&error);
        Self {
            status,
            response: CypherResponse {
                results: vec![],
                errors: vec![error],
                commit: None,
                transaction: None,
            },
        }
    }

    /// ZetaError로부터 생성 (Phase 2)
    pub fn from_zeta_error(error: ZetaError) -> Self {
        let status =
            StatusCode::from_u16(error.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let cypher_error = CypherError::new(error.code().to_string(), error.to_string());
        Self {
            status,
            response: CypherResponse {
                results: vec![],
                errors: vec![cypher_error],
                commit: None,
                transaction: None,
            },
        }
    }

    /// HttpError로부터 생성
    pub fn from_http_error(error: HttpError) -> Self {
        let (status, cypher_error) = match error {
            HttpError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                CypherError::transaction_not_found(&msg),
            ),
            HttpError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                CypherError::new("Zeta.ClientError.Request.Invalid", msg),
            ),
            HttpError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                CypherError::new(
                    "Zeta.TransientError.Transaction.MaximumTransactionLimitReached",
                    "Maximum number of concurrent transactions reached",
                ),
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                CypherError::internal_error(format!("{}", error)),
            ),
        };

        Self {
            status,
            response: CypherResponse {
                results: vec![],
                errors: vec![cypher_error],
                commit: None,
                transaction: None,
            },
        }
    }

    /// 에러 코드에서 HTTP 상태 코드 결정
    fn error_to_status(error: &CypherError) -> StatusCode {
        match error.code.as_str() {
            "CY-ADMIN-001" => return StatusCode::SERVICE_UNAVAILABLE,
            "CY-ADMIN-002" => return StatusCode::NOT_IMPLEMENTED,
            "CY-ADMIN-003" => return StatusCode::UNAUTHORIZED,
            "CY-ADMIN-004" => return StatusCode::INTERNAL_SERVER_ERROR,
            "GR-MEM-001" => return StatusCode::SERVICE_UNAVAILABLE,
            _ => {}
        }
        if error.code.contains("ClientError") {
            if error.code.contains("Security") {
                if error.code.contains("Unauthorized") {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::FORBIDDEN
                }
            } else if error.code.contains("NotFound") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            }
        } else if error.code.contains("TransientError") {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

impl IntoResponse for CypherErrorResponse {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(self.response)).into_response()
    }
}

#[cfg(test)]
mod admin_error_tests {
    use super::*;

    #[test]
    fn stable_admin_codes_have_declared_http_statuses() {
        let cases = [
            ("CY-ADMIN-001", StatusCode::SERVICE_UNAVAILABLE),
            ("CY-ADMIN-002", StatusCode::NOT_IMPLEMENTED),
            ("CY-ADMIN-003", StatusCode::UNAUTHORIZED),
            ("CY-ADMIN-004", StatusCode::INTERNAL_SERVER_ERROR),
        ];
        for (code, expected) in cases {
            let error = CypherError::new(code, "test");
            assert_eq!(CypherErrorResponse::error_to_status(&error), expected);
        }
    }

    #[test]
    fn transaction_lock_errors_keep_http_status_and_code() {
        let timeout = CypherErrorResponse::from_zeta_error(ZetaError::lock_timeout("node:1", 250));
        assert_eq!(timeout.status, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(timeout.response.errors[0].code, "TX-LOCK-002");

        let deadlock = CypherErrorResponse::from_zeta_error(ZetaError::deadlock(vec![1, 2, 1]));
        assert_eq!(deadlock.status, StatusCode::CONFLICT);
        assert_eq!(deadlock.response.errors[0].code, "TX-LOCK-005");
    }

    #[test]
    fn memory_admission_is_retryable_service_unavailable() {
        let response =
            CypherErrorResponse::from_zeta_error(ZetaError::memory_admission("limit reached"));
        assert_eq!(response.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.response.errors[0].code, "GR-MEM-001");
    }
}
