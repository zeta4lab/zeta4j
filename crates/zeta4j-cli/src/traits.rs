//! 공통 CLI 트레이트 및 컨텍스트
//!
//! 모든 CLI 커맨드가 구현해야 하는 인터페이스와
//! 실행 컨텍스트를 정의합니다.

use std::path::PathBuf;
use std::process::ExitCode;
use zeta4j_common::error::ZetaError;

// ============================================================
// Command Trait
// ============================================================

/// CLI 커맨드 인터페이스
///
/// 모든 CLI 커맨드는 이 트레이트를 구현해야 합니다.
///
/// # Example
///
/// ```rust,ignore
/// use zeta4j_cli::traits::{Command, Context, CommandResult};
///
/// struct StartCommand {
///     foreground: bool,
/// }
///
/// impl Command for StartCommand {
///     fn name(&self) -> &'static str {
///         "start"
///     }
///
///     fn execute(&self, ctx: &Context) -> CommandResult {
///         // 실행 로직
///         Ok(())
///     }
/// }
/// ```
pub trait Command: Send + Sync {
    /// 커맨드 이름 (로깅/디버깅용)
    fn name(&self) -> &'static str;

    /// 커맨드 실행
    fn execute(&self, ctx: &Context) -> CommandResult;

    /// 커맨드 설명 (help 출력용)
    fn description(&self) -> &'static str {
        ""
    }

    /// 이 커맨드가 서버 실행을 필요로 하는지
    fn requires_running_server(&self) -> bool {
        false
    }
}

/// 비동기 커맨드 인터페이스
///
/// 네트워크 I/O나 긴 작업이 필요한 커맨드용
#[async_trait::async_trait]
pub trait AsyncCommand: Send + Sync {
    /// 커맨드 이름
    fn name(&self) -> &'static str;

    /// 비동기 커맨드 실행
    async fn execute(&self, ctx: &Context) -> CommandResult;

    /// 커맨드 설명
    fn description(&self) -> &'static str {
        ""
    }

    /// 이 커맨드가 서버 실행을 필요로 하는지
    fn requires_running_server(&self) -> bool {
        false
    }
}

// ============================================================
// Command Result
// ============================================================

/// 커맨드 실행 결과
///
/// `ZetaError`를 사용하여 통합 에러 프레임워크와 일관성 유지
pub type CommandResult = Result<(), ZetaError>;

// ============================================================
// ZetaError Helpers (CLI 전용)
// ============================================================

/// CLI 에러 생성 헬퍼
pub mod cli_error {
    use super::ZetaError;

    /// 실행 에러 생성
    pub fn execution(msg: impl Into<String>) -> ZetaError {
        ZetaError::internal(msg)
    }

    /// 설정 에러 생성
    pub fn config(msg: impl Into<String>) -> ZetaError {
        ZetaError::config_parse(msg)
    }

    /// 연결 에러 생성
    pub fn connection(msg: impl Into<String>) -> ZetaError {
        ZetaError::connection(msg)
    }

    /// 인증 에러 생성
    pub fn auth(msg: impl Into<String>) -> ZetaError {
        ZetaError::auth_failed(msg)
    }

    /// 검증 에러 생성
    pub fn validation(msg: impl Into<String>) -> ZetaError {
        ZetaError::invalid_argument(msg)
    }

    /// 타임아웃 에러 생성
    pub fn timeout(msg: impl Into<String>) -> ZetaError {
        ZetaError::connection_timeout(msg)
    }
}

// ============================================================
// Context
// ============================================================

/// CLI 실행 컨텍스트
///
/// 모든 커맨드에 공통으로 전달되는 실행 환경 정보
#[derive(Debug, Clone)]
pub struct Context {
    /// Zeta4j 홈 디렉토리
    pub home_dir: PathBuf,
    /// Verbose 모드
    pub verbose: bool,
    /// JSON 출력 모드
    pub json_output: bool,
    /// 컬러 출력 비활성화
    pub no_color: bool,
}

impl Default for Context {
    fn default() -> Self {
        Self::new(
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".zeta4j"),
        )
    }
}

impl Context {
    /// 새 컨텍스트 생성
    pub fn new(home_dir: PathBuf) -> Self {
        Self {
            home_dir,
            verbose: false,
            json_output: false,
            no_color: false,
        }
    }

    /// Builder 패턴 시작
    pub fn builder() -> ContextBuilder {
        ContextBuilder::default()
    }
}

