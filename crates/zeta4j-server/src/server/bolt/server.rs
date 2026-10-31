// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bolt server implementation.
//!
//! Provides a TCP server that accepts Bolt protocol connections
//! and handles them asynchronously.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use super::connection::BoltConnection;
use super::connection::handler::AsyncQueryExecutor;
use super::connection::session::SessionStore;
use super::handshake::{BOLT_MAGIC, Handshake};
use super::state::BoltEvent;
use super::{BoltConfig, BoltError, BoltResult};

use crate::server::auth::CredentialTokenService;
use crate::server::system::{LocalSecurityService, SecurityManager};
use crate::server::tenant::TenantDatabaseManager;

/// Bolt server state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoltServerState {
    /// Server is stopped
    Stopped,
    /// Server is starting
    Starting,
    /// Server is running
    Running,
    /// Server is stopping
    Stopping,
}

/// Bolt server statistics.
#[derive(Debug, Default)]
pub struct BoltServerStats {
    /// Total connections accepted
    pub connections_accepted: AtomicU64,
    /// Current active connections
    pub connections_active: AtomicU64,
    /// Total connections rejected (max reached)
    pub connections_rejected: AtomicU64,
    /// Total handshake failures
    pub handshake_failures: AtomicU64,
    /// Total messages processed
    pub messages_processed: AtomicU64,
    /// Current active queries (for graceful shutdown)
    pub active_queries: AtomicU64,
}

impl BoltServerStats {
    /// Create new stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get snapshot of stats.
    pub fn snapshot(&self) -> BoltServerStatsSnapshot {
        BoltServerStatsSnapshot {
            connections_accepted: self.connections_accepted.load(Ordering::Relaxed),
            connections_active: self.connections_active.load(Ordering::Relaxed),
            connections_rejected: self.connections_rejected.load(Ordering::Relaxed),
            handshake_failures: self.handshake_failures.load(Ordering::Relaxed),
            messages_processed: self.messages_processed.load(Ordering::Relaxed),
            active_queries: self.active_queries.load(Ordering::Relaxed),
        }
    }

    /// Increment active queries count.
    pub fn query_start(&self) {
        self.active_queries.fetch_add(1, Ordering::AcqRel);
    }

