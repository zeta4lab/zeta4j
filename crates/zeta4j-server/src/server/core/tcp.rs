//! TCP Server

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

use super::backpressure::BackpressureController;
use super::connection::{Connection, ConnectionId, ConnectionTracker};
use super::shutdown::{ShutdownController, ShutdownReason};
use crate::server::common::config::ServerConfig;
use crate::server::common::error::ServerError;
use crate::server::monitor::server_metrics::ServerMetrics;

/// 서버 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ServerState {
    /// 생성됨
    Created = 0,
    /// 시작 중
    Starting = 1,
    /// 실행 중
    Running = 2,
    /// 종료 중
    ShuttingDown = 3,
    /// 종료됨
    Stopped = 4,
}

impl std::fmt::Display for ServerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "Created"),
            Self::Starting => write!(f, "Starting"),
            Self::Running => write!(f, "Running"),
            Self::ShuttingDown => write!(f, "ShuttingDown"),
            Self::Stopped => write!(f, "Stopped"),
        }
    }
}

/// Atomic 서버 상태
struct AtomicServerState(AtomicU8);

impl AtomicServerState {
    fn new(state: ServerState) -> Self {
        Self(AtomicU8::new(state as u8))
    }

    fn get(&self) -> ServerState {
        match self.0.load(Ordering::SeqCst) {
            0 => ServerState::Created,
            1 => ServerState::Starting,
            2 => ServerState::Running,
            3 => ServerState::ShuttingDown,
            4 => ServerState::Stopped,
            _ => ServerState::Stopped,
        }
    }

    fn set(&self, state: ServerState) {
        self.0.store(state as u8, Ordering::SeqCst);
    }

    fn transition(&self, from: ServerState, to: ServerState) -> bool {
        self.0
            .compare_exchange(from as u8, to as u8, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }
}

/// TCP 서버
///
/// 고성능 비동기 TCP 서버로, 다음 기능을 제공합니다:
/// - 멀티 스레드 accept (SO_REUSEPORT)
/// - 연결 풀링 및 제한
/// - 백프레셔 제어
/// - Graceful shutdown
/// - 메트릭 수집
pub struct TcpServer {
    config: Arc<ServerConfig>,
    listeners: Vec<TcpListener>,
    shutdown_tx: broadcast::Sender<()>,
    connection_tracker: Arc<ConnectionTracker>,
    backpressure: Arc<BackpressureController>,
    metrics: Arc<ServerMetrics>,
    shutdown_controller: Arc<ShutdownController>,
    state: Arc<AtomicServerState>,
    started_at: Option<Instant>,
}

impl TcpServer {
    /// 서버 생성 및 바인딩
    pub async fn bind(config: ServerConfig) -> Result<Self, ServerError> {
        config.validate()?;

        let (shutdown_tx, _) = broadcast::channel(1);

        // 메인 리스너 생성
        let main_listener = Self::create_listener(&config, config.bind_addr).await?;
        let mut listeners = vec![main_listener];

        // 추가 주소 바인딩
        for addr in &config.additional_addrs {
            let listener = Self::create_listener(&config, *addr).await?;
            listeners.push(listener);
        }

        let config = Arc::new(config);

        let shutdown_controller = Arc::new(ShutdownController::new(
            config.shutdown_timeout,
            config.force_shutdown_timeout,
        ));

        Ok(Self {
            config: config.clone(),
            listeners,
            shutdown_tx,
            connection_tracker: Arc::new(ConnectionTracker::new(config.max_connections)),
            backpressure: Arc::new(BackpressureController::new(&config)),
            metrics: Arc::new(ServerMetrics::new()),
            shutdown_controller,
            state: Arc::new(AtomicServerState::new(ServerState::Created)),
            started_at: None,
        })
    }

    /// 리스너 생성
    async fn create_listener(
        config: &ServerConfig,
        addr: SocketAddr,
    ) -> Result<TcpListener, ServerError> {
        let socket = if addr.is_ipv6() {
            tokio::net::TcpSocket::new_v6()?
        } else {
            tokio::net::TcpSocket::new_v4()?
        };

        // 소켓 옵션 설정
        if config.reuse_addr {
            socket.set_reuseaddr(true)?;
        }

        #[cfg(target_os = "linux")]
        if config.reuse_port {
            socket.set_reuseport(true)?;
        }

        socket
            .bind(addr)
            .map_err(|e| ServerError::BindFailed(format!("Failed to bind to {}: {}", addr, e)))?;

        let listener = socket.listen(config.backlog)?;

        tracing::info!("TCP server bound to {}", addr);

        Ok(listener)
    }

