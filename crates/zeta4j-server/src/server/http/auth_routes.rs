// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Authentication Routes
//!
//! HTTP API 인증 엔드포인트
//! - POST /auth/login - 로그인 (Basic Auth → JWT 토큰 발급)
//! - POST /auth/refresh - 토큰 갱신
//! - POST /auth/logout - durable session/family 폐기

use std::sync::Arc;

use crate::server::access::PublicError;
use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::{StatusCode, header::AUTHORIZATION},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use zeta4j_credential::{CredentialAudience, CredentialFailure, CredentialSecret, SessionId};
use zeta4j_cypher_executor::{AdminService, AdminServiceError};

use crate::server::auth::{
    AuthenticatedUser, CredentialTokenService, extract_basic_auth, is_basic_auth,
};
use crate::server::system::{
    LocalSecurityService, SecurityManager, SessionDescriptor, SystemError,
};

/// Auth 라우터 상태
pub struct AuthAppState {
    pub security_manager: Arc<SecurityManager>,
    pub credential_security: Arc<LocalSecurityService>,
    pub credential_tokens: Arc<CredentialTokenService>,
    pub admin_service: Option<Arc<dyn AdminService>>,
}

impl AuthAppState {
    pub fn new(
        security_manager: Arc<SecurityManager>,
        credential_security: Arc<LocalSecurityService>,
        credential_tokens: Arc<CredentialTokenService>,
        admin_service: Option<Arc<dyn AdminService>>,
    ) -> Self {
        Self {
            security_manager,
            credential_security,
            credential_tokens,
            admin_service,
        }
    }
}

/// 로그인 요청 (JSON body)
#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default = "default_http_audience")]
    pub audience: CredentialAudience,
}

/// 토큰 갱신 요청
#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
    #[serde(default = "default_http_audience")]
    pub audience: CredentialAudience,
}

#[derive(Deserialize)]
pub struct AudienceQuery {
    #[serde(default = "default_http_audience")]
    pub audience: CredentialAudience,
}

#[derive(Deserialize)]
pub struct RevokeSessionRequest {
    pub session_id: String,
    #[serde(default = "default_http_audience")]
    pub audience: CredentialAudience,
}

fn default_http_audience() -> CredentialAudience {
    CredentialAudience::Http
}

/// One-time bootstrap completion request.
#[derive(Deserialize)]
pub struct BootstrapCompleteRequest {
    pub username: String,
    pub current_password: String,
    pub new_password: String,
}

/// 로그인 응답
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub access_expires_at: i64,
    pub refresh_expires_at: i64,
    pub session_id: String,
}

