// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! start 명령 구현

use std::env;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use zeta4j_common::RuntimeProfile;
use zeta4j_common::private_fs::{ensure_private_dir, write_private_file};

use crate::ctl::utils::{
    check_health, find_process_using_port, get_log_path, is_process_running, kill_process,
    read_pid_file, remove_pid_file, write_pid_file,
};

use super::runtime_state::{
    RuntimeState, bolt_url, remove_runtime_state, web_url, write_runtime_state,
};
use super::status::{TlsProbe, probe_endpoints};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct EffectiveConfigInspection {
    pub http_address: String,
    pub bolt_address: String,
    pub grpc_address: String,
    pub tls_enabled: bool,
    pub config_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct DaemonOptions {
    pub profile: Option<RuntimeProfile>,
    pub host: Option<String>,
    pub http_port: Option<u16>,
    pub bolt_port: Option<u16>,
    pub grpc_port: Option<u16>,
    pub timezone: Option<String>,
    pub no_auth: bool,
}

fn append_daemon_options(command: &mut Command, options: &DaemonOptions) {
    if let Some(profile) = options.profile {
        command.args(["--profile", profile.as_str()]);
    }
    if let Some(value) = options.host.as_deref() {
        command.args(["--host", value]);
    }
    for (name, value) in [
        ("--http-port", options.http_port),
        ("--bolt-port", options.bolt_port),
        ("--grpc-port", options.grpc_port),
    ] {
        if let Some(value) = value {
            command.args([name, &value.to_string()]);
        }
    }
    if let Some(value) = options.timezone.as_deref() {
        command.args(["--timezone", value]);
    }
    if options.no_auth {
        command.arg("--no-auth");
    }
}

pub(super) fn daemon_command(server_bin: &Path, home: &Path, options: &DaemonOptions) -> Command {
    let mut command = Command::new(server_bin);
    command
        .env("ZETA4J_HOME", home)
        .env("ZETA4J_DATA_DIR", home.join("databases"))
        .env("ZETA4J_LOG_DIR", home.join("logs"))
        .args(["--config-dir", &home.join("config").to_string_lossy()]);
    append_daemon_options(&mut command, options);
    command
}

pub(super) fn inspect_effective_config(
    server_bin: &Path,
    home: &Path,
    options: &DaemonOptions,
) -> Result<EffectiveConfigInspection, String> {
    let output = daemon_command(server_bin, home, options)
        .arg("--effective-config-json")
        .output()
        .map_err(|error| format!("cannot inspect {}: {error}", server_bin.display()))?;
    if !output.status.success() {
        return Err(format!(
            "effective configuration inspection failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid effective configuration output: {error}"))
}

pub(super) fn parse_endpoint(value: &str) -> Result<(String, u16), String> {
    let (host, port) = value
        .rsplit_once(':')
        .ok_or_else(|| format!("invalid effective address '{value}'"))?;
    let port = port
        .parse::<u16>()
        .map_err(|error| format!("invalid effective address '{value}': {error}"))?;
    Ok((host.trim_matches(['[', ']']).to_string(), port))
}

fn server_binary_name() -> String {
    format!("zeta4jd{}", env::consts::EXE_SUFFIX)
}

/// `zeta4jd` 경로를 해석한다: 명시 경로, `z4j`와 같은 디렉토리, `PATH` 순서.
pub fn find_server_binary(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        if !path.is_file() {
            return Err(format!("server path is not a file: {}", path.display()));
        }
        return Ok(path.to_path_buf());
    }

    let executable = env::current_exe()
        .map_err(|error| format!("cannot determine current z4j path: {error}"))?;
    let mut candidates = Vec::new();
    if let Some(parent) = executable.parent() {
        candidates.push(parent.join(server_binary_name()));
    }
    if let Some(path) = env::var_os("PATH") {
        candidates
            .extend(env::split_paths(&path).map(|directory| directory.join(server_binary_name())));
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            "zeta4jd was not found next to z4j or on PATH; pass --server-path".to_owned()
        })
}

