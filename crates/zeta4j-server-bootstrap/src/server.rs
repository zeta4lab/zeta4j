// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server configuration builder.

use std::net::SocketAddr;
use std::sync::Arc;

use log::info;

use zeta4j_credential::KeyVersion;
use zeta4j_graph::config::Edition;
use zeta4j_server::server::auth::{CredentialTokenService, SignerReference};
use zeta4j_server::server::bolt::{BoltConfig, BoltQueryExecutor, BoltServer};
use zeta4j_server::server::http::{HttpConfig, HttpRouterProtection, UnifiedRouterBuilder};
use zeta4j_server::server::system::{
    CredentialRuntimePolicy, LocalSecurityService, SecurityManager,
    initialize_local_credential_runtime,
};

use super::database::{DatabaseServices, RuntimeAdminService};
use super::{EffectiveConfig, ValidatedStartupConfig};

/// Server components ready for runtime
pub struct ServerComponents {
    /// Unified HTTP router (axum)
    pub app: axum::Router,
    /// HTTP bind address
    pub http_addr: SocketAddr,
    /// Maximum concurrently accepted HTTP connections.
    pub http_max_connections: usize,
    /// Maximum idle duration for an accepted HTTP connection.
    pub http_keep_alive_timeout: std::time::Duration,
    /// Bolt server (pre-configured)
    pub bolt_server: BoltServer,
    /// Bolt bind address
    pub bolt_addr: SocketAddr,
    /// gRPC bind address
    pub grpc_addr: SocketAddr,
    /// System database service for gRPC
    pub system_db_service: zeta4j_server::server::system::SharedSystemDatabaseService,
    /// Tenant database manager for gRPC dump/load
    pub db_manager: Arc<zeta4j_server::server::tenant::TenantDatabaseManager>,
    /// gRPC authentication enabled
    pub grpc_auth_enabled: bool,
    /// One canonical credential lifecycle service shared by every adapter.
    pub credential_security: Option<Arc<LocalSecurityService>>,
    /// One canonical Ed25519 token verifier shared by HTTP and gRPC.
    pub credential_tokens: Option<Arc<CredentialTokenService>>,
    /// Security manager for gRPC authentication
    pub security_manager: Option<Arc<SecurityManager>>,
    /// Server edition
    pub edition: Edition,
    /// GraphEngine (shutdown checkpoint용)
    pub graph_engine: Arc<zeta4j_server::engine::GraphEngine>,
    /// 비밀 값을 제외한 실제 병합 런타임 설정.
    pub admin_config_content: String,
}

struct PendingServerComponents {
    components: ServerComponents,
    http_protection: HttpRouterProtection,
}

/// Server configuration builder
pub struct ServerConfigBuilder<'a> {
    config: &'a ValidatedStartupConfig,
    db_services: &'a DatabaseServices,
    edition: Edition,
}

impl<'a> ServerConfigBuilder<'a> {
    /// Create a new ServerConfigBuilder
    pub fn new(config: &'a ValidatedStartupConfig, db_services: &'a DatabaseServices) -> Self {
        Self {
            config,
            db_services,
            edition: Edition::Base,
        }
    }

