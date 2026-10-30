//! Zeta4j Cypher Shell (`z4j shell`)
//!
//! 모든 에디션에서 공유하는 Cypher Shell CLI
//!
//! ## 사용법
//!
//! ```bash
//! # 대화형 모드
//! z4j shell --address 127.0.0.1:9045 --username admin --password password
//!
//! # 쿼리 직접 실행
//! z4j shell --address 127.0.0.1:9045 --username admin --password password "MATCH (n) RETURN count(n)"
//!
//! # 파일 실행
//! z4j shell --address 127.0.0.1:9045 --username admin --password password -f script.cypher
//!
//! # JSON 출력
//! z4j shell --address 127.0.0.1:9045 --username admin --password password --format json "MATCH (n) RETURN n"
//! ```

use std::io::{self, Write};
use std::path::PathBuf;

use clap::{Args as ClapArgs, ValueEnum};

use serde::Deserialize;
use zeta4j_bolt_client::{CypherShell, OutputFormat, ShellConfig};
use zeta4j_cli_grpc::credential::load_or_refresh_credential_envelope;

/// Output format (CLI)
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliFormat {
    /// Table (default)
    Table,
    /// JSON
    Json,
    /// CSV
    Csv,
    /// Plain text
    Plain,
}

impl From<CliFormat> for OutputFormat {
    fn from(f: CliFormat) -> Self {
        match f {
            CliFormat::Table => OutputFormat::Table,
            CliFormat::Json => OutputFormat::Json,
            CliFormat::Csv => OutputFormat::Csv,
            CliFormat::Plain => OutputFormat::Plain,
        }
    }
}

/// Zeta4j Cypher Shell CLI Arguments
#[derive(ClapArgs, Debug)]
#[command(author = "Zeta4j Team")]
#[command(about = "Cypher Shell for Zeta4j Graph Database", long_about = None)]
#[command(version)]
pub struct Args {
    /// Server address (host:port)
    #[arg(
        short = 'a',
        long,
        env = "ZETA4J_ADDRESS",
        default_value = "127.0.0.1:9045"
    )]
    pub address: String,

    /// Username
    #[arg(long, env = "ZETA4J_USERNAME")]
    pub username: Option<String>,

    /// Password
    #[arg(long, env = "ZETA4J_PASSWORD", hide_env_values = true)]
    pub password: Option<String>,

    /// Owner-only Bolt credential envelope created by `z4j auth --audience bolt`.
    #[arg(long, env = "ZETA4J_BOLT_CREDENTIAL_FILE")]
    pub credential_file: Option<PathBuf>,

    /// Database
    #[arg(long, env = "ZETA4J_DATABASE", default_value = "zeta4j")]
    pub database: String,

    /// Output format
    #[arg(long, value_enum)]
    pub format: Option<CliFormat>,

    /// Page size (0 = unlimited)
    #[arg(long)]
    pub page_size: Option<usize>,

    /// Disable colored output
    #[arg(long)]
    pub no_color: bool,

    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Configuration directory
    #[arg(long, env = "ZETA4J_CONFIG_DIR")]
    pub config_dir: Option<PathBuf>,

    /// Run a script file
    #[arg(short = 'f', long)]
    pub file: Option<PathBuf>,

    /// Cypher query to run directly
    #[arg(trailing_var_arg = true)]
    pub query: Vec<String>,
}