/// 서버 시작
#[expect(clippy::too_many_arguments)]
pub fn cmd_start(
    home: PathBuf,
    server_path: Option<PathBuf>,
    host: Option<String>,
    http_port: Option<u16>,
    bolt_port: Option<u16>,
    grpc_port: Option<u16>,
    timeout: u64,
    replace: bool,
    timezone: Option<String>,
    no_auth: bool,
    profile: Option<RuntimeProfile>,
) -> ExitCode {
    let options = DaemonOptions {
        profile,
        host,
        http_port,
        bolt_port,
        grpc_port,
        timezone,
        no_auth,
    };

    let server_bin = match find_server_binary(server_path.as_deref()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let inspection = match inspect_effective_config(&server_bin, &home, &options) {
        Ok(inspection) => inspection,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };

    // 이미 실행 중인지 확인
    if let Some(pid) = read_pid_file(&home) {
        if is_process_running(pid) {
            if replace {
                println!("Stopping existing Zeta4j server (PID: {})...", pid);
                kill_process(pid, false);

                // Graceful 종료 대기 (최대 10초)
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(10) {
                    if !is_process_running(pid) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(500));
                }

                // 아직 살아있으면 강제 종료
                if is_process_running(pid) {
                    println!("Force killing...");
                    kill_process(pid, true);
                    thread::sleep(Duration::from_secs(1));
                }

                if let Err(error) = remove_pid_file(&home) {
                    eprintln!("Error: failed to remove stale PID file: {error}");
                    return ExitCode::FAILURE;
                }
                if let Err(error) = remove_runtime_state(&home) {
                    eprintln!("Error: {error}");
                    return ExitCode::FAILURE;
                }
                println!("Previous server stopped");
            } else {
                eprintln!("Zeta4j is already running (PID: {})", pid);
                eprintln!("Use --replace to stop and replace the existing server");
                return ExitCode::FAILURE;
            }
        } else {
            // Stale PID 파일 제거
            if let Err(error) = remove_pid_file(&home) {
                eprintln!("Error: failed to remove stale PID file: {error}");
                return ExitCode::FAILURE;
            }
            if let Err(error) = remove_runtime_state(&home) {
                eprintln!("Error: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    let (http_host, http_port) = match parse_endpoint(&inspection.http_address) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (bolt_host, bolt_port) = match parse_endpoint(&inspection.bolt_address) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (grpc_host, grpc_port) = match parse_endpoint(&inspection.grpc_address) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let endpoints = vec![
        (
            &http_host,
            http_port,
            if inspection.tls_enabled {
                "HTTPS"
            } else {
                "HTTP"
            },
        ),
        (&bolt_host, bolt_port, "Bolt"),
        (&grpc_host, grpc_port, "gRPC"),
    ];
    let mut seen = std::collections::HashSet::new();
    if let Some((_, port, _)) = endpoints
        .iter()
        .find(|(host, port, _)| !seen.insert((host.as_str(), *port)))
    {
        eprintln!("Error: server endpoint addresses must be distinct (duplicate port: {port})");
        return ExitCode::FAILURE;
    }
    for (host, port, name) in endpoints {
        if let Err(error) = prepare_port(host, port, name, replace) {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    }

    // 로그 디렉토리와 파일은 서버의 owner-only runtime 정책보다 먼저 준비된다.
    let log_path = get_log_path(&home);
    let log_file = match create_owner_only_log(&log_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: Failed to create log file: {}", e);
            return ExitCode::FAILURE;
        }
    };
    let log_file_err = match log_file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: Failed to clone log file: {}", e);
            return ExitCode::FAILURE;
        }
    };

    // 서버 시작
    println!("Starting Zeta4j...");
    let mut cmd = daemon_command(&server_bin, &home, &options);
    if let Some(fingerprint) = inspection.config_fingerprint.as_deref() {
        cmd.args(["--expected-config-fingerprint", fingerprint]);
    }

    // 백그라운드 서버를 호출 터미널의 프로세스 그룹에서 분리한다.
    #[cfg(unix)]
    cmd.process_group(0);

    let child = cmd
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err))
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: Failed to start server: {}", e);
            return ExitCode::FAILURE;
        }
    };

    let pid = child.id();
    let runtime_state = RuntimeState {
        pid,
        http_host: http_host.clone(),
        https_host: http_host.clone(),
        bolt_host: bolt_host.clone(),
        grpc_host: grpc_host.clone(),
        http_port,
        https_port: inspection.tls_enabled.then_some(http_port),
        bolt_port,
        grpc_port,
    };
    // Readiness uses the same transport-aware probes as `z4j server status`.
    let tls_probe = runtime_state
        .https_port
        .and_then(|_| TlsProbe::from_environment(&runtime_state.https_host).ok());

    // PID 파일 저장
    if let Err(error) = write_pid_file(&home, pid) {
        let _ = kill_process(pid, true);
        eprintln!("Error: failed to write PID file: {error}");
        return ExitCode::FAILURE;
    }

    print!("Waiting for server to be ready");
    let start = Instant::now();
    let deadline = Duration::from_secs(timeout);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                println!(" FAILED");
                let _ = remove_pid_file(&home);
                let _ = remove_runtime_state(&home);
                eprintln!("Error: Server exited before becoming ready ({status})");
                eprintln!("Check logs: z4j server logs");
                return ExitCode::FAILURE;
            }
            Ok(None) => {}
            Err(error) => {
                println!(" FAILED");
                abort_started_process(&home, pid);
                eprintln!("Error: Failed to inspect server process: {error}");
                return ExitCode::FAILURE;
            }
        }

        if start.elapsed() > deadline {
            println!(" TIMEOUT");
            eprintln!(
                "Error: Server did not become ready within {} seconds",
                timeout
            );
            eprintln!("Check logs: z4j server logs");
            abort_started_process(&home, pid);
            return ExitCode::FAILURE;
        }

        if probe_endpoints(&runtime_state, tls_probe.as_ref()).healthy() {
            println!(" OK");
            break;
        }

        // 프로세스가 종료되었는지 확인
        if !is_process_running(pid) {
            println!(" FAILED");
            eprintln!("Error: Server process terminated unexpectedly");
            eprintln!("Check logs: z4j server logs");
            let _ = remove_pid_file(&home);
            let _ = remove_runtime_state(&home);
            return ExitCode::FAILURE;
        }

        print!(".");
        if let Err(error) = std::io::stdout().flush() {
            eprintln!("Error: failed to flush progress output: {error}");
            abort_started_process(&home, pid);
            return ExitCode::FAILURE;
        }
        thread::sleep(Duration::from_millis(500));
    }

    if let Err(error) = write_runtime_state(&home, &runtime_state) {
        let _ = kill_process(pid, true);
        let _ = remove_pid_file(&home);
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }

    println!();
    println!("Zeta4j started successfully");
    println!("  PID:       {}", pid);
    let tls = inspection.tls_enabled;
    println!(
        "  {:<10} {}",
        if tls { "HTTPS:" } else { "HTTP:" },
        web_url(tls, &http_host, http_port)
    );
    println!("  Bolt:      {}", bolt_url(tls, &bolt_host, bolt_port));
    println!("  gRPC:      {}:{}", grpc_host, grpc_port);
    println!("  Logs:      {}", log_path.display());

    ExitCode::SUCCESS
}

