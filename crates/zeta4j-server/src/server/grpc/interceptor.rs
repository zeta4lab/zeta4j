//! gRPC Authentication Interceptor
//!
//! HTTP와 동일한 canonical credential 인증을 gRPC에 적용합니다.
//! 인증된 활성 주체는 모든 RPC를 호출할 수 있습니다.
//!
//! # 지원 인증 방식
//!
//! - `Authorization: Bearer <access_token>` - durable session bearer 인증

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use http::{Request as HttpRequest, Response as HttpResponse};
use tonic::{Status, body::Body};
use tower::{Layer, Service};
use zeta4j_credential::{
    CredentialAudience, CredentialFailure, CredentialSecret, Principal, PrincipalKind,
    PrincipalStatus, StableId,
};

use super::super::access::PublicError;
use super::super::auth::{AuthenticatedUser, CredentialTokenService};
use super::super::system::{LocalSecurityService, SecurityManager};

/// Re-validates a streaming caller against current persisted security state.
#[derive(Clone)]
pub struct SecurityGenerationGuard {
    security_manager: Option<Arc<SecurityManager>>,
    username: String,
    principal: Principal,
}

impl SecurityGenerationGuard {
    fn new(security_manager: Option<Arc<SecurityManager>>, user: &AuthenticatedUser) -> Self {
        Self {
            security_manager,
            username: user.username.clone(),
            principal: user.principal.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), PublicError> {
        if self.principal.kind != PrincipalKind::User {
            return Ok(());
        }
        let manager = self
            .security_manager
            .as_ref()
            .ok_or(PublicError::PolicyInvariant)?;
        let current = manager
            .resolve_user_principal(&self.username)
            .map_err(|_| PublicError::PrincipalInactive)?;
        if current.status != PrincipalStatus::Active
            || current.id != self.principal.id
            || current.security_generation != self.principal.security_generation
        {
            return Err(PublicError::PrincipalInactive);
        }
        Ok(())
    }
}

/// gRPC 요청 인증기
#[derive(Clone)]
pub struct GrpcAuthInterceptor {
    credential_security: Option<Arc<LocalSecurityService>>,
    credential_tokens: Option<Arc<CredentialTokenService>>,
    /// 인증 필수 여부
    required: bool,
    security_manager: Option<Arc<SecurityManager>>,
}

impl std::fmt::Debug for GrpcAuthInterceptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcAuthInterceptor")
            .field("credential_security", &self.credential_security.is_some())
            .field("credential_tokens", &self.credential_tokens.is_some())
            .field("required", &self.required)
            .field("security_manager", &self.security_manager.is_some())
            .finish()
    }
}

impl GrpcAuthInterceptor {
    /// Canonical credential interceptor.
    pub fn with_credentials(
        credential_security: Arc<LocalSecurityService>,
        credential_tokens: Arc<CredentialTokenService>,
    ) -> Self {
        Self {
            credential_security: Some(credential_security),
            credential_tokens: Some(credential_tokens),
            required: true,
            security_manager: None,
        }
    }

    /// Uses current persisted user state for every JWT-authenticated RPC.
    pub fn with_security_manager(mut self, manager: Arc<SecurityManager>) -> Self {
        self.security_manager = Some(manager);
        self
    }

    /// 인증 필수 여부 설정
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// 인증 비활성화 (no-auth 모드)
    pub fn disabled() -> Self {
        Self {
            credential_security: None,
            credential_tokens: None,
            required: false,
            security_manager: None,
        }
    }

    /// Bearer 토큰 인증
    fn authenticate_bearer(&self, header: &str) -> Result<AuthenticatedUser, Status> {
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| access_status(PublicError::AuthenticationFailed))?;

        let Some(credential_security) = self.credential_security.as_ref() else {
            tracing::warn!("gRPC bearer credential did not match an accepted credential class");
            return Err(access_status(PublicError::AuthenticationFailed));
        };
        let credential_tokens = self
            .credential_tokens
            .as_ref()
            .ok_or_else(|| access_status(PublicError::PolicyInvariant))?;
        let (_, user) = credential_security
            .validate_access(
                credential_tokens,
                &CredentialSecret::new(token),
                CredentialAudience::Grpc,
            )
            .map_err(|failure| {
                let error = match failure {
                    CredentialFailure::Internal | CredentialFailure::Unavailable => {
                        PublicError::PolicyInvariant
                    }
                    CredentialFailure::PrincipalChanged | CredentialFailure::InactiveSession => {
                        PublicError::PrincipalInactive
                    }
                    CredentialFailure::Expired => PublicError::TokenExpired,
                    _ => PublicError::AuthenticationFailed,
                };
                access_status(error)
            })?;

