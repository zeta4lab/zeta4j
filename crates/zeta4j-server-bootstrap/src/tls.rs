//! Runtime adapters for the preflight-validated transport generation.

use std::sync::Arc;
use zeta4j_transport_security::TransportMode;

use super::ValidatedStartupConfig;

/// Protocol-specific adapters derived from one validated generation.
pub struct RuntimeTlsConfig {
    pub http: Arc<rustls::ServerConfig>,
    pub grpc: Arc<rustls::ServerConfig>,
}

/// Adapts validated transport material without reopening any file.
pub struct TlsConfigBuilder<'a> {
    config: &'a ValidatedStartupConfig,
}

impl<'a> TlsConfigBuilder<'a> {
    pub fn new(config: &'a ValidatedStartupConfig) -> Self {
        Self { config }
    }

    pub fn load(&self) -> Option<RuntimeTlsConfig> {
        if self.config.transport().mode() == TransportMode::Plaintext {
            return None;
        }
        let server = self.config.transport().http_server()?;
        let grpc = self.config.transport().grpc_server()?;
        Some(RuntimeTlsConfig { http: server, grpc })
    }
}
