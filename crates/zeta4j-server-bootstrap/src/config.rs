// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Configuration merging utilities
//!
//! CLI 인자와 설정 파일을 병합합니다.
//! CLI 인자가 명시적으로 지정된 경우 설정 파일보다 우선합니다.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use log::info;
use serde::Serialize;

use zeta4j_server::config::{ConfigError, ConfigRegistry};
use zeta4j_server::server::http::CorsConfig;

use super::defaults::*;
use zeta4j_common::{Edition, RuntimeProfile};

/// Effective configuration after merging CLI args and config files
pub struct EffectiveConfig {
    // Runtime security
    pub runtime_profile: RuntimeProfile,

    // Network
    pub http_host: String,
    pub bolt_host: String,
    pub grpc_host: String,
    pub http_port: u16,
    pub bolt_port: u16,
    pub grpc_port: u16,

    // HTTP
    pub compression: bool,
    pub timeout_secs: u64,
    pub max_connections: usize,
    pub max_body_size: usize,
    pub keep_alive_timeout_secs: u64,
    pub cors: CorsConfig,

    // Bolt
    pub bolt_max_connections: usize,
    pub bolt_timeout_secs: u64,
    pub bolt_default_database: String,

    // Auth
    pub auth_enabled: bool,
    pub jwt_signing_key_file: PathBuf,
    pub jwt_key_id: String,
    pub jwt_key_version: u64,
    pub access_token_ttl_secs: u64,
    pub refresh_token_ttl_secs: u64,

    // Database
    pub max_databases: usize,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,

    // Logging
    pub log_level: String,
    pub log_format: String,
    pub log_dir: PathBuf,
    pub log_rotation_size_mb: u64,
    pub log_rotation_count: u32,

    // Query
    pub max_result_size: usize,
    pub query_timeout_ms: u64,

    // Transport references
    pub transport_mode: String,
    pub server_cert_ref: String,
    pub server_key_ref: String,

    // Other
    pub timezone: String,
    pub replace: bool,
}

impl std::fmt::Debug for EffectiveConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EffectiveConfig")
            .field("runtime_profile", &self.runtime_profile)
            .field("http_host", &self.http_host)
            .field("bolt_host", &self.bolt_host)
            .field("grpc_host", &self.grpc_host)
            .field("http_port", &self.http_port)
            .field("bolt_port", &self.bolt_port)
            .field("grpc_port", &self.grpc_port)
            .field("compression", &self.compression)
            .field("timeout_secs", &self.timeout_secs)
            .field("max_connections", &self.max_connections)
            .field("max_body_size", &self.max_body_size)
            .field("keep_alive_timeout_secs", &self.keep_alive_timeout_secs)
            .field("cors", &self.cors)
            .field("bolt_max_connections", &self.bolt_max_connections)
            .field("bolt_timeout_secs", &self.bolt_timeout_secs)
            .field("bolt_default_database", &self.bolt_default_database)
            .field("auth_enabled", &self.auth_enabled)
            .field("jwt_signing_key_file", &"[REDACTED REFERENCE]")
            .field("jwt_key_id", &self.jwt_key_id)
            .field("jwt_key_version", &self.jwt_key_version)
            .field("access_token_ttl_secs", &self.access_token_ttl_secs)
            .field("refresh_token_ttl_secs", &self.refresh_token_ttl_secs)
            .field("max_databases", &self.max_databases)
            .field("data_dir", &self.data_dir)
            .field("config_dir", &self.config_dir)
            .field("log_level", &self.log_level)
            .field("log_format", &self.log_format)
            .field("log_dir", &self.log_dir)
            .field("log_rotation_size_mb", &self.log_rotation_size_mb)
            .field("log_rotation_count", &self.log_rotation_count)
            .field("max_result_size", &self.max_result_size)
            .field("query_timeout_ms", &self.query_timeout_ms)
            .field("transport_mode", &self.transport_mode)
            .field("server_cert_ref", &"[REDACTED REFERENCE]")
            .field("server_key_ref", &"[REDACTED REFERENCE]")
            .field("timezone", &self.timezone)
            .field("replace", &self.replace)
            .finish()
    }
}

