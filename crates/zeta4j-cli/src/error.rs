//! CLI 에러 처리
//!
//! `ZetaError`를 기반으로 사용자 친화적인 에러 메시지와 해결 제안을 제공합니다.
//!
//! ## 사용법
//!
//! ```rust,ignore
//! use zeta4j_cli::error::{CliErrorExt, CliResult};
//! use zeta4j_common::error::ZetaError;
//!
//! fn example() -> CliResult<()> {
//!     let err = ZetaError::server_not_running();
//!     eprintln!("{}", err.suggestion());
//!     Err(err)
//! }
//! ```

use zeta4j_common::error::ZetaError;

/// CLI 결과 타입 (ZetaError 기반)
pub type CliResult<T> = Result<T, ZetaError>;

// ============================================================
// CLI Error Extension Trait
// ============================================================

/// CLI용 ZetaError 확장 트레이트
///
/// 사용자 친화적인 해결 제안과 포맷팅 기능을 제공합니다.
pub trait CliErrorExt {
    /// 해결 제안 반환
    fn suggestion(&self) -> &'static str;

    /// ANSI 색상이 포함된 포맷된 출력
    fn format_colored(&self) -> String;

    /// 플레인 텍스트 포맷된 출력
    fn format_plain(&self) -> String;
}

impl CliErrorExt for ZetaError {
    fn suggestion(&self) -> &'static str {
        match self {
            ZetaError::Connection { .. } | ZetaError::Network { .. } => {
                "Resolution:\n\
                 1. Check if the server is running: z4j server status\n\
                 2. Verify the server address and port are correct\n\
                 3. Check firewall settings\n\
                 4. Check network connectivity"
            }
            ZetaError::AuthFailed { .. } | ZetaError::TokenExpired { .. } => {
                "Resolution:\n\
                 1. Verify username and password\n\
                 2. Check environment variables: ZETA4J_USERNAME, ZETA4J_PASSWORD\n\
                 3. Contact an administrator about account status"
            }
            ZetaError::ConfigParse { .. } | ZetaError::InvalidConfig { .. } => {
                "Resolution:\n\
                 1. Check config file syntax: z4j server config check\n\
                 2. Generate default config file: z4j server config init\n\
                 3. Verify config file path"
            }
            ZetaError::QueryParse { .. } | ZetaError::QueryExecution { .. } => {
                "Resolution:\n\
                 1. Check Cypher syntax\n\
                 2. Ensure query ends with semicolon (;)\n\
                 3. Use single quotes (') or double quotes (\") for strings\n\
                 4. Check help with :help command"
            }
            ZetaError::PortConflict { .. } => {
                "Resolution:\n\
                 1. Diagnose port conflicts: z4j server doctor\n\
                 2. Use different ports: --http-port, --bolt-port\n\
                 3. Stop existing process: z4j server stop --force\n\
                 4. Auto-replace with --replace option: z4j server start --replace"
            }
            ZetaError::PermissionDenied { .. } => {
                "Resolution:\n\
                 1. Check required permissions\n\
                 2. Check data directory permissions\n\
                 3. Run with sudo or administrator privileges\n\
                 4. Check service account settings"
            }
            ZetaError::NotFound { .. } => {
                "Resolution:\n\
                 1. Check file path\n\
                 2. Verify file exists\n\
                 3. Use absolute path instead of relative path"
            }
            ZetaError::ServerNotRunning { .. } => {
                "Resolution:\n\
                 1. Start the server: z4j server start\n\
                 2. Check server status: z4j server status\n\
                 3. Check logs: z4j server logs"
            }
            ZetaError::ServerAlreadyRunning { .. } => {
                "Resolution:\n\
                 1. Stop existing server: z4j server stop\n\
                 2. Force stop: z4j server stop --force\n\
                 3. Replace with --replace option: z4j server start --replace"
            }
            ZetaError::ConnectionTimeout { .. } | ZetaError::QueryTimeout { .. } => {
                "Resolution:\n\
                 1. Check server status: z4j server status\n\
                 2. Increase timeout value: --timeout\n\
                 3. Check network status\n\
                 4. Check server resources: z4j server doctor"
            }
            _ => {
                "Resolution:\n\
                 1. Check detailed logs: z4j server logs\n\
                 2. Collect a doctor report: z4j server doctor --output doctor-report\n\
                 3. Contact support if the issue persists"
            }
        }
    }

    fn format_colored(&self) -> String {
        let red = "\x1b[31m";
        let cyan = "\x1b[36m";
        let bold = "\x1b[1m";
        let reset = "\x1b[0m";

        format!(
            "{red}{bold}Error:{reset} {}\n\n{cyan}{}{reset}\n",
            self,
            self.suggestion()
        )
    }

    fn format_plain(&self) -> String {
        format!("Error: {}\n\n{}\n", self, self.suggestion())
    }
}