        let security_manager = self
            .security_manager
            .as_ref()
            .ok_or_else(|| access_status(PublicError::PolicyInvariant))?;
        let principal = security_manager
            .resolve_user_principal(&user.username)
            .map_err(|error| {
                tracing::warn!(
                    error = %error,
                    "gRPC principal is no longer resolvable from current security state"
                );
                access_status(PublicError::PrincipalInactive)
            })?;
        if principal.status != PrincipalStatus::Active {
            return Err(access_status(PublicError::PrincipalInactive));
        }
        Ok(AuthenticatedUser::from_canonical(user.username, principal))
    }

    /// Authenticates one RPC and installs the caller identity into the request extensions.
    fn admit_call(
        &self,
        headers: &http::HeaderMap,
        extensions: &mut http::Extensions,
    ) -> Result<(), Status> {
        let auth_header = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok());
        let user = match auth_header {
            Some(header) if header.starts_with("Bearer ") => self.authenticate_bearer(header)?,
            _ if self.required => {
                tracing::warn!(
                    authorization_header_present = auth_header.is_some(),
                    "required gRPC bearer credential is missing or malformed"
                );
                return Err(access_status(PublicError::AuthenticationFailed));
            }
            _ => local_test_anonymous()?,
        };
        let generation_guard = SecurityGenerationGuard::new(self.security_manager.clone(), &user);
        extensions.insert(generation_guard);
        extensions.insert(user);
        Ok(())
    }
}

fn local_test_anonymous() -> Result<AuthenticatedUser, Status> {
    let principal = Principal {
        id: StableId::new("local-test-anonymous")
            .map_err(|_| access_status(PublicError::PolicyInvariant))?,
        kind: PrincipalKind::LocalTestAnonymous,
        status: PrincipalStatus::Active,
        tenant_id: StableId::new("system")
            .map_err(|_| access_status(PublicError::PolicyInvariant))?,
        security_generation: 1,
    };
    Ok(AuthenticatedUser::from_canonical(
        "local-test-anonymous",
        principal,
    ))
}

/// Authentication layer installed outside Tonic's generated router so every
/// RPC of the protected service is authenticated before dispatch.
#[derive(Clone)]
pub struct GrpcAuthLayer {
    interceptor: Arc<GrpcAuthInterceptor>,
}

impl GrpcAuthLayer {
    pub fn new(interceptor: GrpcAuthInterceptor) -> Self {
        Self {
            interceptor: Arc::new(interceptor),
        }
    }

    pub fn protect<S>(&self, inner: S) -> GrpcAuthService<S> {
        self.layer(inner)
    }
}

impl<S> Layer<S> for GrpcAuthLayer {
    type Service = GrpcAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        GrpcAuthService {
            inner,
            interceptor: Arc::clone(&self.interceptor),
        }
    }
}

impl<S> tonic::server::NamedService for GrpcAuthService<S>
where
    S: tonic::server::NamedService,
{
    const NAME: &'static str = S::NAME;
}

#[derive(Clone)]
pub struct GrpcAuthService<S> {
    inner: S,
    interceptor: Arc<GrpcAuthInterceptor>,
}

impl<S, RequestBody> Service<HttpRequest<RequestBody>> for GrpcAuthService<S>
where
    S: Service<HttpRequest<RequestBody>, Response = HttpResponse<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    RequestBody: Send + 'static,
{
    type Response = HttpResponse<Body>;
    type Error = S::Error;
    type Future =
        Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, mut request: HttpRequest<RequestBody>) -> Self::Future {
        let headers = request.headers().clone();
        if let Err(status) = self
            .interceptor
            .admit_call(&headers, request.extensions_mut())
        {
            return Box::pin(async move { Ok(status.into_http::<Body>()) });
        }
        Box::pin(self.inner.call(request))
    }
}