impl Default for EffectiveConfig {
    fn default() -> Self {
        Self {
            runtime_profile: RuntimeProfile::Production,
            http_host: DEFAULT_HOST.to_string(),
            bolt_host: DEFAULT_HOST.to_string(),
            grpc_host: DEFAULT_HOST.to_string(),
            http_port: DEFAULT_HTTP_PORT,
            bolt_port: DEFAULT_BOLT_PORT,
            grpc_port: DEFAULT_GRPC_PORT,
            compression: true,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            max_body_size: DEFAULT_MAX_BODY_SIZE,
            keep_alive_timeout_secs: DEFAULT_KEEP_ALIVE_TIMEOUT_SECS,
            cors: CorsConfig::default(),
            bolt_max_connections: DEFAULT_BOLT_MAX_CONNECTIONS,
            bolt_timeout_secs: DEFAULT_BOLT_TIMEOUT_SECS,
            bolt_default_database: DEFAULT_DATABASE_NAME.to_string(),
            auth_enabled: true,
            jwt_signing_key_file: std::env::var_os("ZETA4J_JWT_SIGNING_KEY_FILE")
                .map(PathBuf::from)
                .unwrap_or_default(),
            jwt_key_id: std::env::var("ZETA4J_JWT_KEY_ID").unwrap_or_else(|_| "key-1".to_string()),
            jwt_key_version: std::env::var("ZETA4J_JWT_KEY_VERSION")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(1),
            access_token_ttl_secs: DEFAULT_ACCESS_TOKEN_TTL_SECS,
            refresh_token_ttl_secs: DEFAULT_REFRESH_TOKEN_TTL_SECS,
            max_databases: DEFAULT_MAX_DATABASES,
            data_dir: default_data_dir(),
            config_dir: default_config_dir(),
            log_level: DEFAULT_LOG_LEVEL.to_string(),
            log_format: DEFAULT_LOG_FORMAT.to_string(),
            log_dir: default_log_dir(),
            log_rotation_size_mb: DEFAULT_LOG_ROTATION_SIZE_MB,
            log_rotation_count: DEFAULT_LOG_ROTATION_COUNT,
            max_result_size: DEFAULT_MAX_RESULT_SIZE,
            query_timeout_ms: DEFAULT_QUERY_TIMEOUT_MS,
            transport_mode: "plaintext".to_string(),
            server_cert_ref: String::new(),
            server_key_ref: String::new(),
            timezone: "local".to_string(),
            replace: false,
        }
    }
}

impl EffectiveConfig {
    /// Whether product listeners require TLS.
    pub fn tls_enabled(&self) -> bool {
        self.transport_mode == "tls"
    }

    /// Get HTTP bind address
    pub fn http_addr(&self) -> String {
        format!("{}:{}", self.http_host, self.http_port)
    }

    /// Get Bolt bind address
    pub fn bolt_addr(&self) -> String {
        format!("{}:{}", self.bolt_host, self.bolt_port)
    }

    /// Get gRPC bind address
    pub fn grpc_addr(&self) -> String {
        format!("{}:{}", self.grpc_host, self.grpc_port)
    }

