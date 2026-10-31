// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Verbose 및 디버그 모드
//!
//! CLI 도구들의 상세 출력 및 디버그 기능을 제공합니다.
//!
//! ## 사용법
//!
//! ```bash
//! # 기본 verbose 모드
//! z4j server -v status
//!
//! # 상세 verbose 모드
//! z4j server -vv status
//!
//! # 최대 디버그 모드
//! z4j server -vvv status
//!
//! # 디버그 환경변수
//! ZETA4J_DEBUG=1 z4j server status
//! ```

use std::io::Write;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

/// 전역 verbose 레벨
static VERBOSE_LEVEL: AtomicU8 = AtomicU8::new(0);

/// Verbose 레벨
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VerboseLevel {
    /// 기본 (에러만)
    Quiet = 0,
    /// 정보 메시지
    Info = 1,
    /// 상세 정보
    Verbose = 2,
    /// 디버그 정보
    Debug = 3,
    /// 트레이스 (모든 정보)
    Trace = 4,
}

impl From<u8> for VerboseLevel {
    fn from(level: u8) -> Self {
        match level {
            0 => VerboseLevel::Quiet,
            1 => VerboseLevel::Info,
            2 => VerboseLevel::Verbose,
            3 => VerboseLevel::Debug,
            _ => VerboseLevel::Trace,
        }
    }
}

impl VerboseLevel {
    /// 현재 레벨이 주어진 레벨 이상인지 확인
    pub fn is_at_least(&self, level: VerboseLevel) -> bool {
        *self >= level
    }
}

/// Verbose 레벨 설정
pub fn set_verbose_level(level: u8) {
    VERBOSE_LEVEL.store(level, Ordering::SeqCst);
}

/// 현재 verbose 레벨 반환
pub fn get_verbose_level() -> VerboseLevel {
    VerboseLevel::from(VERBOSE_LEVEL.load(Ordering::SeqCst))
}

/// Verbose 레벨 증가 (CLI -v 옵션 처리)
pub fn increase_verbose_level() {
    let current = VERBOSE_LEVEL.load(Ordering::SeqCst);
    VERBOSE_LEVEL.store(current.saturating_add(1).min(4), Ordering::SeqCst);
}

/// 환경변수에서 verbose 레벨 초기화
pub fn init_from_env() {
    if let Ok(val) = std::env::var("ZETA4J_DEBUG") {
        if let Ok(level) = val.parse::<u8>() {
            set_verbose_level(level);
        } else if val == "true" || val == "1" {
            set_verbose_level(3); // Debug level
        }
    }

    if let Ok(val) = std::env::var("ZETA4J_VERBOSE")
        && let Ok(level) = val.parse::<u8>()
    {
        set_verbose_level(level);
    }
}

/// 정보 메시지 출력 (verbose level >= 1)
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        if $crate::verbose::get_verbose_level().is_at_least($crate::verbose::VerboseLevel::Info) {
            eprintln!("{}", format!($($arg)*));
        }
    };
}

/// 상세 메시지 출력 (verbose level >= 2)
#[macro_export]
macro_rules! verbose {
    ($($arg:tt)*) => {
        if $crate::verbose::get_verbose_level().is_at_least($crate::verbose::VerboseLevel::Verbose) {
            let cyan = "\x1b[36m";
            let reset = "\x1b[0m";
            eprintln!("{}[verbose]{} {}", cyan, reset, format!($($arg)*));
        }
    };
}

/// 디버그 메시지 출력 (verbose level >= 3)
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::verbose::get_verbose_level().is_at_least($crate::verbose::VerboseLevel::Debug) {
            let yellow = "\x1b[33m";
            let reset = "\x1b[0m";
            eprintln!("{}[debug]{} {}", yellow, reset, format!($($arg)*));
        }
    };
}

/// 트레이스 메시지 출력 (verbose level >= 4)
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::verbose::get_verbose_level().is_at_least($crate::verbose::VerboseLevel::Trace) {
            let magenta = "\x1b[35m";
            let reset = "\x1b[0m";
            eprintln!("{}[trace]{} {}", magenta, reset, format!($($arg)*));
        }
    };
}

/// 타이밍 측정 헬퍼
pub struct Timer {
    name: String,
    start: Instant,
    level: VerboseLevel,
}

impl Timer {
    /// 새 타이머 시작
    pub fn start(name: impl Into<String>) -> Self {
        Self::start_at_level(name, VerboseLevel::Debug)
    }

