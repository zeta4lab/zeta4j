//! Server runtime management

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::ConnectInfo;
use axum::extract::connect_info::Connected;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::serve::{IncomingStream, Listener};
use log::{error, info, warn};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;
use tonic::transport::Server as TonicServer;
use tonic::transport::server::Connected as TonicConnected;
use zeta4j_cli_grpc::AdminServiceServer;
use zeta4j_server::server::grpc::{AdminGrpcService, GrpcAuthInterceptor, GrpcAuthLayer};

use super::RuntimeTlsConfig;
use super::banner::StartupInfo;
use super::server::ServerComponents;
use zeta4j_common::Edition;
use zeta4j_common::defaults::{
    DEFAULT_GRPC_CONCURRENCY_PER_CONNECTION, DEFAULT_GRPC_MAX_MESSAGE_SIZE,
    DEFAULT_GRPC_TIMEOUT_SECS, DEFAULT_TLS_HANDSHAKE_CONCURRENCY,
    DEFAULT_TLS_HANDSHAKE_TIMEOUT_SECS,
};

/// Server runtime manager
pub struct ServerRuntime {
    edition: Edition,
    components: ServerComponents,
    tls_config: Option<RuntimeTlsConfig>,
}

struct TlsListener {
    listener: TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
    handshake_timeout: Duration,
    handshake_concurrency: usize,
    handshakes: JoinSet<Option<(AdmittedIo<HttpTlsStream>, TlsPeerInfo)>>,
    permits: Arc<Semaphore>,
    idle_timeout: Duration,
}

/// Listener wrapper that stops accepting above the configured connection ceiling.
struct ConnectionAdmissionListener<L> {
    inner: L,
    permits: Arc<Semaphore>,
    idle_timeout: Duration,
}

impl<L> ConnectionAdmissionListener<L> {
    fn new(inner: L, max_connections: usize, idle_timeout: Duration) -> Self {
        Self {
            inner,
            permits: Arc::new(Semaphore::new(max_connections)),
            idle_timeout,
        }
    }
}

/// Keeps the connection permit alive for the exact lifetime of the accepted stream.
struct AdmittedIo<I> {
    inner: I,
    _permit: OwnedSemaphorePermit,
    idle_timeout: Duration,
    idle: Pin<Box<tokio::time::Sleep>>,
}

impl<I> AdmittedIo<I> {
    fn new(inner: I, permit: OwnedSemaphorePermit, idle_timeout: Duration) -> Self {
        Self {
            inner,
            _permit: permit,
            idle_timeout,
            idle: Box::pin(tokio::time::sleep(idle_timeout)),
        }
    }

    fn reset_idle(&mut self) {
        self.idle
            .as_mut()
            .reset(tokio::time::Instant::now() + self.idle_timeout);
    }

    fn poll_idle(&mut self, context: &mut Context<'_>) -> std::io::Result<()> {
        if self.idle.as_mut().poll(context).is_ready() {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "HTTP connection idle timeout reached",
            ))
        } else {
            Ok(())
        }
    }
}

impl<I> AsyncRead for AdmittedIo<I>
where
    I: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Err(error) = self.poll_idle(context) {
            return Poll::Ready(Err(error));
        }
        let previous = buffer.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(context, buffer);
        if matches!(result, Poll::Ready(Ok(()))) && buffer.filled().len() > previous {
            self.reset_idle();
        }
        result
    }
}

impl<I> AsyncWrite for AdmittedIo<I>
where
    I: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Err(error) = self.poll_idle(context) {
            return Poll::Ready(Err(error));
        }
        let result = Pin::new(&mut self.inner).poll_write(context, buffer);
        if matches!(result, Poll::Ready(Ok(written)) if written > 0) {
            self.reset_idle();
        }
        result
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