    /// Get request timeout as Duration
    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    /// Get Bolt timeout as Duration
    pub fn bolt_timeout(&self) -> Duration {
        Duration::from_secs(self.bolt_timeout_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use zeta4j_common::{DEFAULT_CORE_TOML, DEFAULT_SERVER_TOML};

    #[derive(Default)]
    struct TestArgs {
        runtime_profile: Option<RuntimeProfile>,
        host: Option<String>,
        http_port: Option<u16>,
        bolt_port: Option<u16>,
        grpc_port: Option<u16>,
        compression: Option<bool>,
        timeout: Option<u64>,
        log_level: Option<String>,
        log_dir: Option<PathBuf>,
        data_dir: Option<PathBuf>,
        config_dir: Option<PathBuf>,
        timezone: Option<String>,
        replace: bool,
        no_auth: bool,
    }

    impl CommonArgs for TestArgs {
        fn runtime_profile(&self) -> Option<RuntimeProfile> {
            self.runtime_profile
        }
        fn host(&self) -> Option<&str> {
            self.host.as_deref()
        }
        fn http_port(&self) -> Option<u16> {
            self.http_port
        }
        fn bolt_port(&self) -> Option<u16> {
            self.bolt_port
        }
        fn grpc_port(&self) -> Option<u16> {
            self.grpc_port
        }
        fn compression(&self) -> Option<bool> {
            self.compression
        }
        fn timeout(&self) -> Option<u64> {
            self.timeout
        }
        fn log_level(&self) -> Option<&str> {
            self.log_level.as_deref()
        }
        fn log_dir(&self) -> Option<&PathBuf> {
            self.log_dir.as_ref()
        }
        fn data_dir(&self) -> Option<&PathBuf> {
            self.data_dir.as_ref()
        }
        fn config_dir(&self) -> Option<&PathBuf> {
            self.config_dir.as_ref()
        }
        fn timezone(&self) -> Option<&str> {
            self.timezone.as_deref()
        }
        fn replace(&self) -> bool {
            self.replace
        }
        fn no_auth(&self) -> bool {
            self.no_auth
        }
    }

    fn config_directory(server: &str, core: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::write(directory.path().join("core.toml"), core)?;
        fs::write(directory.path().join("server.toml"), server)?;
        Ok(directory)
    }

    #[test]
    fn debug_redacts_all_authentication_credentials() {
        let config = EffectiveConfig {
            jwt_signing_key_file: PathBuf::from("/secret/signer.pk8"),
            ..EffectiveConfig::default()
        };

        let debug = format!("{config:?}");

        assert!(!debug.contains("/secret/signer.pk8"));
        assert!(!debug.contains("management-secret-value"));
        assert!(debug.contains("[REDACTED REFERENCE]"));
    }

    #[test]
    fn config_values_win_when_cli_sources_are_absent() -> Result<(), Box<dyn std::error::Error>> {
        let server = DEFAULT_SERVER_TOML
            .replace("port = 9044", "port = 19044")
            .replace("port = 9046", "port = 19046")
            .replace("compression = true", "compression = false")
            .replace("request_timeout_secs = 30", "request_timeout_secs = 91")
            .replace("timezone = \"local\"", "timezone = \"utc\"");
        let core = DEFAULT_CORE_TOML
            .replace("level = \"info\"", "level = \"debug\"")
            .replace(
                "data_dir = \"~/.zeta4j/databases\"",
                "data_dir = \"/tmp/zeta4j-config-data\"",
            );
        let directory = config_directory(&server, &core)?;
        let args = TestArgs {
            config_dir: Some(directory.path().to_path_buf()),
            ..Default::default()
        };

        let merger = ConfigMerger::new(&args, Edition::Base)?;
        let config = merger.merge();
        assert_eq!(config.http_port, 19044);
        assert_eq!(config.grpc_port, 19046);
        assert!(!config.compression);
        assert_eq!(config.timeout_secs, 91);
        assert_eq!(config.timezone, "utc");
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.data_dir, PathBuf::from("/tmp/zeta4j-config-data"));
        assert!(merger.config_fingerprint().is_some());
        Ok(())
    }

    #[test]
    fn explicit_values_win_even_when_equal_to_public_defaults()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = DEFAULT_SERVER_TOML
            .replace("port = 9044", "port = 19044")
            .replace("port = 9046", "port = 19046")
            .replace("compression = true", "compression = false")
            .replace("request_timeout_secs = 30", "request_timeout_secs = 91")
            .replace("timezone = \"local\"", "timezone = \"utc\"");
        let core = DEFAULT_CORE_TOML
            .replace("level = \"info\"", "level = \"debug\"")
            .replace(
                "data_dir = \"~/.zeta4j/databases\"",
                "data_dir = \"/tmp/zeta4j-config-data\"",
            );
        let directory = config_directory(&server, &core)?;
        let explicit_data = default_data_dir();
        let args = TestArgs {
            http_port: Some(DEFAULT_HTTP_PORT),
            grpc_port: Some(DEFAULT_GRPC_PORT),
            compression: Some(true),
            timeout: Some(DEFAULT_TIMEOUT_SECS),
            timezone: Some(DEFAULT_TIMEZONE.to_string()),
            log_level: Some(DEFAULT_LOG_LEVEL.to_string()),
            data_dir: Some(explicit_data.clone()),
            config_dir: Some(directory.path().to_path_buf()),
            ..Default::default()
        };

        let config = ConfigMerger::new(&args, Edition::Base)?.merge();
        assert_eq!(config.http_port, DEFAULT_HTTP_PORT);
        assert_eq!(config.grpc_port, DEFAULT_GRPC_PORT);
        assert!(config.compression);
        assert_eq!(config.timeout_secs, DEFAULT_TIMEOUT_SECS);
        assert_eq!(config.timezone, DEFAULT_TIMEZONE);
        assert_eq!(config.log_level, DEFAULT_LOG_LEVEL);
        assert_eq!(config.data_dir, explicit_data);
        Ok(())
    }

