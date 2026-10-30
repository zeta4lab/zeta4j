//! Keep-Alive Management
//!
//! Phase 2 Step 1 M1.3: Keep-Alive 관리
//!
//! TCP Keep-Alive 및 애플리케이션 레벨 Keep-Alive를 관리합니다.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::connection::ConnectionId;

/// Keep-Alive 설정
#[derive(Debug, Clone)]
pub struct KeepAliveConfig {
    /// Keep-Alive 활성화
    pub enabled: bool,
    /// 첫 Keep-Alive 프로브까지 유휴 시간
    pub idle_time: Duration,
    /// Keep-Alive 프로브 간격
    pub interval: Duration,
    /// 연결 종료 전 실패 프로브 수
    pub probe_count: u32,
    /// Keep-Alive 응답 타임아웃
    pub response_timeout: Duration,
}

impl Default for KeepAliveConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_time: Duration::from_secs(60),
            interval: Duration::from_secs(15),
            probe_count: 3,
            response_timeout: Duration::from_secs(5),
        }
    }
}

impl KeepAliveConfig {
    /// 빌더 생성
    pub fn builder() -> KeepAliveConfigBuilder {
        KeepAliveConfigBuilder::default()
    }

    /// 비활성화된 설정
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }
}

/// Keep-Alive 설정 빌더
#[derive(Default)]
pub struct KeepAliveConfigBuilder {
    enabled: Option<bool>,
    idle_time: Option<Duration>,
    interval: Option<Duration>,
    probe_count: Option<u32>,
    response_timeout: Option<Duration>,
}

impl KeepAliveConfigBuilder {
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    pub fn idle_time(mut self, d: Duration) -> Self {
        self.idle_time = Some(d);
        self
    }

    pub fn interval(mut self, d: Duration) -> Self {
        self.interval = Some(d);
        self
    }

    pub fn probe_count(mut self, count: u32) -> Self {
        self.probe_count = Some(count);
        self
    }

    pub fn response_timeout(mut self, d: Duration) -> Self {
        self.response_timeout = Some(d);
        self
    }

    pub fn build(self) -> KeepAliveConfig {
        let default = KeepAliveConfig::default();
        KeepAliveConfig {
            enabled: self.enabled.unwrap_or(default.enabled),
            idle_time: self.idle_time.unwrap_or(default.idle_time),
            interval: self.interval.unwrap_or(default.interval),
            probe_count: self.probe_count.unwrap_or(default.probe_count),
            response_timeout: self.response_timeout.unwrap_or(default.response_timeout),
        }
    }
}

/// Keep-Alive 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepAliveState {
    /// 유휴 상태 (활동 대기)
    Idle,
    /// 프로브 대기 중
    Probing,
    /// 응답 대기 중
    WaitingResponse,
    /// 실패 (연결 종료 필요)
    Failed,
}

impl std::fmt::Display for KeepAliveState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Probing => write!(f, "Probing"),
            Self::WaitingResponse => write!(f, "WaitingResponse"),
            Self::Failed => write!(f, "Failed"),
        }
    }
}

/// Keep-Alive 연결 정보
#[derive(Debug, Clone)]
struct KeepAliveEntry {
    conn_id: ConnectionId,
    state: KeepAliveState,
    last_activity: Instant,
    last_probe: Option<Instant>,
    failed_probes: u32,
}

impl KeepAliveEntry {
    fn new(conn_id: ConnectionId) -> Self {
        Self {
            conn_id,
            state: KeepAliveState::Idle,
            last_activity: Instant::now(),
            last_probe: None,
            failed_probes: 0,
        }
    }
}

/// Keep-Alive 이벤트
#[derive(Debug, Clone)]
pub enum KeepAliveEvent {
    /// 프로브 전송 필요
    SendProbe(ConnectionId),
    /// 연결 종료 필요 (Keep-Alive 실패)
    ConnectionDead(ConnectionId),
}

/// Keep-Alive 관리자
///
/// 연결별 Keep-Alive 상태를 추적하고 프로브를 관리합니다.
pub struct KeepAliveManager {
    config: KeepAliveConfig,
    entries: Arc<Mutex<HashMap<ConnectionId, KeepAliveEntry>>>,
    event_tx: mpsc::Sender<KeepAliveEvent>,
    event_rx: Mutex<Option<mpsc::Receiver<KeepAliveEvent>>>,
    running: std::sync::atomic::AtomicBool,
}

impl KeepAliveManager {
    /// 새 Keep-Alive 관리자 생성
    pub fn new(config: KeepAliveConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1000);

        Self {
            config,
            entries: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            event_rx: Mutex::new(Some(event_rx)),
            running: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(KeepAliveConfig::default())
    }

    /// 연결 등록
    pub fn register(&self, conn_id: ConnectionId) {
        if !self.config.enabled {
            return;
        }

        let mut entries = self.entries.lock();
        entries.insert(conn_id, KeepAliveEntry::new(conn_id));
    }

    /// 연결 해제
    pub fn unregister(&self, conn_id: ConnectionId) {
        let mut entries = self.entries.lock();
        entries.remove(&conn_id);
    }