pub(super) fn create_owner_only_log(path: &Path) -> std::io::Result<File> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "log path requires a parent directory",
        )
    })?;
    ensure_private_dir(parent)?;
    write_private_file(path, b"")?;

    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path)
}

fn abort_started_process(home: &Path, pid: u32) {
    let _ = kill_process(pid, true);
    let _ = remove_pid_file(home);
    let _ = remove_runtime_state(home);
}

pub(super) fn prepare_port(host: &str, port: u16, name: &str, replace: bool) -> Result<(), String> {
    if !check_health(host, port) {
        return Ok(());
    }
    if !replace {
        return Err(format!(
            "{name} port {port} is already in use; use --replace to take it over"
        ));
    }
    let pid = find_process_using_port(port)
        .ok_or_else(|| format!("could not identify the process using {name} port {port}"))?;
    println!("Stopping process {pid} using {name} port {port}...");
    if !kill_process(pid, false) {
        return Err(format!(
            "failed to stop process {pid} on {name} port {port}"
        ));
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) && is_process_running(pid) {
        thread::sleep(Duration::from_millis(250));
    }
    if is_process_running(pid) {
        if !kill_process(pid, true) {
            return Err(format!(
                "failed to kill process {pid} on {name} port {port}"
            ));
        }
        thread::sleep(Duration::from_millis(250));
    }
    if check_health(host, port) {
        return Err(format!("{name} port {port} is still in use"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::create_owner_only_log;

    #[cfg(unix)]
    #[test]
    fn daemon_log_is_created_owner_only() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir()?;
        let path = directory.path().join("logs").join("zeta4j.log");
        let file = create_owner_only_log(&path)?;
        drop(file);

        assert_eq!(std::fs::metadata(path)?.permissions().mode() & 0o777, 0o600);
        Ok(())
    }
}