    #[test]
    fn malformed_explicit_config_fails_instead_of_falling_back()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = format!("{DEFAULT_SERVER_TOML}\nunknown = true\n");
        let directory = config_directory(&server, DEFAULT_CORE_TOML)?;
        let args = TestArgs {
            config_dir: Some(directory.path().to_path_buf()),
            ..Default::default()
        };

        assert!(ConfigMerger::new(&args, Edition::Base).is_err());
        Ok(())
    }

    #[test]
    fn missing_required_config_fails() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        fs::write(directory.path().join("core.toml"), DEFAULT_CORE_TOML)?;
        let args = TestArgs {
            config_dir: Some(directory.path().to_path_buf()),
            ..Default::default()
        };

        assert!(ConfigMerger::new(&args, Edition::Base).is_err());
        Ok(())
    }

    #[test]
    fn bolt_section_cannot_disable_authentication() -> Result<(), Box<dyn std::error::Error>> {
        let server = DEFAULT_SERVER_TOML.replace("[bolt]\n", "[bolt]\nauth_required = false\n");
        let directory = config_directory(&server, DEFAULT_CORE_TOML)?;
        let args = TestArgs {
            config_dir: Some(directory.path().to_path_buf()),
            ..Default::default()
        };

        assert!(ConfigMerger::new(&args, Edition::Base).is_err());
        Ok(())
    }

    #[test]
    fn config_auth_disable_controls_effective_auth_state() -> Result<(), Box<dyn std::error::Error>>
    {
        let server = DEFAULT_SERVER_TOML.replace("enabled = true", "enabled = false");
        let directory = config_directory(&server, DEFAULT_CORE_TOML)?;
        let args = TestArgs {
            config_dir: Some(directory.path().to_path_buf()),
            ..Default::default()
        };

        let config = ConfigMerger::new(&args, Edition::Base)?.merge();
        assert!(!config.auth_enabled);
        Ok(())
    }
}

/// Common server CLI arguments
pub trait CommonArgs {
    fn runtime_profile(&self) -> Option<RuntimeProfile>;
    fn host(&self) -> Option<&str>;
    fn http_port(&self) -> Option<u16>;
    fn bolt_port(&self) -> Option<u16>;
    fn grpc_port(&self) -> Option<u16>;
    fn compression(&self) -> Option<bool>;
    fn timeout(&self) -> Option<u64>;
    fn log_level(&self) -> Option<&str>;
    fn log_dir(&self) -> Option<&PathBuf>;
    fn data_dir(&self) -> Option<&PathBuf>;
    fn config_dir(&self) -> Option<&PathBuf>;
    fn timezone(&self) -> Option<&str>;
    fn replace(&self) -> bool;
    fn no_auth(&self) -> bool;
}