    /// 활동 기록 (타이머 리셋)
    pub fn record_activity(&self, conn_id: ConnectionId) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(&conn_id) {
            entry.last_activity = Instant::now();
            entry.state = KeepAliveState::Idle;
            entry.failed_probes = 0;
            entry.last_probe = None;
        }
    }

    /// 프로브 응답 수신
    pub fn receive_response(&self, conn_id: ConnectionId) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(&conn_id) {
            entry.state = KeepAliveState::Idle;
            entry.failed_probes = 0;
            entry.last_activity = Instant::now();
        }
    }

    /// 연결 상태 조회
    pub fn get_state(&self, conn_id: ConnectionId) -> Option<KeepAliveState> {
        let entries = self.entries.lock();
        entries.get(&conn_id).map(|e| e.state)
    }

    /// 유휴 시간 조회
    pub fn idle_time(&self, conn_id: ConnectionId) -> Option<Duration> {
        let entries = self.entries.lock();
        entries.get(&conn_id).map(|e| e.last_activity.elapsed())
    }

    /// 이벤트 수신기 획득
    pub fn take_event_receiver(&self) -> Option<mpsc::Receiver<KeepAliveEvent>> {
        self.event_rx.lock().take()
    }

    /// Keep-Alive 체크 루프 시작
    pub async fn run(&self) {
        if !self.config.enabled {
            return;
        }

        if self.running.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return; // 이미 실행 중
        }

        let check_interval = self.config.interval.min(Duration::from_millis(100));

        loop {
            tokio::time::sleep(check_interval).await;

            let now = Instant::now();
            let mut events = Vec::new();

            {
                let mut entries = self.entries.lock();

                for entry in entries.values_mut() {
                    match entry.state {
                        KeepAliveState::Idle => {
                            // 유휴 시간 체크
                            if now.duration_since(entry.last_activity) >= self.config.idle_time {
                                entry.state = KeepAliveState::Probing;
                                entry.last_probe = Some(now);
                                events.push(KeepAliveEvent::SendProbe(entry.conn_id));
                            }
                        }
                        KeepAliveState::Probing | KeepAliveState::WaitingResponse => {
                            if let Some(last_probe) = entry.last_probe {
                                // 응답 타임아웃 체크
                                if now.duration_since(last_probe) >= self.config.response_timeout {
                                    entry.failed_probes += 1;

                                    if entry.failed_probes >= self.config.probe_count {
                                        entry.state = KeepAliveState::Failed;
                                        events.push(KeepAliveEvent::ConnectionDead(entry.conn_id));
                                    } else {
                                        // 다음 프로브
                                        entry.last_probe = Some(now);
                                        events.push(KeepAliveEvent::SendProbe(entry.conn_id));
                                    }
                                }
                            }
                        }
                        KeepAliveState::Failed => {
                            // 이미 실패 상태
                        }
                    }
                }
            }

            // 이벤트 발송
            for event in events {
                if self.event_tx.send(event).await.is_err() {
                    // 수신자가 없음
                }
            }
        }
    }

    /// 등록된 연결 수
    pub fn connection_count(&self) -> usize {
        self.entries.lock().len()
    }

    /// 설정 조회
    pub fn config(&self) -> &KeepAliveConfig {
        &self.config
    }

    /// Keep-Alive 활성화 여부
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

/// TCP Keep-Alive 설정 헬퍼
#[cfg(unix)]
pub mod tcp_keepalive {
    use std::time::Duration;
    use tokio::net::TcpStream;

    /// TCP Keep-Alive 설정
    #[allow(unsafe_code)]
    pub fn set_keepalive(
        stream: &TcpStream,
        idle_time: Duration,
        interval: Duration,
        count: u32,
    ) -> std::io::Result<()> {
        use std::os::unix::io::AsRawFd;

        let fd = stream.as_raw_fd();
        let idle_seconds = libc::c_int::try_from(idle_time.as_secs()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "TCP keepalive idle time exceeds the platform limit",
            )
        })?;
        let interval_seconds = libc::c_int::try_from(interval.as_secs()).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "TCP keepalive interval exceeds the platform limit",
            )
        })?;
        let probe_count = libc::c_int::try_from(count).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "TCP keepalive probe count exceeds the platform limit",
            )
        })?;

        // SO_KEEPALIVE 활성화
        set_socket_option(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1)?;

        // TCP_KEEPIDLE (첫 프로브까지 유휴 시간)
        #[cfg(target_os = "linux")]
        set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_KEEPIDLE, idle_seconds)?;

        #[cfg(target_os = "macos")]
        set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_KEEPALIVE, idle_seconds)?;

        // TCP_KEEPINTVL (프로브 간격)
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_KEEPINTVL, interval_seconds)?;

        // TCP_KEEPCNT (프로브 수)
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        set_socket_option(fd, libc::IPPROTO_TCP, libc::TCP_KEEPCNT, probe_count)?;

        Ok(())
    }

    #[allow(unsafe_code)]
    fn set_socket_option(
        fd: std::os::fd::RawFd,
        level: libc::c_int,
        name: libc::c_int,
        value: libc::c_int,
    ) -> std::io::Result<()> {
        // SAFETY: `fd` is borrowed from a live TcpStream. `value` points to a
        // properly aligned c_int for the duration of the call, and its exact
        // size is passed to the operating system.
        let result = unsafe {
            libc::setsockopt(
                fd,
                level,
                name,
                &value as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if result < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(not(unix))]
pub mod tcp_keepalive {
    use std::time::Duration;
    use tokio::net::TcpStream;

    /// TCP Keep-Alive 설정 (Windows 등 비 Unix 플랫폼)
    pub fn set_keepalive(
        _stream: &TcpStream,
        _idle_time: Duration,
        _interval: Duration,
        _count: u32,
    ) -> std::io::Result<()> {
        // Windows에서는 socket2 크레이트 필요
        Ok(())
    }
}
