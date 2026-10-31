// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server Configuration

use super::error::ConfigError;
use std::net::SocketAddr;
use std::time::Duration;

/// TCP Keepalive 설정
#[derive(Debug, Clone)]
pub struct TcpKeepalive {
    /// Keepalive 시작 시간 (기본: 60초)
    pub time: Duration,
    /// Keepalive 간격 (기본: 10초)
    pub interval: Duration,
    /// 재시도 횟수 (기본: 9)
    pub retries: u32,
}

impl Default for TcpKeepalive {
    fn default() -> Self {
        Self {
            time: Duration::from_secs(60),
            interval: Duration::from_secs(10),
            retries: 9,
        }
    }
}

/// 서버 설정
///
/// # Example
/// ```ignore
/// let config = ServerConfig::builder()
///     .bind_addr("0.0.0.0:9045".parse()?)
///     .max_connections(10_000)
///     .build()?;
/// ```
#[derive(Debug, Clone)]
pub struct ServerConfig {
    // === 네트워크 설정 ===
    /// 바인딩 주소 (기본: 0.0.0.0:9045)
    pub bind_addr: SocketAddr,

    /// 바인딩할 추가 주소 (다중 인터페이스)
    pub additional_addrs: Vec<SocketAddr>,

    /// TCP backlog 크기 (기본: 1024)
    pub backlog: u32,

    /// SO_REUSEADDR 옵션 (기본: true)
    pub reuse_addr: bool,

    /// SO_REUSEPORT 옵션 - Linux only (기본: true)
    pub reuse_port: bool,

    /// TCP_NODELAY (Nagle 알고리즘 비활성화, 기본: true)
    pub tcp_nodelay: bool,

    /// TCP keepalive 설정
    pub tcp_keepalive: Option<TcpKeepalive>,

    // === 연결 제한 ===
    /// 최대 동시 연결 수 (기본: 10,000)
    pub max_connections: usize,

    /// IP당 최대 연결 수 (기본: 100, 0=무제한)
    pub max_connections_per_ip: usize,

    /// 초당 최대 새 연결 수 (기본: 1,000)
    pub max_connections_per_second: u32,

    // === 타임아웃 ===
    /// 연결 수락 타임아웃 (기본: 10초)
    pub accept_timeout: Duration,

    /// 핸드셰이크 타임아웃 (기본: 30초)
    pub handshake_timeout: Duration,

    /// 읽기 타임아웃 (기본: 300초)
    pub read_timeout: Duration,

    /// 쓰기 타임아웃 (기본: 60초)
    pub write_timeout: Duration,

    /// 유휴 연결 타임아웃 (기본: 3600초)
    pub idle_timeout: Duration,

    // === 워커 설정 ===
    /// Acceptor 스레드 수 (기본: CPU 코어 수)
    pub acceptor_threads: usize,

    /// Worker 스레드 수 (기본: CPU 코어 수 * 2)
    pub worker_threads: usize,

    /// 워커당 최대 연결 수 (기본: 500)
    pub connections_per_worker: usize,

    // === 버퍼 설정 ===
    /// 수신 버퍼 크기 (기본: 64KB)
    pub recv_buffer_size: usize,

    /// 송신 버퍼 크기 (기본: 64KB)
    pub send_buffer_size: usize,

    // === 백프레셔 설정 ===
    /// 백프레셔 활성화 (기본: true)
    pub backpressure_enabled: bool,

    /// 백프레셔 고수위 (기본: 80%)
    pub backpressure_high_watermark: f64,

    /// 백프레셔 저수위 (기본: 50%)
    pub backpressure_low_watermark: f64,

    // === Graceful Shutdown ===
    /// 셧다운 드레인 타임아웃 (기본: 30초)
    pub shutdown_timeout: Duration,

    /// 강제 종료 전 대기 시간 (기본: 60초)
    pub force_shutdown_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        let cpu_count = num_cpus::get();

        Self {
            bind_addr: ([0, 0, 0, 0], 9045).into(),
            additional_addrs: vec![],
            backlog: 1024,
            reuse_addr: true,
            reuse_port: true,
            tcp_nodelay: true,
            tcp_keepalive: Some(TcpKeepalive::default()),

            max_connections: 10_000,
            max_connections_per_ip: 100,
            max_connections_per_second: 1_000,

            accept_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(30),
            read_timeout: Duration::from_secs(300),
            write_timeout: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(3600),

            acceptor_threads: cpu_count,
            worker_threads: cpu_count * 2,
            connections_per_worker: 500,

            recv_buffer_size: 64 * 1024,
            send_buffer_size: 64 * 1024,

            backpressure_enabled: true,
            backpressure_high_watermark: 0.8,
            backpressure_low_watermark: 0.5,

            shutdown_timeout: Duration::from_secs(30),
            force_shutdown_timeout: Duration::from_secs(60),
        }
    }
}

