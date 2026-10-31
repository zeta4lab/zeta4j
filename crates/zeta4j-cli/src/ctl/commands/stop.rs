// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! stop 명령 구현

use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};
use zeta4j_common::RuntimeProfile;

use crate::ctl::utils::{is_process_running, kill_process, read_pid_file, remove_pid_file};

use super::runtime_state::remove_runtime_state;
use super::start::{DaemonOptions, cmd_start, find_server_binary, inspect_effective_config};

/// 기본 타임아웃 (초)
pub const DEFAULT_STOP_TIMEOUT: u64 = 30;
pub const DEFAULT_START_TIMEOUT: u64 = 30;

/// 서버 종료
pub fn cmd_stop(home: PathBuf, force: bool, timeout: u64) -> ExitCode {
    let pid = match read_pid_file(&home) {
        Some(p) => p,
        None => {
            if let Err(error) = remove_runtime_state(&home) {
                eprintln!("Error: {error}");
                return ExitCode::FAILURE;
            }
            println!("Zeta4j is not running");
            return ExitCode::SUCCESS;
        }
    };

    if !is_process_running(pid) {
        if let Err(error) = remove_pid_file(&home) {
            eprintln!("Error: failed to remove stale PID file: {error}");
            return ExitCode::FAILURE;
        }
        if let Err(error) = remove_runtime_state(&home) {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
        println!("Zeta4j is not running (stale PID file removed)");
        return ExitCode::SUCCESS;
    }

    if force {
        println!("Force stopping Zeta4j (PID: {})...", pid);
        if !kill_process(pid, true) {
            eprintln!("Error: failed to kill Zeta4j process {pid}");
            return ExitCode::FAILURE;
        }
        let wait_start = Instant::now();
        while wait_start.elapsed() < Duration::from_secs(5) && is_process_running(pid) {
            thread::sleep(Duration::from_millis(100));
        }
        if is_process_running(pid) {
            eprintln!("Error: Zeta4j process {pid} is still running");
            return ExitCode::FAILURE;
        }
        if !cleanup_state(&home) {
            return ExitCode::FAILURE;
        }
        println!("Zeta4j killed");
        return ExitCode::SUCCESS;
    }

    println!("Stopping Zeta4j (PID: {})...", pid);
    if !kill_process(pid, false) {
        eprintln!("Error: failed to signal Zeta4j process {pid}");
        return ExitCode::FAILURE;
    }

    // Graceful 종료 대기
    let start = Instant::now();
    let deadline = Duration::from_secs(timeout);

    while start.elapsed() < deadline {
        if !is_process_running(pid) {
            if !cleanup_state(&home) {
                return ExitCode::FAILURE;
            }
            println!("Zeta4j stopped");
            return ExitCode::SUCCESS;
        }
        thread::sleep(Duration::from_millis(500));
    }

    // 타임아웃 - 강제 종료
    println!("Graceful shutdown timed out, force killing...");
    if !kill_process(pid, true) {
        eprintln!("Error: failed to kill Zeta4j process {pid}");
        return ExitCode::FAILURE;
    }
    let force_start = Instant::now();
    while force_start.elapsed() < Duration::from_secs(5) && is_process_running(pid) {
        thread::sleep(Duration::from_millis(100));
    }
    if is_process_running(pid) {
        eprintln!("Error: Zeta4j process {pid} is still running");
        return ExitCode::FAILURE;
    }
    if !cleanup_state(&home) {
        return ExitCode::FAILURE;
    }
    println!("Zeta4j killed");

    ExitCode::SUCCESS
}

/// 서버 재시작
#[expect(clippy::too_many_arguments)]
pub fn cmd_restart(
    home: PathBuf,
    server_path: Option<PathBuf>,
    host: Option<String>,
    http_port: Option<u16>,
    bolt_port: Option<u16>,
    grpc_port: Option<u16>,
    force: bool,
    timezone: Option<String>,
    no_auth: bool,
    profile: Option<RuntimeProfile>,
) -> ExitCode {
    println!("Restarting Zeta4j...");

    let server_bin = match find_server_binary(server_path.as_deref()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    let options = DaemonOptions {
        profile,
        host: host.clone(),
        http_port,
        bolt_port,
        grpc_port,
        timezone: timezone.clone(),
        no_auth,
    };
    if let Err(error) = inspect_effective_config(&server_bin, &home, &options) {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }

    // 먼저 종료
    if cmd_stop(home.clone(), force, DEFAULT_STOP_TIMEOUT) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }

    // 잠시 대기
    thread::sleep(Duration::from_secs(1));

    // 시작 (restart는 이미 stop을 했으므로 replace 불필요)
    // 참고: restart는 HTTPS 인증서 옵션을 지원하지 않음 (start 명령 사용 필요)
    cmd_start(
        home,
        server_path,
        host,
        http_port,
        bolt_port,
        grpc_port,
        DEFAULT_START_TIMEOUT,
        false,
        timezone,
        no_auth,
        profile,
    )
}

fn cleanup_state(home: &std::path::Path) -> bool {
    if let Err(error) = remove_pid_file(home) {
        eprintln!("Error: failed to remove PID file: {error}");
        return false;
    }
    if let Err(error) = remove_runtime_state(home) {
        eprintln!("Error: {error}");
        return false;
    }
    true
}