impl<L> Listener for ConnectionAdmissionListener<L>
where
    L: Listener,
{
    type Io = AdmittedIo<L::Io>;
    type Addr = L::Addr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let Ok(permit) = self.permits.clone().acquire_owned().await else {
                tokio::task::yield_now().await;
                continue;
            };
            let (stream, address) = self.inner.accept().await;
            return (AdmittedIo::new(stream, permit, self.idle_timeout), address);
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

type HttpTlsStream = tokio_rustls::server::TlsStream<TcpStream>;

struct GrpcTlsStream(tokio_rustls::server::TlsStream<TcpStream>);

impl AsyncRead for GrpcTlsStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(context, buffer)
    }
}

impl AsyncWrite for GrpcTlsStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(context, buffer)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(context)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(context)
    }
}

impl TonicConnected for GrpcTlsStream {
    type ConnectInfo = ();

    fn connect_info(&self) -> Self::ConnectInfo {}
}

#[derive(Debug, Clone)]
struct TlsPeerInfo {
    remote_addr: SocketAddr,
}

impl Listener for TlsListener {
    type Io = AdmittedIo<HttpTlsStream>;
    type Addr = TlsPeerInfo;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            if self.handshakes.len() >= self.handshake_concurrency {
                if let Some(completed) = self.next_completed_handshake().await {
                    return completed;
                }
                continue;
            }

            let permit = tokio::select! {
                completed = self.handshakes.join_next(), if !self.handshakes.is_empty() => {
                    if let Some(completed) = completed_http_handshake(completed) {
                        return completed;
                    }
                    continue;
                }
                permit = self.permits.clone().acquire_owned() => {
                    let Ok(permit) = permit else {
                        tokio::task::yield_now().await;
                        continue;
                    };
                    permit
                }
            };

            tokio::select! {
                completed = self.handshakes.join_next(), if !self.handshakes.is_empty() => {
                    drop(permit);
                    if let Some(completed) = completed_http_handshake(completed) {
                        return completed;
                    }
                }
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, remote_addr)) => {
                        self.spawn_handshake(stream, remote_addr, permit);
                    }
                    Err(error) => warn!("TLS accept failed: {error}"),
                },
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.listener
            .local_addr()
            .map(|remote_addr| TlsPeerInfo { remote_addr })
    }
}

impl TlsListener {
    fn spawn_handshake(
        &mut self,
        stream: TcpStream,
        remote_addr: SocketAddr,
        permit: OwnedSemaphorePermit,
    ) {
        let acceptor = self.acceptor.clone();
        let handshake_timeout = self.handshake_timeout;
        let idle_timeout = self.idle_timeout;
        let _ = self.handshakes.spawn(async move {
            let stream =
                match tokio::time::timeout(handshake_timeout, acceptor.accept(stream)).await {
                    Ok(Ok(stream)) => stream,
                    Ok(Err(_)) => {
                        warn!("[SV-TLS-003] TLS handshake rejected");
                        return None;
                    }
                    Err(_) => {
                        warn!("[SV-TLS-003] TLS handshake timed out");
                        return None;
                    }
                };
            Some((
                AdmittedIo::new(stream, permit, idle_timeout),
                TlsPeerInfo { remote_addr },
            ))
        });
    }

    async fn next_completed_handshake(
        &mut self,
    ) -> Option<(AdmittedIo<HttpTlsStream>, TlsPeerInfo)> {
        loop {
            let completed = self.handshakes.join_next().await;
            if let Some(completed) = completed_http_handshake(completed) {
                return Some(completed);
            }
            if self.handshakes.is_empty() {
                return None;
            }
        }
    }
}

fn completed_http_handshake(
    completed: Option<
        Result<Option<(AdmittedIo<HttpTlsStream>, TlsPeerInfo)>, tokio::task::JoinError>,
    >,
) -> Option<(AdmittedIo<HttpTlsStream>, TlsPeerInfo)> {
    match completed {
        Some(Ok(completed)) => completed,
        Some(Err(error)) => {
            warn!("TLS handshake task failed: {error}");
            None
        }
        None => None,
    }
}

impl Connected<IncomingStream<'_, TlsListener>> for TlsPeerInfo {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        stream.remote_addr().clone()
    }
}