/// 에러 응답
#[derive(Debug, Serialize)]
pub struct AuthErrorResponse {
    pub error: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct WhoAmIResponse {
    pub principal_id: String,
    pub principal_kind: String,
    pub status: String,
    pub security_generation: u64,
}

/// Auth 라우트 생성
pub fn auth_routes(state: Arc<AuthAppState>) -> Router {
    Router::new()
        .route("/auth/login", post(login_handler))
        .route("/auth/bootstrap/complete", post(bootstrap_complete_handler))
        .route("/auth/refresh", post(refresh_handler))
        .route("/auth/logout", post(logout_handler))
        .route("/auth/whoami", get(whoami_handler))
        .route("/auth/sessions", get(sessions_handler))
        .route("/auth/revoke-session", post(revoke_session_handler))
        .with_state(state)
}

async fn sessions_handler(
    State(state): State<Arc<AuthAppState>>,
    Query(query): Query<AudienceQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Vec<SessionDescriptor>>, (StatusCode, Json<AuthErrorResponse>)> {
    let token = bearer_token(&headers)?;
    state
        .credential_security
        .list_sessions(
            &state.credential_tokens,
            &CredentialSecret::new(token),
            query.audience,
        )
        .map(Json)
        .map_err(credential_http_error)
}

async fn revoke_session_handler(
    State(state): State<Arc<AuthAppState>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<RevokeSessionRequest>,
) -> Result<StatusCode, (StatusCode, Json<AuthErrorResponse>)> {
    let token = bearer_token(&headers)?;
    let session_id = SessionId::parse(request.session_id)
        .map_err(|_| credential_http_error(CredentialFailure::Malformed))?;
    state
        .credential_security
        .revoke_session(
            &state.credential_tokens,
            &CredentialSecret::new(token),
            request.audience,
            &session_id,
        )
        .map_err(credential_http_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn whoami_handler(
    State(state): State<Arc<AuthAppState>>,
    Query(query): Query<AudienceQuery>,
    headers: axum::http::HeaderMap,
    principal: Option<Extension<AuthenticatedUser>>,
) -> Result<Json<WhoAmIResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    if let Some(Extension(authenticated)) = principal {
        return Ok(Json(whoami_response(&authenticated.principal)));
    }
    let token = bearer_token(&headers)?;
    let (claims, user) = state
        .credential_security
        .validate_access(
            &state.credential_tokens,
            &CredentialSecret::new(token),
            query.audience,
        )
        .map_err(credential_http_error)?;
    let principal = state
        .security_manager
        .resolve_user_principal(&user.username)
        .map_err(|_| credential_http_error(CredentialFailure::InvalidCredential))?;
    if principal.id.as_str() != claims.subject.as_str() {
        return Err(credential_http_error(CredentialFailure::InvalidCredential));
    }
    Ok(Json(whoami_response(&principal)))
}

fn whoami_response(principal: &zeta4j_credential::Principal) -> WhoAmIResponse {
    WhoAmIResponse {
        principal_id: principal.id.as_str().to_string(),
        principal_kind: serde_json::to_value(principal.kind)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string()),
        status: serde_json::to_value(principal.status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string()),
        security_generation: principal.security_generation,
    }
}

/// POST /auth/login - 로그인
///
/// Basic Auth 헤더 또는 JSON body로 인증
async fn login_handler(
    State(state): State<Arc<AuthAppState>>,
    headers: axum::http::HeaderMap,
    body: Option<Json<LoginRequest>>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    // Basic Auth 헤더 또는 JSON body에서 credentials 추출
    let auth_header_str = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok());

    let (username, password, audience) = if let Some(header) = auth_header_str {
        if is_basic_auth(header) {
            let creds = extract_basic_auth(Some(header)).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(AuthErrorResponse {
                        error: PublicError::AuthenticationFailed.code(),
                        message: "Authentication failed".to_string(),
                    }),
                )
            })?;
            (creds.username, creds.password, default_http_audience())
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AuthErrorResponse {
                    error: PublicError::AuthenticationFailed.code(),
                    message: "Authentication failed".to_string(),
                }),
            ));
        }
    } else if let Some(Json(req)) = body {
        (req.username, req.password, req.audience)
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AuthErrorResponse {
                error: PublicError::AuthenticationFailed.code(),
                message: "Authentication failed".to_string(),
            }),
        ));
    };

    let token_pair = state
        .credential_security
        .login(&state.credential_tokens, &username, &password, audience)
        .map_err(credential_http_error)?;

    Ok(Json(LoginResponse {
        access_token: token_pair.access_token().expose().to_string(),
        refresh_token: token_pair.refresh_token().expose().to_string(),
        token_type: "Bearer".to_string(),
        expires_in: token_pair
            .access_expires_at
            .saturating_sub(chrono::Utc::now().timestamp()) as u64,
        access_expires_at: token_pair.access_expires_at,
        refresh_expires_at: token_pair.refresh_expires_at,
        session_id: token_pair.session_id.as_str().to_string(),
    }))
}

/// Consume the initial credential without issuing a general-purpose token.
async fn bootstrap_complete_handler(
    State(state): State<Arc<AuthAppState>>,
    Json(request): Json<BootstrapCompleteRequest>,
) -> Result<StatusCode, (StatusCode, Json<AuthErrorResponse>)> {
    let result = if let Some(admin_service) = &state.admin_service {
        admin_service
            .complete_bootstrap(
                &request.username,
                &request.current_password,
                &request.new_password,
            )
            .await
    } else {
        state
            .security_manager
            .complete_bootstrap(
                &request.username,
                &request.current_password,
                &request.new_password,
            )
            .map(|_| ())
            .map_err(|error| match error {
                SystemError::BootstrapExpired => AdminServiceError::BootstrapExpired,
                SystemError::BootstrapConsumed => AdminServiceError::BootstrapConsumed,
                SystemError::InvalidPassword => AdminServiceError::InvalidPassword,
                SystemError::AuthenticationFailed(_) => AdminServiceError::InvalidCredential,
                other => AdminServiceError::Storage(other.to_string()),
            })
    };

    result.map(|_| StatusCode::NO_CONTENT).map_err(|error| {
        let (status, code, message) = match error {
            AdminServiceError::BootstrapExpired => (
                StatusCode::GONE,
                PublicError::PrincipalInactive.code(),
                "The one-time bootstrap credential has expired",
            ),
            AdminServiceError::BootstrapConsumed => (
                StatusCode::CONFLICT,
                PublicError::PrincipalInactive.code(),
                "Authentication failed",
            ),
            AdminServiceError::InvalidPassword => (
                StatusCode::BAD_REQUEST,
                PublicError::AuthenticationFailed.code(),
                "The new password does not meet policy",
            ),
            _ => (
                StatusCode::UNAUTHORIZED,
                PublicError::AuthenticationFailed.code(),
                "Authentication failed",
            ),
        };
        (
            status,
            Json(AuthErrorResponse {
                error: code,
                message: message.to_string(),
            }),
        )
    })
}