/// Configuration merger that combines CLI args with config file settings
pub struct ConfigMerger<'a, A: CommonArgs> {
    args: &'a A,
    registry: Option<ConfigRegistry>,
    config_fingerprint: Option<String>,
}

impl<'a, A: CommonArgs> ConfigMerger<'a, A> {
    /// Create a new ConfigMerger
    pub fn new(args: &'a A, edition: Edition) -> Result<Self, ConfigError> {
        let (registry, config_fingerprint) = if let Some(config_dir) = args.config_dir() {
            let fingerprint_before = fingerprint_config_directory(config_dir)?;
            let registry = ConfigRegistry::for_edition_with_path(edition, config_dir)?;
            let fingerprint_after = fingerprint_config_directory(config_dir)?;
            if fingerprint_before != fingerprint_after {
                return Err(ConfigError::InvalidConfig(
                    "configuration changed while it was being loaded".to_string(),
                ));
            }
            (Some(registry), Some(fingerprint_after))
        } else {
            (None, None)
        };

        Ok(Self {
            args,
            registry,
            config_fingerprint,
        })
    }

    /// Get the ConfigRegistry (if loaded)
    pub fn registry(&self) -> Option<&ConfigRegistry> {
        self.registry.as_ref()
    }

    /// Fingerprint of the exact configuration files used for resolution.
    pub fn config_fingerprint(&self) -> Option<&str> {
        self.config_fingerprint.as_deref()
    }

    /// Merge CLI args with config file settings
    pub fn merge(&self) -> EffectiveConfig {
        let mut config = EffectiveConfig {
            runtime_profile: self
                .args
                .runtime_profile()
                .unwrap_or(RuntimeProfile::Production),
            config_dir: self
                .args
                .config_dir()
                .cloned()
                .unwrap_or_else(default_config_dir),
            timezone: self.args.timezone().unwrap_or(DEFAULT_TIMEZONE).to_string(),
            replace: self.args.replace(),
            ..Default::default()
        };

        self.resolve(&mut config);

        config
    }