// ============================================================
// Error Context Trait
// ============================================================

/// 에러 컨텍스트를 추가하는 trait
pub trait ErrorContext<T> {
    /// 연결 에러로 변환
    fn connection_error(self, msg: impl Into<String>) -> Result<T, ZetaError>;

    /// 인증 에러로 변환
    fn auth_error(self, msg: impl Into<String>) -> Result<T, ZetaError>;

    /// 설정 에러로 변환
    fn config_error(self, msg: impl Into<String>) -> Result<T, ZetaError>;

    /// 쿼리 에러로 변환
    fn query_error(self, msg: impl Into<String>) -> Result<T, ZetaError>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> ErrorContext<T> for Result<T, E> {
    fn connection_error(self, msg: impl Into<String>) -> Result<T, ZetaError> {
        self.map_err(|_| ZetaError::connection(msg))
    }

    fn auth_error(self, msg: impl Into<String>) -> Result<T, ZetaError> {
        self.map_err(|_| ZetaError::auth_failed(msg))
    }

    fn config_error(self, msg: impl Into<String>) -> Result<T, ZetaError> {
        self.map_err(|_| ZetaError::config_parse(msg))
    }

    fn query_error(self, msg: impl Into<String>) -> Result<T, ZetaError> {
        self.map_err(|_| ZetaError::query_parse(msg))
    }
}

// ============================================================
// Convenience Helpers
// ============================================================

/// 연결 에러 생성 헬퍼
pub fn connection_error(address: &str) -> ZetaError {
    ZetaError::connection(format!("Cannot connect to server: {}", address))
}

/// 인증 에러 생성 헬퍼
pub fn auth_error(username: &str) -> ZetaError {
    ZetaError::auth_failed(format!("Authentication failed: user '{}'", username))
}

/// 포트 충돌 에러 생성 헬퍼
pub fn port_conflict_error(port: u16, process: Option<&str>) -> ZetaError {
    ZetaError::port_conflict(port, process.map(String::from))
}

/// 서버 미실행 에러 생성 헬퍼
pub fn server_not_running_error() -> ZetaError {
    ZetaError::server_not_running()
}

/// 타임아웃 에러 생성 헬퍼
pub fn timeout_error(operation: &str, timeout_secs: u64) -> ZetaError {
    ZetaError::connection_timeout(format!(
        "{} operation timed out after {} seconds",
        operation, timeout_secs
    ))
}

/// 쿼리 에러 생성 헬퍼
pub fn query_syntax_error(
    query: &str,
    position: Option<usize>,
    expected: Option<&str>,
) -> ZetaError {
    let mut msg = "Cypher syntax error".to_string();

    if let Some(pos) = position {
        msg.push_str(&format!(" (position: {})", pos));
    }

    if let Some(exp) = expected {
        msg.push_str(&format!(", expected: {}", exp));
    }

    // Append part of the query to the message
    let truncated = if query.len() > 50 {
        format!("{}...", &query[..50])
    } else {
        query.to_string()
    };
    msg.push_str(&format!(" - query: {}", truncated));

    ZetaError::query_parse(msg)
}
