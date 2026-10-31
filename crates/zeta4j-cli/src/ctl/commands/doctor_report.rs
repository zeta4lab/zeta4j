// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! `server doctor --output` report collection.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::ctl::utils::{
    check_health, collect_system_info, find_process_using_port, get_log_path, get_process_uptime,
    is_process_running, read_pid_file,
};

use super::runtime_state::read_runtime_state;

/// Doctor report collection.
pub fn collect_doctor_report(home: &Path, output: &Path) -> ExitCode {
    println!("Collecting doctor report...");
    println!();

    // 출력 디렉토리 생성
    if let Err(e) = fs::create_dir_all(output) {
        eprintln!("Error: Failed to create output directory: {}", e);
        return ExitCode::FAILURE;
    }

    let mut collected = Vec::<String>::new();
    let mut all_ok = true;

    // 1. 시스템 정보
    print!("  System info...");
    let system_info = collect_system_info();
    let system_info_path = output.join("system_info.txt");
    match fs::write(&system_info_path, &system_info) {
        Ok(_) => {
            println!(" \x1b[32mOK\x1b[0m");
            collected.push("system_info.txt".to_owned());
        }
        Err(e) => {
            println!(" \x1b[31mFAILED\x1b[0m ({})", e);
            all_ok = false;
        }
    }

    // 2. 로그 파일 (최근 10000줄)
    print!("  Log file...");
    let log_path = get_log_path(home);
    if log_path.exists() {
        match collect_log_tail(&log_path, 10000) {
            Ok(log_content) => {
                let log_output = output.join("zeta4j.log");
                match fs::write(&log_output, log_content) {
                    Ok(_) => {
                        println!(" \x1b[32mOK\x1b[0m");
                        collected.push("zeta4j.log".to_owned());
                    }
                    Err(e) => {
                        println!(" \x1b[31mFAILED\x1b[0m ({})", e);
                        all_ok = false;
                    }
                }
            }
            Err(e) => {
                println!(" \x1b[31mFAILED\x1b[0m ({})", e);
                all_ok = false;
            }
        }
    } else {
        println!(" \x1b[33mNOT FOUND\x1b[0m");
    }

    // 3. 설정 파일 (민감 정보 마스킹)
    print!("  Configuration...");
    match collect_masked_config(home, output) {
        Ok(files) => {
            println!(" \x1b[32mOK\x1b[0m ({} masked files)", files.len());
            collected.extend(files);
        }
        Err(error) => {
            println!(" \x1b[31mFAILED\x1b[0m ({error})");
            all_ok = false;
        }
    }

    // 4. PID 파일 상태
    print!("  Process info...");
    let pid_info = collect_process_info(home);
    let pid_output = output.join("process_info.txt");
    match fs::write(&pid_output, &pid_info) {
        Ok(_) => {
            println!(" \x1b[32mOK\x1b[0m");
            collected.push("process_info.txt".to_owned());
        }
        Err(e) => {
            println!(" \x1b[31mFAILED\x1b[0m ({})", e);
            all_ok = false;
        }
    }

    // 5. 포트 상태
    print!("  Port status...");
    let port_info = collect_port_info(home);
    let port_output = output.join("port_status.txt");
    match fs::write(&port_output, &port_info) {
        Ok(_) => {
            println!(" \x1b[32mOK\x1b[0m");
            collected.push("port_status.txt".to_owned());
        }
        Err(e) => {
            println!(" \x1b[31mFAILED\x1b[0m ({})", e);
            all_ok = false;
        }
    }

    println!();
    println!("Doctor report collected: {}", output.display());
    println!("Files: {}", collected.join(", "));
    println!();
    println!("Note: Sensitive information (passwords, tokens) has been masked.");
    println!("Please review files before sharing with support.");

    if all_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// 로그 파일 tail 수집
fn collect_log_tail(log_path: &PathBuf, max_lines: usize) -> std::io::Result<String> {
    let content = fs::read_to_string(log_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = if lines.len() > max_lines {
        lines.len() - max_lines
    } else {
        0
    };
    Ok(lines[start..].join("\n"))
}

/// 설정에서 민감 정보 마스킹
fn mask_sensitive_config(content: &str) -> String {
    let mut result = Vec::new();

    // 민감한 키워드
    let sensitive_keys = ["password", "token", "secret", "api_key", "credential"];

    for line in content.lines() {
        let trimmed = line.trim();

        // 민감한 키를 포함하는 라인 마스킹
        let should_mask = sensitive_keys
            .iter()
            .any(|key| trimmed.to_lowercase().contains(key));

        if should_mask && trimmed.contains('=') {
            // key = value 형식에서 value만 마스킹
            if let Some(eq_pos) = line.find('=') {
                let key_part = &line[..=eq_pos];
                result.push(format!("{} \"[MASKED]\"", key_part.trim_end()));
            } else {
                result.push(line.to_string());
            }
        } else {
            result.push(line.to_string());
        }
    }

    result.join("\n")
}

fn collect_masked_config(home: &Path, output: &Path) -> Result<Vec<String>, String> {
    let source_dir = home.join("config");
    let target_dir = output.join("config");
    fs::create_dir_all(&target_dir)
        .map_err(|error| format!("failed to create {}: {error}", target_dir.display()))?;
    let entries = fs::read_dir(&source_dir)
        .map_err(|error| format!("failed to read {}: {error}", source_dir.display()))?;
    let mut collected = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("failed to inspect {}: {error}", source_dir.display()))?;
        let source = entry.path();
        if source.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        let name = source
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("invalid configuration filename: {}", source.display()))?;
        let content = fs::read_to_string(&source)
            .map_err(|error| format!("failed to read {}: {error}", source.display()))?;
        let target = target_dir.join(name);
        fs::write(&target, mask_sensitive_config(&content))
            .map_err(|error| format!("failed to write {}: {error}", target.display()))?;
        collected.push(format!("config/{name}"));
    }
    collected.sort();
    if collected.is_empty() {
        return Err(format!("no TOML files found in {}", source_dir.display()));
    }
    Ok(collected)
}