    /// Resolve every source through one precedence path.
    fn resolve(&self, config: &mut EffectiveConfig) {
        if let Some(registry) = self.registry.as_ref() {
            let core = registry.core();
            config.log_level = self
                .args
                .log_level()
                .unwrap_or(&core.logging.level)
                .to_string();
            config.log_format = core.logging.format.clone();
            config.log_dir = self
                .args
                .log_dir()
                .cloned()
                .unwrap_or_else(|| expand_tilde(&core.logging.directory));

            if let Some(server) = registry.server() {
                config.runtime_profile = self
                    .args
                    .runtime_profile()
                    .unwrap_or(server.runtime.profile);
                let global_host = self.args.host();
                config.http_host = global_host.unwrap_or(&server.http.bind).to_string();
                config.bolt_host = global_host.unwrap_or(&server.bolt.bind).to_string();
                config.grpc_host = global_host.unwrap_or(&server.grpc.bind).to_string();
                config.http_port = self.args.http_port().unwrap_or(server.http.port);
                config.bolt_port = self.args.bolt_port().unwrap_or(server.bolt.port);
                config.grpc_port = self.args.grpc_port().unwrap_or(server.grpc.port);

                config.compression = self.args.compression().unwrap_or(server.http.compression);
                config.timeout_secs = self
                    .args
                    .timeout()
                    .unwrap_or(server.http.request_timeout_secs);
                config.timezone = self
                    .args
                    .timezone()
                    .unwrap_or(&server.http.timezone)
                    .to_string();
                config.max_connections = server.http.max_connections;
                config.max_body_size = server.http.max_body_size;
                config.keep_alive_timeout_secs = server.http.keep_alive_timeout_secs;

                let cors_cfg = &server.http.cors;
                config.cors = CorsConfig {
                    allowed_origins: cors_cfg.allowed_origins.clone(),
                    allowed_methods: cors_cfg.allowed_methods.clone(),
                    allowed_headers: cors_cfg.allowed_headers.clone(),
                    expose_headers: vec!["x-request-id".to_string()],
                    max_age: Duration::from_secs(cors_cfg.max_age_secs),
                    allow_credentials: cors_cfg.allow_credentials,
                };

                config.bolt_max_connections = server.bolt.max_connections;
                config.bolt_timeout_secs = server.bolt.connection_timeout_secs;
                config.bolt_default_database = server.bolt.default_database.clone();

                config.auth_enabled = !self.args.no_auth() && server.auth.enabled;
                config.jwt_signing_key_file = PathBuf::from(&server.auth.jwt_signing_key_file);
                config.jwt_key_id = server.auth.jwt_key_id.clone();
                config.jwt_key_version = server.auth.jwt_key_version;
                config.access_token_ttl_secs = server.auth.access_token_ttl_secs;
                config.refresh_token_ttl_secs = server.auth.refresh_token_ttl_secs;

                config.transport_mode = server.transport.mode.clone();
                config.server_cert_ref = server.transport.server_cert_ref.clone();
                config.server_key_ref = server.transport.server_key_ref.clone();

                self.log_server_config(config);
            }

            config.data_dir = self
                .args
                .data_dir()
                .cloned()
                .unwrap_or_else(|| expand_tilde(&core.storage.data_dir));
            config.max_result_size = core.query.max_result_size;
            config.query_timeout_ms = core.query.timeout_ms;

            self.log_core_config(core);
        } else {
            let host = self.args.host().unwrap_or(DEFAULT_HOST).to_string();
            config.http_host.clone_from(&host);
            config.bolt_host.clone_from(&host);
            config.grpc_host = host;
            config.http_port = self.args.http_port().unwrap_or(DEFAULT_HTTP_PORT);
            config.bolt_port = self.args.bolt_port().unwrap_or(DEFAULT_BOLT_PORT);
            config.grpc_port = self.args.grpc_port().unwrap_or(DEFAULT_GRPC_PORT);
            config.compression = self.args.compression().unwrap_or(true);
            config.timeout_secs = self.args.timeout().unwrap_or(DEFAULT_TIMEOUT_SECS);
            config.log_level = self
                .args
                .log_level()
                .unwrap_or(DEFAULT_LOG_LEVEL)
                .to_string();
            config.log_dir = self.args.log_dir().cloned().unwrap_or_else(default_log_dir);
            config.data_dir = self
                .args
                .data_dir()
                .cloned()
                .unwrap_or_else(default_data_dir);
            config.auth_enabled = !self.args.no_auth();
        }
        apply_credential_environment(config);
        apply_transport_environment(config);
    }

    /// Log server config application
    fn log_server_config(&self, config: &EffectiveConfig) {
        info!(
            "Applied server.toml: HTTP {}:{}, Bolt :{}, compression={}, timeout={}s, auth={}",
            config.http_host,
            config.http_port,
            config.bolt_port,
            config.compression,
            config.timeout_secs,
            config.auth_enabled
        );
        info!("  CORS origins: {:?}", config.cors.allowed_origins);
        info!("  Driver: max_databases={}", config.max_databases);
    }