    /// Decrement active queries count.
    pub fn query_end(&self) {
        self.active_queries.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Snapshot of server statistics.
#[derive(Debug, Clone)]
pub struct BoltServerStatsSnapshot {
    pub connections_accepted: u64,
    pub connections_active: u64,
    pub connections_rejected: u64,
    pub handshake_failures: u64,
    pub messages_processed: u64,
    pub active_queries: u64,
}

/// Default shutdown timeout (10 seconds).
const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 10;

/// Bolt protocol server.
pub struct BoltServer {
    /// Server configuration
    config: Arc<BoltConfig>,
    /// TCP listener
    listener: Option<TcpListener>,
    /// Server state
    state: Arc<RwLock<BoltServerState>>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
    /// Connection semaphore (for max connections)
    connection_semaphore: Arc<Semaphore>,
    /// Session store
    sessions: Arc<SessionStore>,
    /// Statistics
    stats: Arc<BoltServerStats>,
    /// Query executor for Cypher queries
    executor: Option<Arc<dyn AsyncQueryExecutor>>,
    /// Security manager for authentication (RocksDB-backed)
    security_manager: Option<Arc<SecurityManager>>,
    /// Canonical durable credential lifecycle.
    credential_security: Option<Arc<LocalSecurityService>>,
    credential_tokens: Option<Arc<CredentialTokenService>>,
    /// Canonical database identity resolver.
    tenant_db_manager: Option<Arc<TenantDatabaseManager>>,
    /// Shutdown timeout for graceful shutdown (default: 10 seconds)
    shutdown_timeout: Duration,
    /// Preflight-validated TLS generation. `None` is local-test plaintext only.
    tls_config: Option<Arc<rustls::ServerConfig>>,
}

impl BoltServer {
    /// Create a new Bolt server.
    pub fn new(config: BoltConfig) -> Self {
        let max_connections = config.max_connections;
        Self {
            config: Arc::new(config),
            listener: None,
            state: Arc::new(RwLock::new(BoltServerState::Stopped)),
            shutdown: Arc::new(AtomicBool::new(false)),
            connection_semaphore: Arc::new(Semaphore::new(max_connections)),
            sessions: Arc::new(SessionStore::new()),
            stats: Arc::new(BoltServerStats::new()),
            executor: None,
            security_manager: None,
            credential_security: None,
            credential_tokens: None,
            tenant_db_manager: None,
            shutdown_timeout: Duration::from_secs(DEFAULT_SHUTDOWN_TIMEOUT_SECS),
            tls_config: None,
        }
    }

    /// Create a new Bolt server with a query executor.
    pub fn with_executor(config: BoltConfig, executor: Arc<dyn AsyncQueryExecutor>) -> Self {
        let max_connections = config.max_connections;
        Self {
            config: Arc::new(config),
            listener: None,
            state: Arc::new(RwLock::new(BoltServerState::Stopped)),
            shutdown: Arc::new(AtomicBool::new(false)),
            connection_semaphore: Arc::new(Semaphore::new(max_connections)),
            sessions: Arc::new(SessionStore::new()),
            stats: Arc::new(BoltServerStats::new()),
            executor: Some(executor),
            security_manager: None,
            credential_security: None,
            credential_tokens: None,
            tenant_db_manager: None,
            shutdown_timeout: Duration::from_secs(DEFAULT_SHUTDOWN_TIMEOUT_SECS),
            tls_config: None,
        }
    }

    /// Set shutdown timeout for graceful shutdown.
    pub fn set_shutdown_timeout(&mut self, timeout: Duration) {
        self.shutdown_timeout = timeout;
    }

    /// Installs the preflight-validated TLS generation before binding.
    pub fn set_tls_config(&mut self, config: Arc<rustls::ServerConfig>) {
        self.tls_config = Some(config);
    }

    /// Set the security manager for authentication (RocksDB-backed).
    pub fn set_security_manager(&mut self, security_manager: Arc<SecurityManager>) {
        self.security_manager = Some(security_manager);
    }

    pub fn set_credential_security(&mut self, credential_security: Arc<LocalSecurityService>) {
        self.credential_security = Some(credential_security);
    }

    pub fn set_credential_tokens(&mut self, credential_tokens: Arc<CredentialTokenService>) {
        self.credential_tokens = Some(credential_tokens);
    }

    /// Set the canonical database identity resolver.
    pub fn set_tenant_db_manager(&mut self, manager: Arc<TenantDatabaseManager>) {
        self.tenant_db_manager = Some(manager);
    }

    /// Get server state.
    pub fn state(&self) -> BoltServerState {
        *self.state.read()
    }

    /// Get statistics.
    pub fn stats(&self) -> BoltServerStatsSnapshot {
        self.stats.snapshot()
    }

    /// Get active connection count.
    pub fn active_connections(&self) -> usize {
        self.stats.connections_active.load(Ordering::Relaxed) as usize
    }

    /// Get session count.
    pub fn session_count(&self) -> usize {
        self.sessions.count()
    }

    /// Bind to address.
    pub async fn bind(&mut self, addr: SocketAddr) -> BoltResult<()> {
        *self.state.write() = BoltServerState::Starting;

        let listener = TcpListener::bind(addr).await.map_err(BoltError::Io)?;

        info!("Bolt server bound to {}", addr);
        self.listener = Some(listener);

        Ok(())
    }

    /// Start accepting connections.
    pub async fn run(&mut self) -> BoltResult<()> {
        let listener = self
            .listener
            .take()
            .ok_or_else(|| BoltError::Protocol("Server not bound".to_string()))?;

        *self.state.write() = BoltServerState::Running;
        self.shutdown.store(false, Ordering::Release);

        info!("Bolt server started, accepting connections");

        loop {
            // Check shutdown
            if self.shutdown.load(Ordering::Acquire) {
                break;
            }

            // Accept connection with timeout
            let accept_result =
                tokio::time::timeout(Duration::from_millis(100), listener.accept()).await;

            let (stream, addr) = match accept_result {
                Ok(Ok((stream, addr))) => (stream, addr),
                Ok(Err(e)) => {
                    error!("Accept error: {}", e);
                    continue;
                }
                Err(_) => {
                    // Timeout, check shutdown flag
                    continue;
                }
            };
            if let Err(error) = stream.set_nodelay(true) {
                warn!("Failed to configure connection from {}: {}", addr, error);
                continue;
            }

            // Check max connections
            let permit = match self.connection_semaphore.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    warn!("Max connections reached, rejecting {}", addr);
                    self.stats
                        .connections_rejected
                        .fetch_add(1, Ordering::Relaxed);
                    drop(stream);
                    continue;
                }
            };

            self.stats
                .connections_accepted
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .connections_active
                .fetch_add(1, Ordering::Relaxed);

            debug!("Accepted connection from {}", addr);

            // Spawn connection handler
            let config = Arc::clone(&self.config);
            let sessions = Arc::clone(&self.sessions);
            let stats = Arc::clone(&self.stats);
            let shutdown = Arc::clone(&self.shutdown);
            let executor = self.executor.clone();
            let security_manager = self.security_manager.clone();
            let credential_security = self.credential_security.clone();
            let credential_tokens = self.credential_tokens.clone();
            let tenant_db_manager = self.tenant_db_manager.clone();
            let tls_config = self.tls_config.clone();

            tokio::spawn(async move {
                let _permit = permit; // Hold permit until connection closes
                let stats_clone = Arc::clone(&stats);

                let result = async move {
                    if let Some(tls_config) = tls_config {
                        let acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
                        match acceptor.accept(stream).await {
                            Ok(stream) => {
                                handle_connection(
                                    stream,
                                    addr,
                                    config,
                                    sessions,
                                    stats,
                                    shutdown,
                                    executor,
                                    security_manager,
                                    credential_security,
                                    credential_tokens,
                                    tenant_db_manager,
                                )
                                .await
                            }
                            Err(_) => Err(BoltError::Protocol(
                                "[SV-TLS-003] TLS handshake rejected".to_string(),
                            )),
                        }
                    } else {
                        handle_connection(
                            stream,
                            addr,
                            config,
                            sessions,
                            stats,
                            shutdown,
                            executor,
                            security_manager,
                            credential_security,
                            credential_tokens,
                            tenant_db_manager,
                        )
                        .await
                    }
                }
                .await;
                if let Err(error) = result
                    && !matches!(error, BoltError::Io(_))
                {
                    warn!("Connection error from {}: {}", addr, error);
                }

                stats_clone
                    .connections_active
                    .fetch_sub(1, Ordering::Relaxed);
                debug!("Connection closed from {}", addr);
            });
        }

        *self.state.write() = BoltServerState::Stopped;
        info!("Bolt server stopped");

        Ok(())
    }