    /// 서버 실행
    pub async fn run(mut self) -> Result<(), ServerError> {
        if !self
            .state
            .transition(ServerState::Created, ServerState::Starting)
        {
            return Err(ServerError::AlreadyRunning);
        }

        self.started_at = Some(Instant::now());

        tracing::info!(
            "Starting TCP server on {} with {} max connections",
            self.config.bind_addr,
            self.config.max_connections
        );

        // Acceptor 태스크 생성
        let mut accept_handles = Vec::new();

        for (i, listener) in self.listeners.into_iter().enumerate() {
            let shutdown_rx = self.shutdown_tx.subscribe();
            let tracker = Arc::clone(&self.connection_tracker);
            let backpressure = Arc::clone(&self.backpressure);
            let metrics = Arc::clone(&self.metrics);
            let config = Arc::clone(&self.config);
            let state = Arc::clone(&self.state);

            let handle = tokio::spawn(async move {
                Self::accept_loop(
                    i,
                    listener,
                    shutdown_rx,
                    tracker,
                    backpressure,
                    metrics,
                    config,
                    state,
                )
                .await
            });

            accept_handles.push(handle);
        }

        self.state.set(ServerState::Running);
        tracing::info!("TCP server is now running");

        // 시그널 핸들러 및 shutdown 대기
        let shutdown_controller = Arc::clone(&self.shutdown_controller);
        let shutdown_tx = self.shutdown_tx.clone();

        tokio::spawn(async move {
            shutdown_controller.wait_for_signal().await;
            let _ = shutdown_tx.send(());
        });

        // Acceptor 종료 대기
        for handle in accept_handles {
            let _ = handle.await;
        }

        self.state.set(ServerState::Stopped);
        tracing::info!("TCP server stopped");

        Ok(())
    }