/// POST /auth/refresh - 토큰 갱신
async fn refresh_handler(
    State(state): State<Arc<AuthAppState>>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<AuthErrorResponse>)> {
    let token_pair = state
        .credential_security
        .refresh(
            &state.credential_tokens,
            &CredentialSecret::new(req.refresh_token),
            req.audience,
        )
        .map_err(credential_http_error)?;

    Ok(Json(LoginResponse {
        access_token: token_pair.access_token().expose().to_string(),
        refresh_token: token_pair.refresh_token().expose().to_string(),
        token_type: "Bearer".to_string(),
        expires_in: token_pair
            .access_expires_at
            .saturating_sub(chrono::Utc::now().timestamp()) as u64,
        access_expires_at: token_pair.access_expires_at,
        refresh_expires_at: token_pair.refresh_expires_at,
        session_id: token_pair.session_id.as_str().to_string(),
    }))
}

/// POST /auth/logout - 로그아웃
async fn logout_handler(
    State(state): State<Arc<AuthAppState>>,
    Query(query): Query<AudienceQuery>,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<AuthErrorResponse>)> {
    let token = bearer_token(&headers)?;
    state
        .credential_security
        .logout(
            &state.credential_tokens,
            &CredentialSecret::new(token),
            query.audience,
        )
        .map_err(credential_http_error)?;

    Ok(StatusCode::NO_CONTENT)
}

fn bearer_token(
    headers: &axum::http::HeaderMap,
) -> Result<&str, (StatusCode, Json<AuthErrorResponse>)> {
    let auth_header = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(AuthErrorResponse {
                    error: PublicError::AuthenticationFailed.code(),
                    message: "Authentication failed".to_string(),
                }),
            )
        })?;

    auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(AuthErrorResponse {
                error: PublicError::AuthenticationFailed.code(),
                message: "Authentication failed".to_string(),
            }),
        )
    })
}

fn credential_http_error(failure: CredentialFailure) -> (StatusCode, Json<AuthErrorResponse>) {
    let status = match failure {
        CredentialFailure::Throttled => StatusCode::TOO_MANY_REQUESTS,
        CredentialFailure::Unavailable | CredentialFailure::Internal => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        CredentialFailure::PasswordPolicy | CredentialFailure::Policy => StatusCode::BAD_REQUEST,
        _ => StatusCode::UNAUTHORIZED,
    };
    (
        status,
        Json(AuthErrorResponse {
            error: failure.code().to_string(),
            message: failure.public_message().to_string(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use zeta4j_credential::{Principal, PrincipalKind, PrincipalStatus, StableId};

    use super::whoami_response;

    #[test]
    fn whoami_projects_api_key_principal_without_bearer_claims()
    -> Result<(), Box<dyn std::error::Error>> {
        let principal = Principal {
            id: StableId::new("service.0123456789abcdef0123456789abcdef")?,
            kind: PrincipalKind::ApiKeyService,
            status: PrincipalStatus::Active,
            tenant_id: StableId::new("system")?,
            security_generation: 1,
        };

        let response = whoami_response(&principal);

        assert_eq!(
            response.principal_id,
            "service.0123456789abcdef0123456789abcdef"
        );
        assert_eq!(response.principal_kind, "api-key-service");
        assert_eq!(response.status, "active");
        assert_eq!(response.security_generation, 1);
        Ok(())
    }
}
