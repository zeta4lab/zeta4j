//! Zeta4j Server Binary
//!
//! Zeta4j graph database server entry point

#![allow(clippy::too_many_arguments)]
#![allow(clippy::lines_filter_map_ok)]
#![allow(clippy::ptr_arg)]
//!
//! ## 사용법
//!
//! ```bash
//! # 기본 실행 (HTTP: 9044, Bolt: 9045)
//! # 데이터: ~/.zeta4j/databases, 로그: ~/.zeta4j/logs
//! cargo run --bin zeta4jd
//!
//! # 포트 지정
//! cargo run --bin zeta4jd -- --http-port 8080 --bolt-port 7688
//!
//! # 바인드 주소 지정
//! cargo run --bin zeta4jd -- --host 0.0.0.0 --http-port 9044
//!
//! # production은 [transport] identity/root/CRL reference 없이는 시작하지 않음
//!
//! # 데이터/로그 디렉토리 지정
//! cargo run --bin zeta4jd -- --data-dir /var/lib/zeta4j --log-dir /var/log/zeta4j
//! ```

use std::path::PathBuf;

use clap::Parser;
use log::error;

use zeta4j_server::config::Edition;
use zeta4j_server_bootstrap::{
    CommonArgs, ConfigMerger, DatabaseInit, DirectoryInit, PortManager, RuntimeProfile,
    ServerConfigBuilder, ServerRuntime, TlsConfigBuilder, enforce_owner_only_process_umask,
    init_logging, print_banner, validate_startup_security,
};
use zeta4j_server_lifecycle::{PidLock, cleanup_stale_pid, get_home_dir};

/// Cargo.toml에서 버전 가져오기
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Zeta4j graph database server
#[derive(Parser, Debug)]
#[command(name = "zeta4j-server")]
#[command(author = "Zeta4j Team")]
#[command(version = VERSION)]
#[command(about = "Zeta4j graph database server", long_about = None)]
pub struct Args {
    /// Bind host
    #[arg(long, env = "ZETA4J_BIND_ADDRESS")]
    pub host: Option<String>,

    /// HTTP port
    #[arg(long = "http-port", env = "ZETA4J_HTTP_PORT")]
    pub http_port: Option<u16>,

    /// Bolt protocol port
    #[arg(long, env = "ZETA4J_BOLT_PORT")]
    pub bolt_port: Option<u16>,

    /// gRPC admin port
    #[arg(long, env = "ZETA4J_GRPC_PORT")]
    pub grpc_port: Option<u16>,

    /// Data directory (default: ~/.zeta4j/databases)
    #[arg(long, env = "ZETA4J_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Log directory (default: ~/.zeta4j/logs)
    #[arg(long, env = "ZETA4J_LOG_DIR")]
    pub log_dir: Option<PathBuf>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, env = "ZETA4J_LOG_LEVEL")]
    pub log_level: Option<String>,

    /// Enable request compression [default: true]
    #[arg(long, env = "ZETA4J_COMPRESSION", action = clap::ArgAction::Set)]
    pub compression: Option<bool>,

    /// Request timeout in seconds
    #[arg(long, env = "ZETA4J_TIMEOUT")]
    pub timeout: Option<u64>,

    /// Stop any process already using the ports before starting
    #[arg(long, env = "ZETA4J_REPLACE")]
    pub replace: bool,

    /// Server time zone (utc, local); applies to logs, API responses, and temporal functions
    #[arg(long, env = "ZETA4J_TIMEZONE", value_parser = ["utc", "local"])]
    pub timezone: Option<String>,

    /// Path to a log4rs configuration file.
    /// Supports TOML and JSON; the built-in configuration is used if omitted.
    #[arg(long, env = "ZETA4J_LOG_CONFIG")]
    pub log_config: Option<PathBuf>,

    /// Disable authentication (development/testing only)
    #[arg(long, env = "ZETA4J_NO_AUTH")]
    pub no_auth: bool,

    /// Runtime security profile
    #[arg(long, env = "ZETA4J_RUNTIME_PROFILE")]
    pub profile: Option<RuntimeProfile>,

    /// Configuration directory.
    /// Directory containing the TOML configuration files (core.toml, server.toml, etc.)
    #[arg(long, env = "ZETA4J_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,

    /// Print machine-readable build information for the server artifact
    #[arg(long, hide = true)]
    pub build_info_json: bool,

    /// Print the effective configuration as JSON without side effects
    #[arg(long, hide = true)]
    pub effective_config_json: bool,

    /// Detect configuration changes between inspection and startup
    #[arg(long, hide = true)]
    pub expected_config_fingerprint: Option<String>,
}