    /// Set the server edition
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.edition = edition;
        self
    }

    /// Build server components
    pub async fn build(self) -> Result<ServerComponents, ServerError> {
        let pending = self.build_unwrapped().await?;
        let mut components = pending.components;
        components.app = pending.http_protection.apply(components.app);
        Ok(components)
    }

    async fn build_unwrapped(self) -> Result<PendingServerComponents, ServerError> {
        // Parse addresses
        let http_addr: SocketAddr = self
            .config
            .http_addr()
            .parse()
            .map_err(|e| ServerError::InvalidAddress(format!("HTTP: {}", e)))?;
        let bolt_addr: SocketAddr = self
            .config
            .bolt_addr()
            .parse()
            .map_err(|e| ServerError::InvalidAddress(format!("Bolt: {}", e)))?;
        let grpc_addr: SocketAddr = self
            .config
            .grpc_addr()
            .parse()
            .map_err(|e| ServerError::InvalidAddress(format!("gRPC: {}", e)))?;

        // Build HTTP config
        let mut http_config = HttpConfig {
            bind_addr: http_addr,
            compression: self.config.compression,
            request_timeout: self.config.request_timeout(),
            max_connections: self.config.max_connections,
            max_body_size: self.config.max_body_size,
            keep_alive_timeout: std::time::Duration::from_secs(self.config.keep_alive_timeout_secs),
            cors: self.config.cors.clone(),
            timezone: self.config.timezone.clone(),
            ..Default::default()
        };
        // Auth settings (from EffectiveConfig, single source of truth)
        http_config.auth.enabled = self.config.auth_enabled;
        http_config.auth.jwt_signing_key_file = self.config.jwt_signing_key_file.clone();
        http_config.auth.jwt_key_id = self.config.jwt_key_id.clone();
        http_config.auth.jwt_key_version = self.config.jwt_key_version;
        http_config.auth.access_token_ttl =
            std::time::Duration::from_secs(self.config.access_token_ttl_secs);
        http_config.auth.refresh_token_ttl =
            std::time::Duration::from_secs(self.config.refresh_token_ttl_secs);
        if !self.config.auth_enabled {
            info!("Authentication DISABLED");
        }

        // Build Bolt config
        let bolt_config = BoltConfig {
            bind_addr: bolt_addr,
            max_connections: self.config.bolt_max_connections,
            connection_timeout: self.config.bolt_timeout(),
            auth_required: self.config.auth_enabled,
            default_database: self.config.bolt_default_database.clone(),
            ..Default::default()
        };

        info!(
            "Query config: max_result_size={}, timeout_ms={}",
            self.config.max_result_size, self.config.query_timeout_ms
        );

        // Create query plan cache (parse: 1000, plan: 500)
        let query_cache =
            Arc::new(zeta4j_cypher_executor::cache::ConcurrentQueryCacheManager::with_defaults());
        info!("Query plan cache enabled (parse: 1000, plan: 500)");

        let admin_service: Arc<dyn zeta4j_cypher_executor::AdminService> =
            Arc::new(RuntimeAdminService::new(
                self.db_services.system_db_service.clone(),
                self.db_services.db_manager.clone(),
                bolt_addr.to_string(),
            ));

        // Create Bolt Query Executor
        let bolt_executor: Arc<
            dyn zeta4j_server::server::bolt::connection::handler::AsyncQueryExecutor,
        > = Arc::new(
            BoltQueryExecutor::with_db_manager(
                self.db_services.default_engine.clone(),
                self.db_services.db_manager.clone(),
            )
            .with_admin_service(admin_service.clone())
            .with_fulltext_provider(self.db_services.fulltext_provider.clone())
            .with_vector_provider(self.db_services.vector_provider.clone())
            .with_query_config(self.config.max_result_size, self.config.query_timeout_ms)
            .with_shared_index_cache(self.db_services.shared_index_cache.clone())
            .with_query_cache(query_cache),
        );

        // Get SecurityManager from SystemDatabaseService (RocksDB-backed, unified auth)
        let security_manager = Arc::new(
            SecurityManager::try_new(self.db_services.system_db_service.storage().clone())
                .map_err(|error| ServerError::CredentialConfiguration(error.to_string()))?,
        );
        let credential_runtime = if self.config.auth_enabled {
            let key_version = KeyVersion::new(self.config.jwt_key_version)
                .map_err(|error| ServerError::CredentialConfiguration(error.to_string()))?;
            let signer_reference = SignerReference::new(
                &self.config.jwt_signing_key_file,
                &self.config.jwt_key_id,
                key_version,
            )
            .map_err(|error| ServerError::CredentialConfiguration(error.to_string()))?;
            let refresh_ttl = i64::try_from(self.config.refresh_token_ttl_secs).map_err(|_| {
                ServerError::CredentialConfiguration(
                    "refresh token TTL is out of range".to_string(),
                )
            })?;
            let access_ttl = i64::try_from(self.config.access_token_ttl_secs).map_err(|_| {
                ServerError::CredentialConfiguration("access token TTL is out of range".to_string())
            })?;
            Some(
                initialize_local_credential_runtime(
                    security_manager.clone(),
                    &signer_reference,
                    CredentialRuntimePolicy {
                        access_ttl_seconds: access_ttl,
                        refresh_ttl_seconds: refresh_ttl,
                        session_absolute_ttl_seconds: refresh_ttl,
                        session_idle_ttl_seconds: refresh_ttl.min(24 * 60 * 60).max(access_ttl),
                        ..CredentialRuntimePolicy::default()
                    },
                    "base".to_string(),
                )
                .map_err(|error| ServerError::CredentialConfiguration(error.to_string()))?,
            )
        } else {
            None
        };
        if let Some((credential_security, credential_tokens)) = &credential_runtime {
            tokio::spawn(zeta4j_server::server::system::maintain_local_signing_keys(
                credential_security.clone(),
                credential_tokens.clone(),
            ));
        }

        // Create unified router
        info!("Creating unified router...");
        let http_max_connections = http_config.max_connections;
        let http_keep_alive_timeout = http_config.keep_alive_timeout;
        let (app, http_protection) =
            UnifiedRouterBuilder::new(self.db_services.default_engine.clone())
                .http_config(http_config)
                .security_manager(security_manager.clone())
                .credential_runtime(
                    credential_runtime
                        .as_ref()
                        .map(|(security, _)| security.clone()),
                    credential_runtime
                        .as_ref()
                        .map(|(_, tokens)| tokens.clone()),
                )
                .tenant_db_manager(self.db_services.db_manager.clone())
                .admin_service(admin_service)
                .fulltext_provider(self.db_services.fulltext_provider.clone())
                .vector_provider(self.db_services.vector_provider.clone())
                .query_config(self.config.max_result_size, self.config.query_timeout_ms)
                .with_edition(self.edition)
                .build_unprotected()
                .map_err(|error| ServerError::RouterBuild(error.to_string()))?;

        // Create Bolt server
        let mut bolt_server = BoltServer::with_executor(bolt_config, bolt_executor);
        if let Some(tls_config) = self.config.transport().bolt_server() {
            bolt_server.set_tls_config(tls_config);
        }
        if self.config.auth_enabled {
            bolt_server.set_security_manager(security_manager.clone());
            if let Some((credential_security, _)) = &credential_runtime {
                bolt_server.set_credential_security(credential_security.clone());
            }
            if let Some((_, credential_tokens)) = &credential_runtime {
                bolt_server.set_credential_tokens(credential_tokens.clone());
            }
        }
        bolt_server.set_tenant_db_manager(self.db_services.db_manager.clone());
        bolt_server
            .bind(bolt_addr)
            .await
            .map_err(|e| ServerError::BindFailed(format!("Bolt: {}", e)))?;

        Ok(PendingServerComponents {
            components: ServerComponents {
                app,
                http_addr,
                http_max_connections,
                http_keep_alive_timeout,
                bolt_server,
                bolt_addr,
                grpc_addr,
                system_db_service: self.db_services.system_db_service.clone(),
                db_manager: self.db_services.db_manager.clone(),
                grpc_auth_enabled: self.config.auth_enabled,
                credential_security: credential_runtime
                    .as_ref()
                    .map(|(security, _)| security.clone()),
                credential_tokens: credential_runtime
                    .as_ref()
                    .map(|(_, tokens)| tokens.clone()),
                security_manager: if self.config.auth_enabled {
                    Some(security_manager)
                } else {
                    None
                },
                edition: self.edition,
                graph_engine: self.db_services.default_engine.clone(),
                admin_config_content: admin_config_content(self.config),
            },
            http_protection,
        })
    }
}