impl ServerConfig {
    /// Builder 생성
    pub fn builder() -> ServerConfigBuilder {
        ServerConfigBuilder::new()
    }

    /// 설정 검증
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.max_connections == 0 {
            return Err(ConfigError::InvalidValue("max_connections must be > 0"));
        }

        if self.backpressure_high_watermark <= self.backpressure_low_watermark {
            return Err(ConfigError::InvalidValue(
                "high_watermark must be > low_watermark",
            ));
        }

        if self.backpressure_high_watermark > 1.0 || self.backpressure_high_watermark < 0.0 {
            return Err(ConfigError::OutOfRange(
                "high_watermark must be between 0.0 and 1.0",
            ));
        }

        if self.backpressure_low_watermark > 1.0 || self.backpressure_low_watermark < 0.0 {
            return Err(ConfigError::OutOfRange(
                "low_watermark must be between 0.0 and 1.0",
            ));
        }

        if self.worker_threads == 0 {
            return Err(ConfigError::InvalidValue("worker_threads must be > 0"));
        }

        if self.acceptor_threads == 0 {
            return Err(ConfigError::InvalidValue("acceptor_threads must be > 0"));
        }

        if self.backlog == 0 {
            return Err(ConfigError::InvalidValue("backlog must be > 0"));
        }

        if self.shutdown_timeout >= self.force_shutdown_timeout {
            return Err(ConfigError::InvalidValue(
                "shutdown_timeout must be < force_shutdown_timeout",
            ));
        }

        Ok(())
    }
}

/// ServerConfig Builder
#[derive(Default)]
pub struct ServerConfigBuilder {
    config: ServerConfig,
}

impl ServerConfigBuilder {
    /// 새 빌더 생성
    pub fn new() -> Self {
        Self {
            config: ServerConfig::default(),
        }
    }

    /// 바인딩 주소 설정
    pub fn bind_addr(mut self, addr: SocketAddr) -> Self {
        self.config.bind_addr = addr;
        self
    }

    /// 추가 바인딩 주소
    pub fn additional_addr(mut self, addr: SocketAddr) -> Self {
        self.config.additional_addrs.push(addr);
        self
    }

    /// 최대 연결 수 설정
    pub fn max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max;
        self
    }

    /// IP당 최대 연결 수 설정
    pub fn max_connections_per_ip(mut self, max: usize) -> Self {
        self.config.max_connections_per_ip = max;
        self
    }

    /// TCP backlog 설정
    pub fn backlog(mut self, backlog: u32) -> Self {
        self.config.backlog = backlog;
        self
    }

    /// TCP_NODELAY 설정
    pub fn tcp_nodelay(mut self, nodelay: bool) -> Self {
        self.config.tcp_nodelay = nodelay;
        self
    }

    /// TCP Keepalive 설정
    pub fn tcp_keepalive(mut self, keepalive: Option<TcpKeepalive>) -> Self {
        self.config.tcp_keepalive = keepalive;
        self
    }

    /// 핸드셰이크 타임아웃 설정
    pub fn handshake_timeout(mut self, timeout: Duration) -> Self {
        self.config.handshake_timeout = timeout;
        self
    }

    /// 읽기 타임아웃 설정
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.config.read_timeout = timeout;
        self
    }

    /// 쓰기 타임아웃 설정
    pub fn write_timeout(mut self, timeout: Duration) -> Self {
        self.config.write_timeout = timeout;
        self
    }

    /// 유휴 타임아웃 설정
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.config.idle_timeout = timeout;
        self
    }

    /// Acceptor 스레드 수 설정
    pub fn acceptor_threads(mut self, threads: usize) -> Self {
        self.config.acceptor_threads = threads;
        self
    }

    /// Worker 스레드 수 설정
    pub fn worker_threads(mut self, threads: usize) -> Self {
        self.config.worker_threads = threads;
        self
    }

    /// 백프레셔 활성화 설정
    pub fn backpressure_enabled(mut self, enabled: bool) -> Self {
        self.config.backpressure_enabled = enabled;
        self
    }

    /// 백프레셔 고수위 설정
    pub fn backpressure_high_watermark(mut self, watermark: f64) -> Self {
        self.config.backpressure_high_watermark = watermark;
        self
    }

    /// 백프레셔 저수위 설정
    pub fn backpressure_low_watermark(mut self, watermark: f64) -> Self {
        self.config.backpressure_low_watermark = watermark;
        self
    }

    /// Shutdown 타임아웃 설정
    pub fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.config.shutdown_timeout = timeout;
        self
    }

    /// 강제 Shutdown 타임아웃 설정
    pub fn force_shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.config.force_shutdown_timeout = timeout;
        self
    }

    /// 설정 빌드
    pub fn build(self) -> Result<ServerConfig, ConfigError> {
        self.config.validate()?;
        Ok(self.config)
    }
}