impl CommonArgs for Args {
    fn runtime_profile(&self) -> Option<RuntimeProfile> {
        self.profile
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. rustls CryptoProvider 설치 (HTTPS 사용 시 필요)
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        eprintln!("Error: Failed to install TLS crypto provider: {:?}", e);
        eprintln!("This may indicate a configuration issue with the TLS subsystem.");
        std::process::exit(1);
    }

    // 2. CLI 인자 파싱
    let args = Args::parse();

    if args.build_info_json {
        println!("{}", serde_json::json!({ "version": VERSION }));
        return Ok(());
    }

    let edition = Edition::Base;
    let merger = ConfigMerger::new(&args, edition)?;
    let config = validate_startup_security(merger.merge())?;
    enforce_owner_only_process_umask();
    if let Some(expected) = args.expected_config_fingerprint.as_deref()
        && merger.config_fingerprint() != Some(expected)
    {
        return Err(std::io::Error::other(
            "configuration changed after effective-config inspection",
        )
        .into());
    }
    if args.effective_config_json {
        println!(
            "{}",
            serde_json::to_string(&config.snapshot(merger.config_fingerprint()))?
        );
        return Ok(());
    }

    // 3. 홈 디렉토리 및 PID 잠금 획득
    let home = get_home_dir(None);

    // Stale PID 파일 정리
    if let Ok(cleaned) = cleanup_stale_pid(&home)
        && cleaned
    {
        eprintln!("Note: Cleaned up stale PID file");
    }

    // PID 잠금 획득 (다른 인스턴스 실행 중이면 실패)
    let pid = std::process::id();
    let _pid_lock = match PidLock::acquire(&home, pid) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("Error: Failed to acquire PID lock: {}", e);
            eprintln!("Another Zeta4j instance may already be running.");
            eprintln!("Check with: z4j server status");
            std::process::exit(1);
        }
    };

    // 6. 다른 시작 부수효과 전에 상태 형식 검증
    DirectoryInit::new(&config).prepare_state()?;

    // 7. 로깅 초기화
    init_logging(edition, &config, args.log_config.as_ref())?;

    // 8. 배너 출력
    print_banner(edition);

    // 9. 포트 충돌 해결
    if let Err(e) = PortManager::new(&config).resolve_conflicts().await {
        error!("{}", e);
        std::process::exit(1);
    }

    // 10. 나머지 디렉토리 초기화
    DirectoryInit::new(&config).create_all()?;

    // 11. 데이터베이스 초기화
    let db_services = DatabaseInit::new(&config, merger.registry())
        .initialize()
        .await?;

    // 12. 서버 컴포넌트 빌드
    let server_components = ServerConfigBuilder::new(&config, &db_services)
        .with_edition(edition)
        .build()
        .await?;

    // 14. TLS 설정 로드
    let tls_config = TlsConfigBuilder::new(&config).load();

    // 15. 서버 실행
    let data_dir = config.data_dir.display().to_string();
    let log_dir = config.log_dir.display().to_string();

    let runtime = ServerRuntime::new(edition, server_components, tls_config);
    runtime.run(&data_dir, &log_dir).await?;

    Ok(())
}