/// Cypher Shell 실행
///
/// # Arguments
///
/// * `version` - 바이너리 버전 (Cargo.toml에서 가져옴)
///
/// 주어진 인자로 Cypher Shell 실행
pub fn run_with_args(args: Args, version: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Verbose 모드 설정
    crate::verbose::set_verbose_level(args.verbose);

    // 설정 파일 로드 (있으면)
    let cli_settings = load_cli_settings(&args.config_dir);

    // CLI 설정 결정: CLI 인자 > config 파일 > 기본값
    let effective_format =
        args.format.map(OutputFormat::from).unwrap_or_else(|| {
            match cli_settings.output_format.as_str() {
                "json" => OutputFormat::Json,
                "csv" => OutputFormat::Csv,
                "plain" => OutputFormat::Plain,
                _ => OutputFormat::Table,
            }
        });

    let effective_page_size = args.page_size.or(if cli_settings.page_size > 0 {
        Some(cli_settings.page_size)
    } else {
        None
    });

    let effective_history_file = cli_settings.history_file.map(PathBuf::from);

    // 셸 설정 (인증 정보는 연결 후 결정)
    let mut config = ShellConfig::new()
        .with_address(&args.address)
        .with_database(&args.database)
        .with_format(effective_format)
        .with_color(!args.no_color);

    // 페이지 크기 설정
    config.page_size = effective_page_size;

    // 히스토리 파일 설정
    if effective_history_file.is_some() {
        config.history_file = effective_history_file;
    }

    let mut shell = CypherShell::new(config);

    // 서버 인증 모드 자동 감지
    match shell.try_connect_without_auth() {
        Ok(true) => {
            // 서버가 --no-auth 모드, 이미 연결됨
            if args.verbose > 0 {
                eprintln!("Server running in no-auth mode");
            }
        }
        Ok(false) => {
            let credential_path = args
                .credential_file
                .clone()
                .unwrap_or_else(default_bolt_credential_path);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|duration| i64::try_from(duration.as_secs()).ok())
                .unwrap_or(i64::MAX);
            if let Ok(envelope) = load_or_refresh_credential_envelope(&credential_path, now)
                && envelope.audience == "bolt"
            {
                shell.connect_remote_bearer(envelope.access_token())?;
                shell.config_mut().username = "credential".to_string();
                shell.config_mut().update_prompt();
            } else {
                let username = args.username.clone().unwrap_or_else(|| {
                    eprint!("Username: ");
                    io::stderr().flush().ok();
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).ok();
                    input.trim().to_string()
                });
                let password = args
                    .password
                    .clone()
                    .map_or_else(|| rpassword::prompt_password("Password: "), Ok)
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                shell.connect_remote(&username, &password)?;
                shell.config_mut().username = username;
                shell.config_mut().update_prompt();
            }
        }
        Err(e) => {
            return Err(e.into());
        }
    }

    // 실행 모드 결정
    if let Some(file_path) = args.file {
        // 파일 모드
        let outputs = shell.execute_file(&file_path)?;
        for output in outputs {
            print_shell_output(&output);
        }
    } else if !args.query.is_empty() {
        // 직접 쿼리 모드 - 세미콜론 자동 추가 (멀티라인 모드 방지)
        let mut query = args.query.join(" ");
        if !query.trim().ends_with(';') {
            query.push(';');
        }
        match shell.execute(&query) {
            Ok(Some(output)) => print_shell_output(&output),
            Ok(None) => {}
            Err(e) => {
                return Err(e.into());
            }
        }
    } else {
        // 대화형 모드
        print_banner(version);
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        shell.run_interactive(&mut stdin.lock(), &mut stdout)?;
    }

    // 연결 종료
    shell.disconnect();

    Ok(())
}

fn print_shell_output(output: &str) {
    if output.ends_with('\n') {
        print!("{}", output);
    } else {
        println!("{}", output);
    }
}

/// 배너 출력
fn print_banner(version: &str) {
    eprintln!(
        r#"
Zeta4j Cypher Shell v{}
Type :help for assistance.
"#,
        version
    );
}

/// CLI 설정 로드
fn load_cli_settings(config_dir: &Option<PathBuf>) -> CliSettings {
    // 설정 디렉토리 결정: CLI 인자 > 환경변수 > 기본값 (~/.zeta4j)
    let config_path = if let Some(dir) = config_dir {
        dir.join("core.toml")
    } else {
        // 기본 경로: ~/.zeta4j/config/core.toml
        if let Some(home) = dirs::home_dir() {
            home.join(".zeta4j").join("config").join("core.toml")
        } else {
            return CliSettings::default();
        }
    };

    // 설정 파일이 없으면 기본값 반환
    if !config_path.exists() {
        return CliSettings::default();
    }

    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|contents| toml::from_str::<CliConfigFile>(&contents).ok())
        .map(|config| config.cli)
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
struct CliConfigFile {
    #[serde(default)]
    cli: CliSettings,
}

#[derive(Debug, Deserialize)]
struct CliSettings {
    #[serde(default = "default_output_format")]
    output_format: String,
    #[serde(default = "default_page_size")]
    page_size: usize,
    #[serde(default)]
    history_file: Option<String>,
}

impl Default for CliSettings {
    fn default() -> Self {
        Self {
            output_format: default_output_format(),
            page_size: default_page_size(),
            history_file: None,
        }
    }
}

fn default_output_format() -> String {
    "table".to_string()
}

fn default_page_size() -> usize {
    100
}

fn default_bolt_credential_path() -> PathBuf {
    if let Some(home) = std::env::var_os("ZETA4J_HOME") {
        return PathBuf::from(home).join("credentials").join("bolt.json");
    }
    dirs::home_dir()
        .map(|home| home.join(".zeta4j").join("credentials").join("bolt.json"))
        .unwrap_or_else(|| PathBuf::from("bolt-credential.json"))
}
