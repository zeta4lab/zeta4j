// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Persisted service API-key authentication adapter.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
};
use tower::{Layer, Service};
use zeta4j_credential::{
    CredentialAudience, CredentialFailure, CredentialScope, CredentialSecret, Principal,
    PrincipalKind, PrincipalStatus, StableId,
};

use crate::server::system::LocalSecurityService;

use super::middleware::AuthenticatedUser;

pub const X_API_KEY_HEADER: &str = "x-api-key";

pub fn extract_api_key(headers: &HeaderMap) -> Option<String> {
    if let Some(value) = headers.get(X_API_KEY_HEADER)
        && let Ok(key) = value.to_str()
    {
        return Some(key.to_string());
    }
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("ApiKey "))
        .map(str::to_string)
}

#[derive(Clone)]
pub struct ApiKeyLayer {
    security: Arc<LocalSecurityService>,
}

impl ApiKeyLayer {
    pub fn new(security: Arc<LocalSecurityService>) -> Self {
        Self { security }
    }
}

impl<S> Layer<S> for ApiKeyLayer {
    type Service = ApiKeyService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ApiKeyService {
            inner,
            security: self.security.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ApiKeyService<S> {
    inner: S,
    security: Arc<LocalSecurityService>,
}

impl<S> Service<Request<Body>> for ApiKeyService<S>
where
    S: Service<Request<Body>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(context)
    }

    fn call(&mut self, mut request: Request<Body>) -> Self::Future {
        let path = request.uri().path().to_string();
        let method = request.method().clone();
        let security = self.security.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            if crate::server::http::is_public_http_surface(&method, &path) {
                return inner.call(request).await;
            }
            let Some(raw_key) = extract_api_key(request.headers()) else {
                return inner.call(request).await;
            };
            let required_scope = if path.starts_with("/admin/") {
                CredentialScope::Administration
            } else {
                CredentialScope::Api
            };
            let authenticated = tokio::task::spawn_blocking(move || {
                security.verify_api_key(
                    &CredentialSecret::new(raw_key),
                    CredentialAudience::Http,
                    required_scope,
                )
            })
            .await
            .map_err(|_| CredentialFailure::Internal)
            .and_then(|result| result);
            let authenticated = match authenticated {
                Ok(authenticated) => authenticated,
                Err(error) => return Ok(api_key_error_response(error)),
            };
            let principal_id = StableId::new(authenticated.principal.id.as_str());
            let tenant_id = StableId::new("system");
            let (Ok(principal_id), Ok(tenant_id)) = (principal_id, tenant_id) else {
                return Ok(api_key_error_response(CredentialFailure::Internal));
            };
            request
                .extensions_mut()
                .insert(AuthenticatedUser::from_canonical(
                    authenticated.principal.name,
                    Principal {
                        id: principal_id,
                        kind: PrincipalKind::ApiKeyService,
                        status: PrincipalStatus::Active,
                        tenant_id,
                        security_generation: authenticated.principal.security_generation,
                    },
                ));
            inner.call(request).await
        })
    }
}

fn api_key_error_response(error: CredentialFailure) -> Response {
    let status = match error {
        CredentialFailure::Throttled => StatusCode::TOO_MANY_REQUESTS,
        CredentialFailure::Internal | CredentialFailure::Unavailable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::UNAUTHORIZED,
    };
    let body = serde_json::json!({
        "error": {
            "code": error.code(),
            "message": error.public_message()
        }
    });
    (status, axum::Json(body)).into_response()
}
