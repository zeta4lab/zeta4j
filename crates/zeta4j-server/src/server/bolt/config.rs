//! Bolt server configuration.

use std::net::SocketAddr;
use std::time::Duration;

use super::handshake::BoltVersion;
use zeta4j_common::defaults::{
    DEFAULT_BOLT_MAX_CONNECTIONS, DEFAULT_BOLT_PORT, DEFAULT_BOLT_TIMEOUT_SECS,
    DEFAULT_DATABASE_NAME,
};

/// Bolt server configuration.
#[derive(Debug, Clone)]
pub struct BoltConfig {
    /// Address to bind the Bolt server
    pub bind_addr: SocketAddr,

    /// Maximum number of concurrent connections
    pub max_connections: usize,

    /// Connection timeout duration
    pub connection_timeout: Duration,

    /// Supported Bolt protocol versions
    pub supported_versions: Vec<BoltVersion>,

    /// Authentication required
    pub auth_required: bool,

    /// Default database name
    pub default_database: String,
}

impl Default for BoltConfig {
    fn default() -> Self {
        Self {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_BOLT_PORT)),
            max_connections: DEFAULT_BOLT_MAX_CONNECTIONS,
            connection_timeout: Duration::from_secs(DEFAULT_BOLT_TIMEOUT_SECS),
            supported_versions: vec![
                BoltVersion::V5_0,
                BoltVersion::V4_4,
                BoltVersion::V4_3,
                BoltVersion::V4_2,
                BoltVersion::V4_1,
                BoltVersion::V4_0,
            ],
            auth_required: true,
            default_database: DEFAULT_DATABASE_NAME.to_string(),
        }
    }
}

impl BoltConfig {
    /// Create a new BoltConfig with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a builder for BoltConfig.
    pub fn builder() -> BoltConfigBuilder {
        BoltConfigBuilder::new()
    }
}

/// Builder for BoltConfig.
#[derive(Debug, Default)]
pub struct BoltConfigBuilder {
    config: BoltConfig,
}

impl BoltConfigBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            config: BoltConfig::default(),
        }
    }

    /// Set the bind address.
    pub fn bind_addr(mut self, addr: SocketAddr) -> Self {
        self.config.bind_addr = addr;
        self
    }

    /// Set the port (uses 0.0.0.0 as host).
    pub fn port(mut self, port: u16) -> Self {
        self.config.bind_addr = SocketAddr::from(([0, 0, 0, 0], port));
        self
    }

    /// Set maximum connections.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max;
        self
    }

    /// Set connection timeout.
    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.config.connection_timeout = timeout;
        self
    }

    /// Set supported versions.
    pub fn supported_versions(mut self, versions: Vec<BoltVersion>) -> Self {
        self.config.supported_versions = versions;
        self
    }

    /// Require authentication.
    pub fn auth_required(mut self, required: bool) -> Self {
        self.config.auth_required = required;
        self
    }

    /// Set default database.
    pub fn default_database(mut self, name: impl Into<String>) -> Self {
        self.config.default_database = name.into();
        self
    }

    /// Build the configuration.
    pub fn build(self) -> BoltConfig {
        self.config
    }
}