    /// 지정된 레벨에서 타이머 시작
    pub fn start_at_level(name: impl Into<String>, level: VerboseLevel) -> Self {
        let name = name.into();
        if get_verbose_level().is_at_least(level) {
            eprintln!("\x1b[90m[timer] {} starting...\x1b[0m", name);
        }
        Self {
            name,
            start: Instant::now(),
            level,
        }
    }

    /// 경과 시간 반환 (밀리초)
    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if get_verbose_level().is_at_least(self.level) {
            let elapsed = self.start.elapsed();
            let ms = elapsed.as_secs_f64() * 1000.0;
            eprintln!(
                "\x1b[90m[timer] {} completed: {:.2}ms\x1b[0m",
                self.name, ms
            );
        }
    }
}

/// 디버그 정보 수집
pub struct DebugInfo {
    /// 시스템 정보
    pub system: SystemInfo,
    /// 환경 변수
    pub env_vars: Vec<(String, String)>,
    /// 설정 경로
    pub config_paths: Vec<String>,
}

/// 시스템 정보
pub struct SystemInfo {
    /// OS 이름
    pub os: String,
    /// OS 버전
    pub os_version: String,
    /// 아키텍처
    pub arch: String,
    /// CPU 코어 수
    pub cpu_cores: usize,
    /// 사용 가능한 메모리 (bytes)
    pub available_memory: Option<u64>,
}

impl DebugInfo {
    /// 디버그 정보 수집
    pub fn collect() -> Self {
        let system = SystemInfo {
            os: std::env::consts::OS.to_string(),
            os_version: os_version(),
            arch: std::env::consts::ARCH.to_string(),
            cpu_cores: num_cpus::get(),
            available_memory: None, // sysinfo 필요시 추가
        };

        let env_vars: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| k.starts_with("ZETA4J_"))
            .collect();

        let mut config_paths = Vec::new();
        if let Some(home) = dirs::home_dir() {
            config_paths.push(home.join(".zeta4j").display().to_string());
        }
        if let Ok(cwd) = std::env::current_dir() {
            config_paths.push(cwd.join("zeta4j.toml").display().to_string());
        }

        Self {
            system,
            env_vars,
            config_paths,
        }
    }

    /// 포맷된 출력
    pub fn format(&self) -> String {
        let mut output = String::new();

        output.push_str("=== System Info ===\n");
        output.push_str(&format!(
            "OS: {} {}\n",
            self.system.os, self.system.os_version
        ));
        output.push_str(&format!("Arch: {}\n", self.system.arch));
        output.push_str(&format!("CPU cores: {}\n", self.system.cpu_cores));

        if !self.env_vars.is_empty() {
            output.push_str("\n=== Environment Variables ===\n");
            for (k, v) in &self.env_vars {
                // 비밀번호 등 민감한 값 마스킹
                let masked_v =
                    if k.contains("PASSWORD") || k.contains("SECRET") || k.contains("KEY") {
                        "***".to_string()
                    } else {
                        v.clone()
                    };
                output.push_str(&format!("{}={}\n", k, masked_v));
            }
        }

        output.push_str("\n=== Config Paths ===\n");
        for path in &self.config_paths {
            let exists = std::path::Path::new(path).exists();
            let status = if exists { "exists" } else { "not found" };
            output.push_str(&format!("{} ({})\n", path, status));
        }

        output
    }
}

/// OS 버전 가져오기
fn os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|line| line.starts_with("VERSION_ID="))
                    .map(|line| {
                        line.trim_start_matches("VERSION_ID=")
                            .trim_matches('"')
                            .to_string()
                    })
            })
            .unwrap_or_else(|| "unknown".to_string())
    }

    #[cfg(target_os = "windows")]
    {
        "unknown".to_string()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "unknown".to_string()
    }
}

/// 진행 상태 표시기
pub struct ProgressIndicator {
    message: String,
    chars: Vec<char>,
    index: usize,
}

impl ProgressIndicator {
    /// 새 진행 표시기 생성
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            chars: vec!['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
            index: 0,
        }
    }

    /// 틱 (다음 프레임)
    pub fn tick(&mut self) {
        if get_verbose_level().is_at_least(VerboseLevel::Info) {
            let char = self.chars[self.index % self.chars.len()];
            eprint!("\r{} {} ", char, self.message);
            let _ = std::io::stderr().flush();
            self.index += 1;
        }
    }

    /// 완료
    pub fn finish(&self, success: bool) {
        if get_verbose_level().is_at_least(VerboseLevel::Info) {
            let (icon, color) = if success {
                ("✓", "\x1b[32m")
            } else {
                ("✗", "\x1b[31m")
            };
            eprintln!("\r{}{}\x1b[0m {} ", color, icon, self.message);
        }
    }
}