    /// Signal shutdown (non-blocking).
    pub fn shutdown(&self) {
        info!("Bolt server shutdown requested");
        *self.state.write() = BoltServerState::Stopping;
        self.shutdown.store(true, Ordering::Release);
    }

    /// Perform graceful shutdown.
    ///
    /// 1. Stops accepting new connections
    /// 2. Waits for active queries to complete (with timeout)
    /// 3. Logs any remaining connections that were terminated
    ///
    /// Returns the number of active queries that were still running when timeout was reached.
    pub async fn shutdown_graceful(&self) -> usize {
        info!("Bolt server graceful shutdown requested");
        *self.state.write() = BoltServerState::Stopping;
        self.shutdown.store(true, Ordering::Release);

        let start = std::time::Instant::now();
        let mut last_count = 0u64;

        // Wait for active queries to complete
        while start.elapsed() < self.shutdown_timeout {
            let active = self.stats.active_queries.load(Ordering::Acquire);

            if active == 0 {
                info!("All queries completed, proceeding with shutdown");
                break;
            }

            if active != last_count {
                info!(
                    "Waiting for {} active queries to complete ({:.1}s remaining)",
                    active,
                    (self.shutdown_timeout - start.elapsed()).as_secs_f32()
                );
                last_count = active;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let remaining = self.stats.active_queries.load(Ordering::Acquire) as usize;
        if remaining > 0 {
            warn!(
                "Shutdown timeout reached with {} queries still active",
                remaining
            );
        }

        let active_connections = self.stats.connections_active.load(Ordering::Acquire);
        if active_connections > 0 {
            info!("Terminating {} active connections", active_connections);
        }

        info!("Bolt server graceful shutdown complete");
        remaining
    }

    /// Get active query count.
    pub fn active_queries(&self) -> usize {
        self.stats.active_queries.load(Ordering::Relaxed) as usize
    }

    /// Check if server is running.
    pub fn is_running(&self) -> bool {
        *self.state.read() == BoltServerState::Running
    }

    /// Get shared stats for passing to handlers.
    pub fn shared_stats(&self) -> Arc<BoltServerStats> {
        Arc::clone(&self.stats)
    }
}

/// Handle a single connection.
#[expect(clippy::too_many_arguments)]
async fn handle_connection(
    mut stream: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    addr: SocketAddr,
    config: Arc<BoltConfig>,
    sessions: Arc<SessionStore>,
    stats: Arc<BoltServerStats>,
    _shutdown: Arc<AtomicBool>,
    executor: Option<Arc<dyn AsyncQueryExecutor>>,
    security_manager: Option<Arc<SecurityManager>>,
    credential_security: Option<Arc<LocalSecurityService>>,
    credential_tokens: Option<Arc<CredentialTokenService>>,
    tenant_db_manager: Option<Arc<TenantDatabaseManager>>,
) -> BoltResult<()> {
    // Read handshake (4 bytes magic + 16 bytes version proposals)
    let mut handshake_buf = [0u8; 20];

    tokio::time::timeout(config.connection_timeout, async {
        stream.read_exact(&mut handshake_buf).await
    })
    .await
    .map_err(|_| BoltError::Timeout)?
    .map_err(BoltError::Io)?;

    // Verify magic number
    if handshake_buf[0..4] != BOLT_MAGIC {
        stats.handshake_failures.fetch_add(1, Ordering::Relaxed);
        // Send zero version to indicate no protocol match
        stream.write_all(&[0, 0, 0, 0]).await?;
        return Err(BoltError::Protocol(format!(
            "Invalid magic number: {:02X}{:02X}{:02X}{:02X}",
            handshake_buf[0], handshake_buf[1], handshake_buf[2], handshake_buf[3]
        )));
    }

    // Perform version negotiation BEFORE creating BoltConnection
    let handshake = Handshake::with_versions(config.supported_versions.clone());
    let handshake_result = handshake.process(&handshake_buf);

    // Send version response (4 bytes)
    let response_bytes = Handshake::generate_response(&handshake_result);
    stream.write_all(&response_bytes).await?;

    // Check if negotiation succeeded
    let version = match handshake_result {
        Ok(result) => result.version,
        Err(e) => {
            stats.handshake_failures.fetch_add(1, Ordering::Relaxed);
            return Err(BoltError::Handshake(e));
        }
    };

    debug!("Connection {} using Bolt {:?}", addr, version);

    // Create session and initialize state machine
    let session = sessions.create_session();
    {
        let mut s = session.write();
        // Transition through handshake states (normally done in handler.handshake())
        s.state_machine_mut()
            .transition(BoltEvent::HandshakeReceived)?;
        s.state_machine_mut()
            .transition(BoltEvent::VersionNegotiated(version))?;
        s.set_version(version);
    }

    // Create connection handler with executor (stream moves here)
    let mut handler = BoltConnection::new(stream, Arc::clone(&config), Arc::clone(&session))
        .with_client_address(addr.ip().to_string());
    if let Some(exec) = executor {
        handler = handler.with_executor(exec);
    }
    if let Some(sm) = security_manager {
        handler = handler.with_security_manager(sm);
    }
    if let Some(service) = credential_security {
        handler = handler.with_credential_security(service);
    }
    if let Some(tokens) = credential_tokens {
        handler = handler.with_credential_tokens(tokens);
    }
    if let Some(manager) = tenant_db_manager {
        handler = handler.with_tenant_db_manager(manager);
    }

    // Run message loop
    handler.run().await?;

    // Clean up session
    let session_id = session.read().id();
    sessions.remove_session(session_id);

    Ok(())
}

/// Builder for BoltServer.
pub struct BoltServerBuilder {
    config: BoltConfig,
    executor: Option<Arc<dyn AsyncQueryExecutor>>,
    shutdown_timeout: Duration,
}

impl BoltServerBuilder {
    /// Create a new builder.
    pub fn new() -> Self {
        Self {
            config: BoltConfig::default(),
            executor: None,
            shutdown_timeout: Duration::from_secs(DEFAULT_SHUTDOWN_TIMEOUT_SECS),
        }
    }

    /// Set configuration.
    pub fn config(mut self, config: BoltConfig) -> Self {
        self.config = config;
        self
    }

    /// Set bind address.
    pub fn bind_address(mut self, addr: &str) -> Self {
        if let Ok(socket_addr) = addr.parse() {
            self.config.bind_addr = socket_addr;
        }
        self
    }

    /// Set max connections.
    pub fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max;
        self
    }

    /// Set connection timeout.
    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.config.connection_timeout = timeout;
        self
    }

    /// Set shutdown timeout for graceful shutdown.
    pub fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    /// Set the query executor.
    pub fn executor(mut self, executor: Arc<dyn AsyncQueryExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Build the server.
    pub fn build(self) -> BoltServer {
        let mut server = match self.executor {
            Some(exec) => BoltServer::with_executor(self.config, exec),
            None => BoltServer::new(self.config),
        };
        server.set_shutdown_timeout(self.shutdown_timeout);
        server
    }
}

impl Default for BoltServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}