async fn attach_remote_addr(
    ConnectInfo(info): ConnectInfo<TlsPeerInfo>,
    mut request: axum::extract::Request,
    next: Next,
) -> Response {
    request.extensions_mut().insert(info.remote_addr);
    next.run(request).await
}

impl ServerRuntime {
    /// Create a new ServerRuntime
    pub fn new(
        edition: Edition,
        components: ServerComponents,
        tls_config: Option<RuntimeTlsConfig>,
    ) -> Self {
        Self {
            edition,
            components,
            tls_config,
        }
    }

    /// Check if HTTPS is enabled
    pub fn https_enabled(&self) -> bool {
        self.tls_config.is_some()
    }

    /// Run the server until shutdown
    pub async fn run(mut self, data_dir: &str, log_dir: &str) -> Result<(), RuntimeError> {
        // Bind every product socket before starting any task. Bolt was bound by
        // the assembly phase; these two binds complete the all-or-nothing gate.
        let mut http_listener = Some(
            TcpListener::bind(self.components.http_addr)
                .await
                .map_err(|error| RuntimeError::BindFailed(format!("HTTP: {error}")))?,
        );
        let grpc_listener = TcpListener::bind(self.components.grpc_addr)
            .await
            .map_err(|error| RuntimeError::BindFailed(format!("gRPC: {error}")))?;

        // Print startup info
        let mut startup_info = StartupInfo::new(self.edition);
        startup_info.http_addr = (!self.https_enabled())
            .then(|| {
                http_listener
                    .as_ref()
                    .and_then(|listener| listener.local_addr().ok())
            })
            .flatten();
        startup_info.secure_http_addr = if self.https_enabled() {
            Some(self.components.http_addr)
        } else {
            None
        };
        startup_info.bolt_addr = self.components.bolt_addr;
        startup_info.grpc_addr = Some(self.components.grpc_addr);
        startup_info.data_dir = data_dir.to_string();
        startup_info.log_dir = log_dir.to_string();
        startup_info.print();

        // Shutdown token
        let shutdown_token = tokio_util::sync::CancellationToken::new();
        // HTTP server task (only if HTTPS is disabled)
        let http_task = if !self.https_enabled() {
            let listener = http_listener.take().ok_or_else(|| {
                RuntimeError::BindFailed("HTTP listener preflight state is invalid".to_string())
            })?;
            let http_app = self.components.app.clone();
            let http_shutdown = shutdown_token.clone();
            Some(tokio::spawn(async move {
                let shutdown_signal = async move {
                    http_shutdown.cancelled().await;
                    info!("Shutdown signal received, stopping HTTP server...");
                };

                let listener = ConnectionAdmissionListener::new(
                    listener,
                    self.components.http_max_connections,
                    self.components.http_keep_alive_timeout,
                );
                axum::serve(listener, http_app)
                    .with_graceful_shutdown(shutdown_signal)
                    .await
            }))
        } else {
            None
        };

        // HTTPS server task (if TLS configured)
        let grpc_tls_config = self.tls_config.as_ref().map(|config| config.grpc.clone());
        let https_task = if let Some(tls_config) = self.tls_config.take() {
            let listener = http_listener.take().ok_or_else(|| {
                RuntimeError::BindFailed("HTTPS listener preflight state is invalid".to_string())
            })?;
            let https_app = self
                .components
                .app
                .clone()
                .layer(middleware::from_fn(attach_remote_addr));
            let https_shutdown = shutdown_token.clone();
            Some(tokio::spawn(async move {
                let listener = TlsListener {
                    listener,
                    acceptor: tokio_rustls::TlsAcceptor::from(tls_config.http),
                    handshake_timeout: Duration::from_secs(DEFAULT_TLS_HANDSHAKE_TIMEOUT_SECS),
                    handshake_concurrency: DEFAULT_TLS_HANDSHAKE_CONCURRENCY
                        .min(self.components.http_max_connections),
                    handshakes: JoinSet::new(),
                    permits: Arc::new(Semaphore::new(self.components.http_max_connections)),
                    idle_timeout: self.components.http_keep_alive_timeout,
                };
                let shutdown = async move { https_shutdown.cancelled().await };
                if let Err(error) = axum::serve(
                    listener,
                    https_app.into_make_service_with_connect_info::<TlsPeerInfo>(),
                )
                .with_graceful_shutdown(shutdown)
                .await
                {
                    error!("HTTPS server error: {error}");
                }
            }))
        } else {
            None
        };

        let bolt_stats = self.components.bolt_server.shared_stats();
        let admin_config_content = self.components.admin_config_content.clone();

        // Bolt server task
        let bolt_handle = tokio::spawn(async move {
            if let Err(e) = self.components.bolt_server.run().await {
                error!("Bolt server error: {}", e);
            }
        });

        // gRPC server task
        let grpc_handle = {
            let grpc_addr = self.components.grpc_addr;
            let edition = self.components.edition;
            let grpc_shutdown = shutdown_token.clone();
            let mut admin_grpc_service = AdminGrpcService::with_db_manager(
                self.components.system_db_service.clone(),
                self.components.db_manager.clone(),
            )
            .with_edition(edition)
            .with_runtime(admin_config_content, bolt_stats);
            if let (Some(credential_security), Some(credential_tokens)) = (
                &self.components.credential_security,
                &self.components.credential_tokens,
            ) {
                admin_grpc_service = admin_grpc_service
                    .with_credentials(credential_security.clone(), credential_tokens.clone());
            }

            // gRPC 인증 인터셉터 설정 (EffectiveConfig 기반, 단일 설정 소스)
            let grpc_auth_enabled = self.components.grpc_auth_enabled;
            let grpc_interceptor = if grpc_auth_enabled {
                let credential_security =
                    self.components.credential_security.clone().ok_or_else(|| {
                        RuntimeError::CredentialConfiguration(
                            "canonical credential service is missing".to_string(),
                        )
                    })?;
                let credential_tokens =
                    self.components.credential_tokens.clone().ok_or_else(|| {
                        RuntimeError::CredentialConfiguration(
                            "canonical credential verifier is missing".to_string(),
                        )
                    })?;

                info!("gRPC authentication enabled (canonical credential service)");
                let interceptor =
                    GrpcAuthInterceptor::with_credentials(credential_security, credential_tokens);
                if let Some(security_manager) = self.components.security_manager.clone() {
                    interceptor.with_security_manager(security_manager)
                } else {
                    interceptor
                }
            } else {
                info!("gRPC authentication disabled (--no-auth mode)");
                GrpcAuthInterceptor::disabled()
            };
            let grpc_auth = GrpcAuthLayer::new(grpc_interceptor);

            Some(tokio::spawn(async move {
                info!(
                    "Starting gRPC server on {} (edition: {:?})",
                    grpc_addr, edition
                );

                let admin_service = grpc_auth.protect(
                    AdminServiceServer::new(admin_grpc_service)
                        .max_encoding_message_size(DEFAULT_GRPC_MAX_MESSAGE_SIZE)
                        .max_decoding_message_size(DEFAULT_GRPC_MAX_MESSAGE_SIZE),
                );
                let mut builder = public_grpc_server_builder();

                let router = builder.add_service(admin_service);

                let result = if let Some(tls_config) = grpc_tls_config {
                    let acceptor = tokio_rustls::TlsAcceptor::from(tls_config);
                    let incoming = async_stream::stream! {
                        let mut handshakes = JoinSet::new();
                        loop {
                            if handshakes.len() >= DEFAULT_TLS_HANDSHAKE_CONCURRENCY {
                                match handshakes.join_next().await {
                                    Some(Ok(Some(stream))) => {
                                        yield Ok::<_, std::io::Error>(stream);
                                    }
                                    Some(Ok(None)) => {}
                                    Some(Err(error)) => warn!("gRPC TLS handshake task failed: {error}"),
                                    None => {}
                                }
                                continue;
                            }
                            tokio::select! {
                                completed = handshakes.join_next(), if !handshakes.is_empty() => {
                                    match completed {
                                        Some(Ok(Some(stream))) => {
                                            yield Ok::<_, std::io::Error>(stream);
                                        }
                                        Some(Ok(None)) => {}
                                        Some(Err(error)) => warn!("gRPC TLS handshake task failed: {error}"),
                                        None => {}
                                    }
                                }
                                accepted = grpc_listener.accept() => match accepted {
                                    Ok((stream, _)) => {
                                        let acceptor = acceptor.clone();
                                        let _ = handshakes.spawn(async move {
                                            match tokio::time::timeout(
                                                Duration::from_secs(DEFAULT_TLS_HANDSHAKE_TIMEOUT_SECS),
                                                acceptor.accept(stream),
                                            ).await {
                                                Ok(Ok(stream)) => Some(GrpcTlsStream(stream)),
                                                Ok(Err(_)) => {
                                                    warn!("[SV-TLS-003] gRPC TLS handshake rejected");
                                                    None
                                                }
                                                Err(_) => {
                                                    warn!("[SV-TLS-003] gRPC TLS handshake timed out");
                                                    None
                                                }
                                            }
                                        });
                                    }
                                    Err(error) => yield Err(error),
                                },
                            }
                        }
                    };
                    router
                        .serve_with_incoming_shutdown(incoming, async move {
                            grpc_shutdown.cancelled().await;
                            info!("Shutdown signal received, stopping gRPC server...");
                        })
                        .await
                } else {
                    let incoming = async_stream::stream! {
                        loop {
                            yield grpc_listener.accept().await.map(|(stream, _)| stream);
                        }
                    };
                    router
                        .serve_with_incoming_shutdown(incoming, async move {
                            grpc_shutdown.cancelled().await;
                            info!("Shutdown signal received, stopping gRPC server...");
                        })
                        .await
                };

                if let Err(e) = result {
                    error!("gRPC server error: {}", e);
                }
            }))
        };

        // Signal handler (SIGINT + SIGTERM)
        let signal_shutdown = shutdown_token.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                match signal(SignalKind::terminate()) {
                    Ok(mut sigterm) => {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {
                                info!("Received SIGINT (Ctrl+C)");
                            }
                            _ = sigterm.recv() => {
                                info!("Received SIGTERM");
                            }
                        }
                    }
                    Err(error) => {
                        error!("Failed to install SIGTERM handler: {}", error);
                        if let Err(error) = tokio::signal::ctrl_c().await {
                            error!("Failed to install SIGINT handler: {}", error);
                        }
                    }
                }
            }
            #[cfg(not(unix))]
            {
                if let Err(e) = tokio::signal::ctrl_c().await {
                    error!("Failed to install CTRL+C handler: {}", e);
                }
                info!("Received shutdown signal");
            }
            signal_shutdown.cancel();
        });

        // Wait for server shutdown
        tokio::select! {
            result = async {
                if let Some(task) = http_task {
                    task.await
                } else {
                    std::future::pending().await
                }
            } => {
                if let Err(e) = result {
                    error!("HTTP server task error: {}", e);
                }
                shutdown_token.cancel();
            }
            result = async {
                if let Some(task) = https_task {
                    task.await
                } else {
                    std::future::pending().await
                }
            } => {
                if let Err(e) = result {
                    error!("HTTPS server task error: {}", e);
                }
                shutdown_token.cancel();
            }
            result = bolt_handle => {
                if let Err(e) = result {
                    error!("Bolt server task error: {}", e);
                }
                shutdown_token.cancel();
            }
            result = async {
                if let Some(task) = grpc_handle {
                    task.await
                } else {
                    std::future::pending::<Result<(), tokio::task::JoinError>>().await
                }
            } => {
                if let Err(e) = result {
                    error!("gRPC server task error: {}", e);
                }
                shutdown_token.cancel();
            }
        }

        // === Graceful Shutdown: GraphEngine checkpoint ===
        info!("Starting graceful shutdown sequence...");
        let engine = self.components.graph_engine.clone();
        match tokio::time::timeout(Duration::from_secs(60), engine.shutdown()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => warn!("GraphEngine shutdown checkpoint failed: {}", e),
            Err(_) => warn!("GraphEngine shutdown checkpoint timed out (60s)"),
        }

        info!("Server stopped gracefully");
        Ok(())
    }
}

