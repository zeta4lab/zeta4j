// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! gRPC Client Configuration
//!
//! gRPC 연결 설정 및 에러 타입

use std::sync::Arc;

use tonic::{Request, Status, metadata::MetadataValue, service::Interceptor, transport::Channel};
use zeta4j_transport_security::{TonicConnector, client_tls_config_from_environment};

use crate::credential::{
    CredentialEnvelopeError, default_credential_path, load_or_refresh_credential_envelope,
};

#[derive(Clone)]
pub(crate) struct BearerInterceptor {
    token: Option<Arc<str>>,
}

impl BearerInterceptor {
    pub(crate) fn new(token: Option<Arc<str>>) -> Self {
        Self { token }
    }
}

impl Interceptor for BearerInterceptor {
    fn call(&mut self, mut request: Request<()>) -> Result<Request<()>, Status> {
        if let Some(token) = &self.token {
            let value = MetadataValue::try_from(format!("Bearer {token}"))
                .map_err(|_| Status::internal("invalid authentication metadata"))?;
            request.metadata_mut().insert("authorization", value);
        }
        Ok(request)
    }
}

/// gRPC 연결 설정
#[derive(Clone)]
pub struct GrpcClientConfig {
    /// 서버 주소 (host:port)
    pub address: String,
    /// 연결 타임아웃 (밀리초)
    pub connect_timeout_ms: u64,
    /// 요청 타임아웃 (밀리초)
    pub request_timeout_ms: u64,
    /// Bearer credential attached to authenticated service requests.
    pub(crate) bearer_token: Option<Arc<str>>,
    credential_error: Option<Arc<str>>,
    /// Closed transport connector and explicit verification name.
    transport: Option<(TonicConnector, Arc<str>)>,
}

impl std::fmt::Debug for GrpcClientConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GrpcClientConfig")
            .field("address", &self.address)
            .field("connect_timeout_ms", &self.connect_timeout_ms)
            .field("request_timeout_ms", &self.request_timeout_ms)
            .field(
                "bearer_token",
                &self.bearer_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "credential_error",
                &self.credential_error.as_ref().map(|_| "invalid"),
            )
            .field("transport", &self.transport.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl Default for GrpcClientConfig {
    fn default() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .unwrap_or(i64::MAX);
        let (bearer_token, credential_error) = match default_credential_path()
            .and_then(|path| load_or_refresh_credential_envelope(&path, now))
        {
            Ok(envelope) if envelope.audience == "grpc" => {
                (Some(Arc::from(envelope.access_token())), None)
            }
            Ok(_) => (
                None,
                Some(Arc::from("credential envelope audience is not gRPC")),
            ),
            Err(CredentialEnvelopeError::Unavailable) => (None, None),
            Err(error) => (None, Some(Arc::from(error.to_string()))),
        };
        Self {
            address: "127.0.0.1:9046".to_string(),
            connect_timeout_ms: 5000,
            request_timeout_ms: 30000,
            bearer_token,
            credential_error,
            transport: None,
        }
    }
}

impl GrpcClientConfig {
    /// 새 설정 생성
    pub fn new(address: impl Into<String>) -> Self {
        let address = address.into();
        let mut config = Self {
            address: address.clone(),
            ..Self::default()
        };
        match connector_from_environment(&address) {
            Ok(transport) => config.transport = Some(transport),
            Err(error) => config.credential_error = Some(Arc::from(error)),
        }
        config
    }

    /// 연결 타임아웃 설정
    pub fn with_connect_timeout(mut self, ms: u64) -> Self {
        self.connect_timeout_ms = ms;
        self
    }

    /// 요청 타임아웃 설정
    pub fn with_request_timeout(mut self, ms: u64) -> Self {
        self.request_timeout_ms = ms;
        self
    }

    /// Attach a preflight-validated connector and explicit server name.
    pub fn with_transport(
        mut self,
        connector: TonicConnector,
        server_name: impl Into<String>,
    ) -> Self {
        self.transport = Some((connector, Arc::from(server_name.into())));
        self.credential_error = None;
        self
    }

