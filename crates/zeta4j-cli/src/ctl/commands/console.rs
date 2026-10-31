// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! console 명령 구현

use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use zeta4j_common::RuntimeProfile;

use crate::ctl::utils::{is_process_running, kill_process, read_pid_file, remove_pid_file};

use super::runtime_state::{RuntimeState, remove_runtime_state, write_runtime_state};
use super::start::{
    DaemonOptions, daemon_command, find_server_binary, inspect_effective_config, parse_endpoint,
    prepare_port,
};

/// 포그라운드 실행
#[expect(clippy::too_many_arguments)]
pub fn cmd_console(
    home: PathBuf,
    server_path: Option<PathBuf>,
    host: Option<String>,
    http_port: Option<u16>,
    bolt_port: Option<u16>,
    grpc_port: Option<u16>,
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
    // 이미 실행 중인 서버 확인
    if let Some(pid) = read_pid_file(&home) {
        if is_process_running(pid) {
            if replace {
                println!("Stopping existing Zeta4j server (PID: {})...", pid);
                if !kill_process(pid, false) {
                    eprintln!("Error: failed to stop existing process {pid}");
                    return ExitCode::FAILURE;
                }

                // Graceful 종료 대기
                let start = std::time::Instant::now();
                while start.elapsed() < Duration::from_secs(10) {
                    if !is_process_running(pid) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(500));
                }

                if is_process_running(pid) {
                    if !kill_process(pid, true) {
                        eprintln!("Error: failed to force-stop existing process {pid}");
                        return ExitCode::FAILURE;
                    }
                    thread::sleep(Duration::from_secs(1));
                }

                if is_process_running(pid) {
                    eprintln!("Error: existing process {pid} did not stop");
                    return ExitCode::FAILURE;
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
    for (host, port, name) in [
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
    ] {
        if let Err(error) = prepare_port(host, port, name, replace) {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    }

    println!("Starting Zeta4j in console mode...");
    println!("Press Ctrl+C to stop");
    println!();

    let mut cmd = daemon_command(&server_bin, &home, &options);
    if let Some(fingerprint) = inspection.config_fingerprint.as_deref() {
        cmd.args(["--expected-config-fingerprint", fingerprint]);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Error: Failed to start server: {}", e);
            return ExitCode::FAILURE;
        }
    };
    let pid = child.id();
    if let Err(error) = crate::ctl::utils::write_pid_file(&home, pid) {
        let _ = child.kill();
        let _ = child.wait();
        eprintln!("Error: failed to write PID file: {error}");
        return ExitCode::FAILURE;
    }
    let state = RuntimeState {
        pid,
        https_host: http_host.clone(),
        http_host,
        bolt_host,
        grpc_host,
        http_port,
        https_port: inspection.tls_enabled.then_some(http_port),
        bolt_port,
        grpc_port,
    };
    if let Err(error) = write_runtime_state(&home, &state) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = remove_pid_file(&home);
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }

    let status = child.wait();
    let cleanup = remove_pid_file(&home)
        .map_err(|error| format!("failed to remove PID file: {error}"))
        .and_then(|()| remove_runtime_state(&home));
    if let Err(error) = cleanup {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }

    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(
            status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(1),
        ),
        Err(error) => {
            eprintln!("Error: failed to wait for server: {error}");
            ExitCode::FAILURE
        }
    }
}