fn public_grpc_server_builder() -> tonic::transport::Server {
    TonicServer::builder()
        .concurrency_limit_per_connection(DEFAULT_GRPC_CONCURRENCY_PER_CONNECTION)
        .timeout(Duration::from_secs(DEFAULT_GRPC_TIMEOUT_SECS))
}

/// Runtime errors
#[derive(Debug)]
pub enum RuntimeError {
    /// Bind failed
    BindFailed(String),
    /// Canonical credential runtime is missing or inconsistent.
    CredentialConfiguration(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::BindFailed(msg) => write!(f, "Failed to bind: {}", msg),
            RuntimeError::CredentialConfiguration(message) => {
                write!(f, "Credential runtime unavailable: {message}")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
    use tokio::io::AsyncReadExt;
    use tokio_rustls::TlsConnector;

    #[tokio::test]
    async fn connection_admission_waits_until_a_stream_releases_its_permit()
    -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let mut admitted = ConnectionAdmissionListener::new(listener, 1, Duration::from_secs(30));

        let _first_client = TcpStream::connect(address).await?;
        let (first_server, _) = admitted.accept().await;
        let _second_client = TcpStream::connect(address).await?;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), admitted.accept())
                .await
                .is_err()
        );

        drop(first_server);
        let accepted = tokio::time::timeout(Duration::from_secs(1), admitted.accept()).await;
        assert!(accepted.is_ok());
        Ok(())
    }

    #[tokio::test]
    async fn admitted_stream_closes_after_idle_timeout() -> Result<(), Box<dyn std::error::Error>> {
        let permits = Arc::new(Semaphore::new(1));
        let permit = permits.acquire_owned().await?;
        let (server, _client) = tokio::io::duplex(64);
        let mut admitted = AdmittedIo::new(server, permit, Duration::from_millis(10));
        tokio::time::sleep(Duration::from_millis(20)).await;

        let mut byte = [0_u8; 1];
        let error = admitted
            .read(&mut byte)
            .await
            .err()
            .ok_or("idle admitted stream unexpectedly remained readable")?;
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        Ok(())
    }

    #[tokio::test]
    async fn slow_tls_handshake_does_not_block_a_ready_client()
    -> Result<(), Box<dyn std::error::Error>> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let certified = generate_simple_self_signed(vec!["localhost".to_owned()])?;
        let certificate = certified.cert.der().clone();
        let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            certified.signing_key.serialize_der(),
        ));
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate.clone()], private_key)?;
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate)?;
        let client_config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let mut listener = TlsListener {
            listener,
            acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(server_config)),
            handshake_timeout: Duration::from_secs(5),
            handshake_concurrency: 2,
            handshakes: JoinSet::new(),
            permits: Arc::new(Semaphore::new(2)),
            idle_timeout: Duration::from_secs(30),
        };
        let accept = tokio::spawn(async move { listener.accept().await });

        let _slow_client = TcpStream::connect(address).await?;
        let ready_tcp = TcpStream::connect(address).await?;
        let connector = TlsConnector::from(Arc::new(client_config));
        let server_name = ServerName::try_from("localhost")?;
        let ready = tokio::time::timeout(
            Duration::from_secs(1),
            connector.connect(server_name, ready_tcp),
        )
        .await??;
        let accepted = tokio::time::timeout(Duration::from_secs(1), accept).await??;

        drop(ready);
        drop(accepted);
        Ok(())
    }
}