pub fn access_status(error: PublicError) -> Status {
    let mut status = match error {
        PublicError::AuthenticationFailed
        | PublicError::TokenExpired
        | PublicError::PrincipalInactive => Status::unauthenticated("Authentication failed"),
        PublicError::DatabaseNotFound => Status::not_found("Database not found"),
        PublicError::PolicyInvariant => Status::internal("Access check unavailable"),
    };
    if let Ok(value) = tonic::metadata::MetadataValue::try_from(error.code()) {
        status.metadata_mut().insert("zeta-error-code", value);
    }
    status
}

#[cfg(test)]
mod tests {
    use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
    use zeta4j_credential::KeyVersion;

    use super::*;

    fn security() -> Result<(tempfile::TempDir, Arc<SecurityManager>), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let storage = Arc::new(crate::server::system::SystemStorage::open(temp.path())?);
        Ok((temp, Arc::new(SecurityManager::new(storage))))
    }

    #[test]
    fn missing_bearer_is_rejected_when_authentication_is_required()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_temp, security) = security()?;
        let interceptor = GrpcAuthInterceptor {
            credential_security: None,
            credential_tokens: None,
            required: true,
            security_manager: Some(security),
        };
        let error = interceptor
            .admit_call(&http::HeaderMap::new(), &mut http::Extensions::new())
            .err()
            .ok_or("unauthenticated RPC was admitted")?;
        assert_eq!(error.code(), tonic::Code::Unauthenticated);
        Ok(())
    }

    #[test]
    fn streaming_generation_guard_rejects_suspended_or_changed_user()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_temp, security) = security()?;
        security.create_user("stream-user", "StreamPassword!234")?;
        let principal = security.resolve_user_principal("stream-user")?;
        let user = AuthenticatedUser::from_canonical("stream-user", principal);
        let guard = SecurityGenerationGuard::new(Some(security.clone()), &user);
        assert!(guard.validate().is_ok());

        security.set_suspended("stream-user", true)?;
        assert_eq!(guard.validate(), Err(PublicError::PrincipalInactive));
        Ok(())
    }

    #[test]
    fn bearer_user_is_resolved_from_current_security_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let (temp, security) = security()?;
        security.create_user("grpc-user", "GrpcPassword!234")?;
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| std::io::Error::other("failed to generate signer"))?;
        let signer_path = temp.path().join("grpc-signer.pk8");
        std::fs::write(&signer_path, document.as_ref())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&signer_path, std::fs::Permissions::from_mode(0o600))?;
        }
        let reference = crate::server::auth::SignerReference::new(
            signer_path,
            "grpc-key-1",
            KeyVersion::new(1)?,
        )?;
        let (credential_security, credential_tokens) =
            crate::server::system::initialize_local_credential_runtime(
                security.clone(),
                &reference,
                crate::server::system::CredentialRuntimePolicy::default(),
                "base",
            )?;
        let token_pair = credential_security.login(
            &credential_tokens,
            "grpc-user",
            "GrpcPassword!234",
            CredentialAudience::Grpc,
        )?;
        let interceptor =
            GrpcAuthInterceptor::with_credentials(credential_security, credential_tokens)
                .with_security_manager(security.clone());
        let mut headers = http::HeaderMap::new();
        headers.insert(
            "authorization",
            http::HeaderValue::try_from(format!("Bearer {}", token_pair.access_token().expose()))?,
        );

        let mut extensions = http::Extensions::new();
        interceptor.admit_call(&headers, &mut extensions)?;
        let user = extensions
            .get::<AuthenticatedUser>()
            .ok_or_else(|| std::io::Error::other("authenticated user is missing"))?;
        assert_eq!(user.username, "grpc-user");
        assert_eq!(user.principal.kind, PrincipalKind::User);

        security.set_suspended("grpc-user", true)?;
        let error = interceptor
            .admit_call(&headers, &mut http::Extensions::new())
            .err()
            .ok_or_else(|| std::io::Error::other("suspended user's JWT was accepted"))?;
        // Suspension closes the session, so the token itself no longer validates.
        assert_eq!(error.code(), tonic::Code::Unauthenticated);
        Ok(())
    }
}
