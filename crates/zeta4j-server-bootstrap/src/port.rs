//! Server port management and conflict resolution.
//!
//! 이 모듈은 서버 시작 시 포트 충돌을 감지하고 해결합니다.
//!
//! # `--replace` 옵션
//!
//! `--replace` 옵션을 사용하면 이미 사용 중인 포트의 프로세스를 강제 종료합니다.
//!
//! ## 동작 방식
//!
//! 1. 포트가 사용 중인지 확인
//! 2. `--replace` 옵션이 설정된 경우:
//!    - Unix: `lsof`로 PID 찾기 → `SIGTERM` 전송 → 1초 대기 → `SIGKILL` 전송
//!    - Windows: `netstat`로 PID 찾기 → `taskkill /F`로 강제 종료
//! 3. 2초 대기 후 포트 바인딩 시도
//!
//! ## 주의사항
//!
//! **⚠️ 운영 환경에서 주의해서 사용하세요:**
//!
//! - 이 옵션은 기존 서버 프로세스를 **강제 종료**합니다
//! - 데이터 손실이나 트랜잭션 중단이 발생할 수 있습니다
//! - 개발 환경이나 테스트 환경에서 사용을 권장합니다
//! - 운영 환경에서는 먼저 기존 프로세스를 정상 종료한 후 새 서버를 시작하세요
//!
//! ## 사용 예시
//!
//! ```bash
//! # 포트 충돌 시 기존 프로세스 종료 후 시작
//! zeta4jd --replace
//!
//! # 특정 포트로 실행하며 충돌 시 기존 프로세스 종료
//! zeta4jd --http-port 8080 --bolt-port 7687 --replace
//! ```

use std::time::Duration;

use log::{error, info, warn};
use tokio::time::sleep;

use super::ValidatedStartupConfig;

/// Manages port binding and conflict resolution
pub struct PortManager<'a> {
    config: &'a ValidatedStartupConfig,
}

impl<'a> PortManager<'a> {
    /// Create a new PortManager
    pub fn new(config: &'a ValidatedStartupConfig) -> Self {
        Self { config }
    }

    /// Check and resolve port conflicts
    ///
    /// Returns error if ports are in use and --replace is not set
    pub async fn resolve_conflicts(&self) -> Result<(), PortError> {
        let addresses_to_check = vec![
            (&self.config.http_host, self.config.http_port, "HTTP"),
            (&self.config.bolt_host, self.config.bolt_port, "Bolt"),
            (&self.config.grpc_host, self.config.grpc_port, "gRPC"),
        ];

        for (host, port, name) in &addresses_to_check {
            if !Self::is_port_available(host, *port) {
                if self.config.replace {
                    self.replace_existing_process(*port, name).await?;
                } else {
                    return Err(PortError::InUse {
                        port: *port,
                        name: name.to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    /// Check if a port is available
    fn is_port_available(host: &str, port: u16) -> bool {
        std::net::TcpListener::bind(format!("{host}:{port}"))
            .map(|listener| {
                drop(listener);
                true
            })
            .unwrap_or(false)
    }

    /// Find and kill existing process using the port
    async fn replace_existing_process(&self, port: u16, name: &str) -> Result<(), PortError> {
        info!(
            "Port {} ({}) is in use, attempting to find and kill existing process...",
            port, name
        );

        if let Some(pid) = find_process_using_port(port) {
            info!("Found process {} using port {}, killing...", pid, port);
            kill_process(pid).await;
            // Wait for process to terminate
            sleep(Duration::from_secs(2)).await;
            Ok(())
        } else {
            error!("Could not find process using port {}", port);
            Err(PortError::ProcessNotFound { port })
        }
    }
}

/// Port-related errors
#[derive(Debug)]
pub enum PortError {
    /// Port is already in use
    InUse { port: u16, name: String },
    /// Could not find process using port
    ProcessNotFound { port: u16 },
}

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortError::InUse { port, name } => {
                write!(
                    f,
                    "Port {} ({}) is already in use. Use --replace to kill the existing process, or use a different port",
                    port, name
                )
            }
            PortError::ProcessNotFound { port } => {
                write!(
                    f,
                    "Could not find process using port {}. Please manually stop the process or use a different port",
                    port
                )
            }
        }
    }
}

impl std::error::Error for PortError {}

/// Check if a port is available on localhost
///
/// # Example
///
/// ```ignore
/// if check_port_available(9044) {
///     println!("Port 9044 is available");
/// }
/// ```
pub fn check_port_available(port: u16) -> bool {
    std::net::TcpListener::bind(format!("127.0.0.1:{}", port))
        .map(|listener| {
            drop(listener);
            true
        })
        .unwrap_or(false)
}

/// Find process using a specific port (Unix implementation)
#[cfg(unix)]
pub fn find_process_using_port(port: u16) -> Option<u32> {
    use std::process::Command;

    let output = Command::new("lsof")
        .args(["-t", "-i", &format!(":{}", port)])
        .output()
        .ok()?;

    if output.status.success() {
        let pids = String::from_utf8_lossy(&output.stdout);
        pids.lines().next()?.trim().parse().ok()
    } else {
        None
    }
}

/// Find process using a specific port (Windows implementation)
#[cfg(windows)]
pub fn find_process_using_port(port: u16) -> Option<u32> {
    use std::process::Command;

    let output = Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .output()
        .ok()?;

    if output.status.success() {
        let lines = String::from_utf8_lossy(&output.stdout);
        for line in lines.lines() {
            if line.contains(&format!(":{}", port)) && line.contains("LISTENING") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(pid_str) = parts.last() {
                    return pid_str.parse().ok();
                }
            }
        }
    }
    None
}

/// Kill a process by PID (Unix implementation)
#[cfg(unix)]
pub async fn kill_process(pid: u32) {
    use std::process::Command;

    // Try SIGTERM first
    if let Err(e) = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .output()
    {
        warn!("Failed to send SIGTERM to process {}: {}", pid, e);
    }

    // Wait 1 second for graceful shutdown
    sleep(Duration::from_secs(1)).await;

    // If still alive, use SIGKILL
    if Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
        && let Err(e) = Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output()
    {
        warn!("Failed to send SIGKILL to process {}: {}", pid, e);
    }
}

/// Kill a process by PID (Windows implementation)
#[cfg(windows)]
pub async fn kill_process(pid: u32) {
    use std::process::Command;

    if let Err(e) = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .output()
    {
        warn!("Failed to kill process {}: {}", pid, e);
    }
}
