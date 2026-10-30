//! Authentication middleware for HTTP requests.
//!
//! Provides Tower Layer/Service middleware for authenticating HTTP requests
//! using Bearer tokens. Every active authenticated principal may use every
//! protected route; only the registered public routes skip authentication.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
};
use tower::{Layer, Service};
use zeta4j_credential::{CredentialAudience, CredentialSecret, Principal, PrincipalKind};

use crate::server::access::PublicError;
use crate::server::system::{LocalSecurityService, SecurityManager};

use super::{CredentialTokenService, error::AuthError};

/// Authenticated user information stored in request extensions.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    /// Username of the authenticated user.
    pub username: String,
    /// Canonical principal resolved from current persisted state.
    pub principal: Principal,
}

impl AuthenticatedUser {
    /// Creates a request identity from canonical persisted state.
    pub fn from_canonical(username: impl Into<String>, principal: Principal) -> Self {
        Self {
            username: username.into(),
            principal,
        }
    }
}

/// Configuration for the authentication layer.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Whether authentication is required (vs optional).
    pub required: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self { required: true }
    }
}

impl AuthConfig {
    /// Creates a new auth config with required authentication.
    pub fn required() -> Self {
        Self::default()
    }

    /// Creates a new auth config with optional authentication.
    pub fn optional() -> Self {
        Self { required: false }
    }

    /// Checks an exact registered public selector.
    pub fn is_public_route(&self, method: &Method, path: &str) -> bool {
        crate::server::http::is_public_http_surface(method, path)
    }
}

/// Tower Layer for authentication.
#[derive(Clone)]
pub struct AuthLayer {
    security_manager: Arc<SecurityManager>,
    credential_security: Option<Arc<LocalSecurityService>>,
    credential_tokens: Option<Arc<CredentialTokenService>>,
    config: AuthConfig,
}

impl AuthLayer {
    /// Creates a new authentication layer without a credential service.
    pub fn new(security_manager: Arc<SecurityManager>) -> Self {
        Self {
            security_manager,
            credential_security: None,
            credential_tokens: None,
            config: AuthConfig::default(),
        }
    }

    /// Creates a new authentication layer with canonical credential support.
    pub fn with_credentials(
        security_manager: Arc<SecurityManager>,
        credential_security: Arc<LocalSecurityService>,
        credential_tokens: Arc<CredentialTokenService>,
    ) -> Self {
        Self {
            security_manager,
            credential_security: Some(credential_security),
            credential_tokens: Some(credential_tokens),
            config: AuthConfig::default(),
        }
    }

    /// Creates a new authentication layer with custom config.
    pub fn with_config(mut self, config: AuthConfig) -> Self {
        self.config = config;
        self
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            security_manager: Arc::clone(&self.security_manager),
            credential_security: self.credential_security.clone(),
            credential_tokens: self.credential_tokens.clone(),
            config: self.config.clone(),
        }
    }
}

/// Tower Service for authentication.
#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    security_manager: Arc<SecurityManager>,
    credential_security: Option<Arc<LocalSecurityService>>,
    credential_tokens: Option<Arc<CredentialTokenService>>,
    config: AuthConfig,
}

impl<S> Service<Request<Body>> for AuthService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let path = req.uri().path().to_string();
        let method = req.method().clone();
        let config = self.config.clone();
        let security_manager = Arc::clone(&self.security_manager);
        let credential_security = self.credential_security.clone();
        let credential_tokens = self.credential_tokens.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            if config.is_public_route(&method, &path) {
                return inner.call(req).await;
            }

            // An earlier layer (API key authentication) may have installed the identity.
            let user = match req.extensions().get::<AuthenticatedUser>().cloned() {
                Some(user) => user,
                None => {
                    let auth_header = req
                        .headers()
                        .get(AUTHORIZATION)
                        .and_then(|v| v.to_str().ok());
                    match authenticate(
                        auth_header,
                        &security_manager,
                        credential_security.as_deref(),
                        credential_tokens.as_deref(),
                    )
                    .await
                    {
                        Ok(user) => user,
                        Err(err) if config.required => return Ok(auth_error_response(err)),
                        Err(_) => return inner.call(req).await,
                    }
                }
            };

            match admit_http_request(user, &security_manager) {
                Ok(user) => {
                    req.extensions_mut().insert(user);
                    inner.call(req).await
                }
                Err(error) => Ok(access_error_response(error)),
            }
        })
    }
}