    /// gRPC 채널 생성
    pub async fn connect(&self) -> Result<Channel, GrpcClientError> {
        if let Some(error) = &self.credential_error {
            return Err(GrpcClientError::InvalidAuthenticationMetadata(
                error.to_string(),
            ));
        }
        let address = self
            .address
            .strip_prefix("http://")
            .or_else(|| self.address.strip_prefix("https://"))
            .unwrap_or(&self.address);
        let (connector, server_name) = self.transport.as_ref().ok_or_else(|| {
            GrpcClientError::TlsConfiguration(
                "SV-TLS-001: an explicit transport connector is required".to_string(),
            )
        })?;
        connector
            .connect(
                address,
                server_name,
                std::time::Duration::from_millis(self.connect_timeout_ms),
                std::time::Duration::from_millis(self.request_timeout_ms),
            )
            .await
            .map_err(|error| GrpcClientError::ConnectionFailed(error.to_string()))
    }

    /// 특정 주소로 gRPC 채널 생성
    pub async fn connect_to(
        address: &str,
        connect_timeout_ms: u64,
        request_timeout_ms: u64,
    ) -> Result<Channel, GrpcClientError> {
        Self::new(address)
            .with_connect_timeout(connect_timeout_ms)
            .with_request_timeout(request_timeout_ms)
            .connect()
            .await
    }
}

fn connector_from_environment(address: &str) -> Result<(TonicConnector, Arc<str>), String> {
    let authority = address
        .strip_prefix("http://")
        .or_else(|| address.strip_prefix("https://"))
        .unwrap_or(address)
        .split('/')
        .next()
        .unwrap_or(address);
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host)
        .trim_matches(['[', ']']);
    if !address.starts_with("https://") {
        return Ok((TonicConnector::plaintext(), Arc::from(host.to_string())));
    }

    let (config, server_name) =
        client_tls_config_from_environment(&[b"h2"], host).map_err(|error| error.to_string())?;
    Ok((
        TonicConnector::tls(Arc::new(config)),
        Arc::from(server_name),
    ))
}

/// gRPC 클라이언트 에러
#[derive(Debug, thiserror::Error)]
pub enum GrpcClientError {
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("TLS configuration failed: {0}")]
    TlsConfiguration(String),

    #[error("Invalid authentication metadata: {0}")]
    InvalidAuthenticationMetadata(String),

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("server does not support the requested capability: {0}")]
    UnsupportedCapability(String),

    #[error("server capability is currently unavailable: {0}")]
    CapabilityUnavailable(String),
}

impl From<tonic::Status> for GrpcClientError {
    fn from(status: tonic::Status) -> Self {
        let stable_code = status
            .metadata()
            .get("zeta-error-code")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let detail = if status.message().is_empty() {
            "the RPC is not implemented by the target edition".to_owned()
        } else {
            status.message().to_owned()
        };
        let detail = stable_code.map_or(detail.clone(), |code| format!("{code}: {detail}"));
        match status.code() {
            tonic::Code::Unimplemented => GrpcClientError::UnsupportedCapability(detail),
            tonic::Code::FailedPrecondition => GrpcClientError::CapabilityUnavailable(detail),
            _ => GrpcClientError::ServerError(detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_scheme_reaches_remote_hosts() -> Result<(), String> {
        let (_connector, server_name) = connector_from_environment("http://192.0.2.10:9046")?;
        assert_eq!(&*server_name, "192.0.2.10");
        Ok(())
    }

    #[test]
    fn debug_redacts_bearer_and_tls_material() {
        let mut config = GrpcClientConfig::new("db.example:9046")
            .with_transport(TonicConnector::plaintext(), "localhost");
        config.bearer_token = Some(Arc::from("management-secret-value"));

        let debug = format!("{config:?}");

        assert!(!debug.contains("management-secret-value"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn bearer_interceptor_authenticates_every_channel_request() -> Result<(), Status> {
        let mut interceptor = BearerInterceptor::new(Some(Arc::from("purpose-derived-credential")));
        let request = interceptor.call(Request::new(()))?;

        assert_eq!(
            request
                .metadata()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer purpose-derived-credential")
        );
        Ok(())
    }

    #[test]
    fn server_authorization_code_is_preserved_for_cli() -> Result<(), Status> {
        let mut status = Status::permission_denied("permission denied");
        status.metadata_mut().insert(
            "zeta-error-code",
            MetadataValue::try_from("SV-AUTH-005")
                .map_err(|_| Status::internal("invalid test metadata"))?,
        );

        let error = GrpcClientError::from(status);

        assert_eq!(
            error.to_string(),
            "Server error: SV-AUTH-005: permission denied"
        );
        Ok(())
    }
}