    /// Log core config application
    fn log_core_config(&self, core: &zeta4j_server::config::Zeta4jConfig) {
        info!(
            "Applied core.toml: tx_timeout={}ms, lock_wait={}ms, query_timeout={}ms",
            core.transaction.timeout_ms, core.transaction.lock_wait_ms, core.query.timeout_ms
        );
        info!(
            "  WAL: segment_size={}MB, max_segments={}, flush_interval={}",
            core.wal.max_segment_size / (1024 * 1024),
            core.wal.max_segments,
            core.wal.flush_interval
        );
        info!(
            "  MVCC: max_write_sets={}, snapshot_cache_size={}",
            core.mvcc.max_write_sets, core.mvcc.snapshot_cache_size
        );
        info!(
            "  Compaction: min_records={}, max_records={}",
            core.compaction.min_records, core.compaction.max_records
        );
        info!(
            "  Index: auto_create={}, default_type={}",
            core.index.auto_create, core.index.default_type
        );
        info!(
            "  Logging: level={}, format={}",
            core.logging.level, core.logging.format
        );
        info!("  Storage: data_dir={}", core.storage.data_dir);
    }
}

fn expand_tilde(value: &str) -> PathBuf {
    if let Some(relative) = value.strip_prefix("~/") {
        dirs::home_dir()
            .map(|home| home.join(relative))
            .unwrap_or_else(|| PathBuf::from(value))
    } else {
        PathBuf::from(value)
    }
}

fn apply_credential_environment(config: &mut EffectiveConfig) {
    if let Some(value) = std::env::var_os("ZETA4J_JWT_SIGNING_KEY_FILE")
        && !value.is_empty()
    {
        config.jwt_signing_key_file = PathBuf::from(value);
    }
    if let Ok(value) = std::env::var("ZETA4J_JWT_KEY_ID")
        && !value.is_empty()
    {
        config.jwt_key_id = value;
    }
    if let Ok(value) = std::env::var("ZETA4J_JWT_KEY_VERSION")
        && !value.is_empty()
    {
        config.jwt_key_version = value.parse::<u64>().unwrap_or(0);
    }
}

fn apply_transport_environment(config: &mut EffectiveConfig) {
    for (name, target) in [
        ("ZETA4J_TRANSPORT_MODE", &mut config.transport_mode),
        (
            "ZETA4J_TRANSPORT_SERVER_CERT_REF",
            &mut config.server_cert_ref,
        ),
        (
            "ZETA4J_TRANSPORT_SERVER_KEY_REF",
            &mut config.server_key_ref,
        ),
    ] {
        if let Ok(value) = std::env::var(name)
            && !value.is_empty()
        {
            *target = value;
        }
    }
}

fn fingerprint_config_directory(directory: &Path) -> Result<String, ConfigError> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for name in ["core.toml", "server.toml"] {
        name.hash(&mut hasher);
        fs::read(directory.join(name))?.hash(&mut hasher);
    }
    Ok(format!("{:016x}", hasher.finish()))
}

/// Secret-free, machine-readable resolved configuration.
#[derive(Debug, Serialize)]
pub struct EffectiveConfigSnapshot<'a> {
    pub runtime_profile: RuntimeProfile,
    pub security_policy: &'static str,
    pub http_address: String,
    pub bolt_address: String,
    pub grpc_address: String,
    pub data_dir: &'a Path,
    pub log_dir: &'a Path,
    pub config_dir: &'a Path,
    pub log_level: &'a str,
    pub compression: bool,
    pub timeout_secs: u64,
    pub timezone: &'a str,
    pub auth_enabled: bool,
    pub tls_enabled: bool,
    pub config_fingerprint: Option<&'a str>,
}

impl EffectiveConfig {
    pub fn snapshot<'a>(
        &'a self,
        config_fingerprint: Option<&'a str>,
    ) -> EffectiveConfigSnapshot<'a> {
        EffectiveConfigSnapshot {
            runtime_profile: self.runtime_profile,
            security_policy: "allowed",
            http_address: self.http_addr(),
            bolt_address: self.bolt_addr(),
            grpc_address: self.grpc_addr(),
            data_dir: &self.data_dir,
            log_dir: &self.log_dir,
            config_dir: &self.config_dir,
            log_level: &self.log_level,
            compression: self.compression,
            timeout_secs: self.timeout_secs,
            timezone: &self.timezone,
            auth_enabled: self.auth_enabled,
            tls_enabled: self.tls_enabled(),
            config_fingerprint,
        }
    }
}