// ============================================================
// Context Builder
// ============================================================

/// Context 빌더
#[derive(Debug, Default)]
pub struct ContextBuilder {
    home_dir: Option<PathBuf>,
    verbose: bool,
    json_output: bool,
    no_color: bool,
}

impl ContextBuilder {
    pub fn home_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.home_dir = Some(path.into());
        self
    }

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn json_output(mut self, json: bool) -> Self {
        self.json_output = json;
        self
    }

    pub fn no_color(mut self, no_color: bool) -> Self {
        self.no_color = no_color;
        self
    }

    pub fn build(self) -> Context {
        let mut context = self.home_dir.map_or_else(Context::default, Context::new);
        context.verbose = self.verbose;
        context.json_output = self.json_output;
        context.no_color = self.no_color;
        context
    }
}

// ============================================================
// Output Formatting
// ============================================================

/// 출력 포맷터
pub struct Output<'a> {
    ctx: &'a Context,
}

impl<'a> Output<'a> {
    pub fn new(ctx: &'a Context) -> Self {
        Self { ctx }
    }

    /// 성공 메시지 출력
    pub fn success(&self, msg: &str) {
        if self.ctx.json_output {
            println!(r#"{{"status":"success","message":"{}"}}"#, msg);
        } else if self.ctx.no_color {
            println!("[OK] {}", msg);
        } else {
            println!("\x1b[32m✓\x1b[0m {}", msg);
        }
    }

    /// 에러 메시지 출력
    pub fn error(&self, msg: &str) {
        if self.ctx.json_output {
            eprintln!(r#"{{"status":"error","message":"{}"}}"#, msg);
        } else if self.ctx.no_color {
            eprintln!("[ERROR] {}", msg);
        } else {
            eprintln!("\x1b[31m✗\x1b[0m {}", msg);
        }
    }

    /// 경고 메시지 출력
    pub fn warning(&self, msg: &str) {
        if self.ctx.json_output {
            println!(r#"{{"status":"warning","message":"{}"}}"#, msg);
        } else if self.ctx.no_color {
            println!("[WARN] {}", msg);
        } else {
            println!("\x1b[33m!\x1b[0m {}", msg);
        }
    }

    /// 정보 메시지 출력
    pub fn info(&self, msg: &str) {
        if self.ctx.json_output {
            println!(r#"{{"status":"info","message":"{}"}}"#, msg);
        } else if self.ctx.no_color {
            println!("[INFO] {}", msg);
        } else {
            println!("\x1b[34mℹ\x1b[0m {}", msg);
        }
    }

    /// Verbose 메시지 (verbose 모드에서만 출력)
    pub fn verbose(&self, msg: &str) {
        if self.ctx.verbose {
            if self.ctx.no_color {
                println!("[DEBUG] {}", msg);
            } else {
                println!("\x1b[90m» {}\x1b[0m", msg);
            }
        }
    }

    /// 키-값 쌍 출력
    pub fn key_value(&self, key: &str, value: &str) {
        if self.ctx.json_output {
            println!(r#"{{"{}":"{}"}}"#, key, value);
        } else if self.ctx.no_color {
            println!("{}: {}", key, value);
        } else {
            println!("\x1b[1m{}\x1b[0m: {}", key, value);
        }
    }
}

// ============================================================
// Command Runner
// ============================================================

/// 커맨드 실행기
///
/// 커맨드를 실행하고 에러를 처리합니다.
pub struct CommandRunner {
    ctx: Context,
}

impl CommandRunner {
    pub fn new(ctx: Context) -> Self {
        Self { ctx }
    }

    /// 동기 커맨드 실행
    pub fn run(&self, cmd: &dyn Command) -> ExitCode {
        let output = Output::new(&self.ctx);

        if self.ctx.verbose {
            output.verbose(&format!("Executing command: {}", cmd.name()));
        }

        match cmd.execute(&self.ctx) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                output.error(&e.to_string());
                e.exit_code()
            }
        }
    }

    /// 비동기 커맨드 실행
    pub async fn run_async(&self, cmd: &dyn AsyncCommand) -> ExitCode {
        let output = Output::new(&self.ctx);

        if self.ctx.verbose {
            output.verbose(&format!("Executing async command: {}", cmd.name()));
        }

        match cmd.execute(&self.ctx).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                output.error(&e.to_string());
                e.exit_code()
            }
        }
    }
}