/// Re-resolves user principals from current state and admits active principals.
fn admit_http_request(
    user: AuthenticatedUser,
    security_manager: &SecurityManager,
) -> Result<AuthenticatedUser, PublicError> {
    let current_user = if user.principal.kind == PrincipalKind::User {
        let stored = security_manager
            .get_user(&user.username)
            .map_err(|_| PublicError::PolicyInvariant)?
            .ok_or(PublicError::PrincipalInactive)?;
        let principal = security_manager
            .resolve_user_principal(&user.username)
            .map_err(|_| PublicError::PrincipalInactive)?;
        AuthenticatedUser::from_canonical(stored.username, principal)
    } else {
        user
    };
    if !current_user.principal.is_active() {
        return Err(PublicError::PrincipalInactive);
    }
    Ok(current_user)
}

fn public_error_status(error: PublicError) -> StatusCode {
    match error {
        PublicError::AuthenticationFailed
        | PublicError::TokenExpired
        | PublicError::PrincipalInactive => StatusCode::UNAUTHORIZED,
        PublicError::DatabaseNotFound => StatusCode::NOT_FOUND,
        PublicError::PolicyInvariant => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn access_error_response(error: PublicError) -> Response {
    let status = public_error_status(error);
    let body = serde_json::json!({
        "error": {
            "code": error.code(),
            "message": match status {
                StatusCode::UNAUTHORIZED => "Authentication failed",
                StatusCode::NOT_FOUND => "Database not found",
                _ => "Access check unavailable",
            }
        }
    });
    (status, axum::Json(body)).into_response()
}

/// Authenticates a request using the Authorization header.
async fn authenticate(
    auth_header: Option<&str>,
    security_manager: &Arc<SecurityManager>,
    credential_security: Option<&LocalSecurityService>,
    credential_tokens: Option<&CredentialTokenService>,
) -> Result<AuthenticatedUser, AuthError> {
    let header = auth_header.ok_or(AuthError::MissingAuthHeader)?;

    if header.starts_with("Bearer ") {
        authenticate_bearer(
            header,
            security_manager,
            credential_security,
            credential_tokens,
        )
        .await
    } else {
        Err(AuthError::InvalidAuthHeader(
            "Expected Bearer authentication".to_string(),
        ))
    }
}

/// Authenticates using a Bearer token (JWT).
async fn authenticate_bearer(
    header: &str,
    security_manager: &Arc<SecurityManager>,
    credential_security: Option<&LocalSecurityService>,
    credential_tokens: Option<&CredentialTokenService>,
) -> Result<AuthenticatedUser, AuthError> {
    let token = header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AuthError::InvalidAuthHeader("Invalid Bearer format".to_string()))?;

    let credential_security = credential_security
        .ok_or_else(|| AuthError::Internal("credential service unavailable".to_string()))?;
    let credential_tokens = credential_tokens
        .ok_or_else(|| AuthError::Internal("credential verifier unavailable".to_string()))?;
    let (_, user) = credential_security
        .validate_access(
            credential_tokens,
            &CredentialSecret::new(token),
            CredentialAudience::Http,
        )
        .map_err(AuthError::Credential)?;
    let principal = security_manager
        .resolve_user_principal(&user.username)
        .map_err(|_| AuthError::InvalidCredentials)?;
    Ok(AuthenticatedUser::from_canonical(user.username, principal))
}

/// Creates an error response for authentication failures.
fn auth_error_response(err: AuthError) -> Response {
    if let AuthError::Credential(failure) = &err {
        let failure = *failure;
        let status = StatusCode::from_u16(AuthError::Credential(failure).status_code())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = serde_json::json!({
            "error": {
                "code": failure.code(),
                "message": failure.public_message(),
            }
        });
        return (status, axum::Json(body)).into_response();
    }
    let public_error = match err {
        AuthError::TokenExpired => PublicError::TokenExpired,
        AuthError::UserDisabled(_) | AuthError::UserNotFound(_) => PublicError::PrincipalInactive,
        AuthError::HashingFailed(_) | AuthError::VerificationFailed(_) | AuthError::Internal(_) => {
            PublicError::PolicyInvariant
        }
        _ => PublicError::AuthenticationFailed,
    };
    let mut response = access_error_response(public_error);

    // Add WWW-Authenticate header for 401 responses
    if response.status() == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            "WWW-Authenticate",
            axum::http::HeaderValue::from_static("Bearer"),
        );
    }

    response
}