fn admin_config_content(config: &EffectiveConfig) -> String {
    let string = |value: &str| toml::Value::String(value.to_string()).to_string();
    format!(
        "[server]\nhttp_host = {}\nbolt_host = {}\ngrpc_host = {}\nhttp_port = {}\nbolt_port = {}\ngrpc_port = {}\ncompression = {}\ntimeout_seconds = {}\nmax_connections = {}\nmax_body_size = {}\n\n[database]\ndata_dir = {}\nmax_databases = {}\n\n[security]\nauth_enabled = {}\ntls_enabled = {}\ntransport_mode = {}\naccess_token_ttl_seconds = {}\nrefresh_token_ttl_seconds = {}\n\n[query]\nmax_result_size = {}\ntimeout_milliseconds = {}\n\n[logging]\nlevel = {}\nformat = {}\ndirectory = {}\nrotation_size_mb = {}\nrotation_count = {}\n",
        string(&config.http_host),
        string(&config.bolt_host),
        string(&config.grpc_host),
        config.http_port,
        config.bolt_port,
        config.grpc_port,
        config.compression,
        config.timeout_secs,
        config.max_connections,
        config.max_body_size,
        string(&config.data_dir.to_string_lossy()),
        config.max_databases,
        config.auth_enabled,
        config.tls_enabled(),
        string(&config.transport_mode),
        config.access_token_ttl_secs,
        config.refresh_token_ttl_secs,
        config.max_result_size,
        config.query_timeout_ms,
        string(&config.log_level),
        string(&config.log_format),
        string(&config.log_dir.to_string_lossy()),
        config.log_rotation_size_mb,
        config.log_rotation_count,
    )
}

/// Server configuration errors
#[derive(Debug)]
pub enum ServerError {
    /// Invalid address format
    InvalidAddress(String),
    /// Bind failed
    BindFailed(String),
    /// HTTP router construction failed
    RouterBuild(String),
    /// Canonical credential runtime could not be initialized.
    CredentialConfiguration(String),
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerError::InvalidAddress(msg) => write!(f, "Invalid address: {}", msg),
            ServerError::BindFailed(msg) => write!(f, "Failed to bind: {}", msg),
            ServerError::RouterBuild(msg) => write!(f, "Failed to build HTTP router: {}", msg),
            ServerError::CredentialConfiguration(message) => {
                write!(f, "Failed to initialize credential runtime: {message}")
            }
        }
    }
}

impl std::error::Error for ServerError {}