/// 프로세스 정보 수집
fn collect_process_info(home: &Path) -> String {
    let mut info = String::new();

    info.push_str("=== Process Information ===\n");

    if let Some(pid) = read_pid_file(home) {
        info.push_str(&format!("PID: {}\n", pid));
        info.push_str(&format!("Running: {}\n", is_process_running(pid)));

        if is_process_running(pid) {
            if let Some(uptime) = get_process_uptime(pid) {
                info.push_str(&format!("Uptime: {} seconds\n", uptime));
            }

            #[cfg(unix)]
            {
                // ps 상세 정보
                if let Ok(output) = Command::new("ps")
                    .args([
                        "-o",
                        "pid,ppid,user,%cpu,%mem,vsz,rss,stat,start,time,command",
                        "-p",
                        &pid.to_string(),
                    ])
                    .output()
                    && output.status.success()
                {
                    info.push_str(&format!(
                        "\nps output:\n{}\n",
                        String::from_utf8_lossy(&output.stdout)
                    ));
                }
            }
        }
    } else {
        info.push_str("PID: Not found (server not running)\n");
    }

    info
}

/// 포트 상태 정보 수집
fn collect_port_info(home: &Path) -> String {
    let mut info = String::new();

    info.push_str("=== Port Status ===\n");

    let ports = read_runtime_state(home)
        .map(|state| {
            let mut ports = vec![state.http_port, state.bolt_port, state.grpc_port];
            if let Some(https_port) = state.https_port {
                ports.push(https_port);
            }
            ports
        })
        .unwrap_or_else(|_| vec![9044, 9045, 9046]);
    for port in ports {
        let tcp_ok = check_health("127.0.0.1", port);
        info.push_str(&format!(
            "Port {}: {}\n",
            port,
            if tcp_ok { "IN USE" } else { "AVAILABLE" }
        ));

        if tcp_ok && let Some(pid) = find_process_using_port(port) {
            info.push_str(&format!("  PID: {}\n", pid));
        }
    }

    info
}