    /// Accept 루프
    #[expect(clippy::too_many_arguments)]
    async fn accept_loop(
        listener_idx: usize,
        listener: TcpListener,
        mut shutdown_rx: broadcast::Receiver<()>,
        tracker: Arc<ConnectionTracker>,
        backpressure: Arc<BackpressureController>,
        metrics: Arc<ServerMetrics>,
        config: Arc<ServerConfig>,
        state: Arc<AtomicServerState>,
    ) {
        tracing::debug!("Acceptor {} started", listener_idx);

        loop {
            tokio::select! {
                biased;

                // Shutdown 시그널
                _ = shutdown_rx.recv() => {
                    tracing::debug!("Acceptor {} shutting down", listener_idx);
                    break;
                }

                // 새 연결 수락
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer_addr)) => {
                            Self::handle_new_connection(
                                stream,
                                peer_addr,
                                &tracker,
                                &backpressure,
                                &metrics,
                                &config,
                                &state,
                            ).await;
                        }
                        Err(e) => {
                            Self::handle_accept_error(e, &metrics).await;
                        }
                    }
                }
            }
        }

        tracing::debug!("Acceptor {} stopped", listener_idx);
    }

    /// 새 연결 처리
    async fn handle_new_connection(
        stream: TcpStream,
        peer_addr: SocketAddr,
        tracker: &Arc<ConnectionTracker>,
        backpressure: &Arc<BackpressureController>,
        metrics: &Arc<ServerMetrics>,
        config: &Arc<ServerConfig>,
        state: &Arc<AtomicServerState>,
    ) {
        // 서버 상태 체크
        if state.get() != ServerState::Running {
            return;
        }

        // 메트릭 업데이트
        metrics.connections_accepted.inc();

        // 백프레셔 체크
        if !backpressure.try_acquire() {
            metrics.connections_rejected_backpressure.inc();
            tracing::warn!("Connection rejected due to backpressure: {}", peer_addr);
            return;
        }

        // IP별 연결 제한 체크
        if config.max_connections_per_ip > 0
            && !tracker.check_per_ip_limit(&peer_addr.ip(), config.max_connections_per_ip)
        {
            metrics.connections_rejected_per_ip.inc();
            tracing::warn!("Connection rejected (per-IP limit): {}", peer_addr);
            backpressure.release();
            return;
        }

        // 연결 등록 시도
        match tracker.try_add(peer_addr) {
            Ok(conn_id) => {
                // TCP 옵션 설정
                if let Err(e) = Self::configure_stream(&stream, config) {
                    tracing::error!("Failed to configure stream: {}", e);
                    tracker.remove(conn_id);
                    backpressure.release();
                    return;
                }

                metrics.active_connections.inc();

                // 연결 핸들러 스폰
                let tracker = Arc::clone(tracker);
                let backpressure = Arc::clone(backpressure);
                let metrics = Arc::clone(metrics);
                let config = Arc::clone(config);

                tokio::spawn(async move {
                    Self::handle_connection(
                        conn_id,
                        stream,
                        peer_addr,
                        tracker,
                        backpressure,
                        metrics,
                        config,
                    )
                    .await;
                });
            }
            Err(_) => {
                metrics.connections_rejected_max.inc();
                tracing::warn!("Connection rejected (max connections): {}", peer_addr);
                backpressure.release();
            }
        }
    }

    /// 연결 핸들러
    async fn handle_connection(
        conn_id: ConnectionId,
        stream: TcpStream,
        peer_addr: SocketAddr,
        tracker: Arc<ConnectionTracker>,
        backpressure: Arc<BackpressureController>,
        metrics: Arc<ServerMetrics>,
        config: Arc<ServerConfig>,
    ) {
        tracing::debug!("Handling connection {} from {}", conn_id, peer_addr);

        let _connection = Connection::new(conn_id, stream, peer_addr);

        // 유휴 타임아웃까지 대기
        // 실제 프로토콜 핸들러(Bolt, HTTP 등)에서 오버라이드
        tokio::time::sleep(config.idle_timeout).await;

        // 정리
        tracker.remove(conn_id);
        backpressure.release();
        metrics.active_connections.dec();
        metrics.connections_closed_normal.inc();

        tracing::debug!("Connection {} closed", conn_id);
    }

    /// TCP 스트림 설정
    fn configure_stream(stream: &TcpStream, config: &ServerConfig) -> Result<(), std::io::Error> {
        stream.set_nodelay(config.tcp_nodelay)?;
        Ok(())
    }

    /// Accept 에러 처리
    async fn handle_accept_error(error: std::io::Error, metrics: &Arc<ServerMetrics>) {
        use std::io::ErrorKind;

        match error.kind() {
            // 일시적 에러 - 계속 시도
            ErrorKind::WouldBlock | ErrorKind::Interrupted => {}

            // 리소스 부족 - 잠시 대기
            ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted => {
                metrics.accept_errors.inc();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }

            // 기타 에러
            _ => {
                #[cfg(unix)]
                {
                    // EMFILE/ENFILE - 파일 디스크립터 부족
                    if error.raw_os_error() == Some(libc::EMFILE)
                        || error.raw_os_error() == Some(libc::ENFILE)
                    {
                        tracing::error!("File descriptor limit reached, waiting...");
                        metrics.accept_errors_fd_limit.inc();
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        return;
                    }
                }

                tracing::error!("Accept error: {}", error);
                metrics.accept_errors.inc();
            }
        }
    }

    /// Graceful shutdown 시작
    pub async fn shutdown(&self) -> Result<(), ServerError> {
        if !self
            .state
            .transition(ServerState::Running, ServerState::ShuttingDown)
        {
            return Err(ServerError::NotRunning);
        }

        tracing::info!("Initiating graceful shutdown...");

        // Shutdown 시그널 전송
        self.shutdown_controller.trigger(ShutdownReason::Requested);
        let _ = self.shutdown_tx.send(());

        // 기존 연결 드레인 대기
        let tracker = Arc::clone(&self.connection_tracker);
        let drained = self
            .shutdown_controller
            .wait_for_drain(|| async { tracker.count() == 0 })
            .await;

        if drained {
            tracing::info!("All connections drained successfully");
        } else {
            let remaining = self.connection_tracker.count();
            tracing::warn!("Shutdown timeout, {} connections remaining", remaining);

            // 강제 종료
            self.connection_tracker.force_close_all().await;
        }

        self.state.set(ServerState::Stopped);
        self.shutdown_controller.terminate();

        Ok(())
    }

    /// 서버 상태 조회
    pub fn state(&self) -> ServerState {
        self.state.get()
    }

    /// 현재 연결 수 조회
    pub fn connection_count(&self) -> usize {
        self.connection_tracker.count()
    }

    /// 메트릭 스냅샷
    pub fn metrics(&self) -> &Arc<ServerMetrics> {
        &self.metrics
    }

    /// 연결 추적기
    pub fn connection_tracker(&self) -> &Arc<ConnectionTracker> {
        &self.connection_tracker
    }

    /// 백프레셔 컨트롤러
    pub fn backpressure(&self) -> &Arc<BackpressureController> {
        &self.backpressure
    }

    /// 설정
    pub fn config(&self) -> &Arc<ServerConfig> {
        &self.config
    }

    /// 서버 가동 시간
    pub fn uptime(&self) -> Option<Duration> {
        self.started_at.map(|t| t.elapsed())
    }

    /// 바인딩 주소
    pub fn bind_addr(&self) -> SocketAddr {
        self.config.bind_addr
    }
}
