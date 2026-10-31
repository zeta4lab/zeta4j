// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! service 명령 구현 (systemd)

use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
use super::start::find_server_binary;

/// systemd 서비스 파일 템플릿
#[cfg(target_os = "linux")]
const SYSTEMD_TEMPLATE: &str = r#"[Unit]
Description=Zeta4j Graph Database Server
Documentation=https://github.com/zeta4j/zeta4j
After=network.target

[Service]
Type=simple
User={user}
Group={user}
ExecStart={ctl_path} server --home {home} console --server-path {exec_path} --host 0.0.0.0
ExecStop={ctl_path} server --home {home} stop
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

# Environment
Environment="ZETA4J_HOME={home_env}"

# Security
PrivateTmp=true
ProtectSystem=full
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
"#;

/// systemd 서비스 설치
#[cfg(target_os = "linux")]
pub fn cmd_service_install(
    home: &Path,
    name: &str,
    user: Option<String>,
    server_path: Option<&std::path::Path>,
) -> ExitCode {
    if let Err(error) = validate_service_name(name) {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }
    let user =
        user.unwrap_or_else(|| std::env::var("USER").unwrap_or_else(|_| "zeta4j".to_string()));
    let mut user_characters = user.chars();
    let valid_user = user_characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && user_characters
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if !valid_user {
        eprintln!(
            "Error: service user must begin with an ASCII letter or '_' and contain only letters, digits, '-' or '_'"
        );
        return ExitCode::FAILURE;
    }

    // 실행 파일 경로 찾기
    let exec_path = match find_server_binary(server_path) {
        Ok(path) => path.canonicalize().unwrap_or(path),
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };

    let ctl_path = match std::env::current_exe() {
        Ok(p) => p.canonicalize().unwrap_or(p),
        Err(e) => {
            eprintln!("Error: Could not determine z4j path: {}", e);
            return ExitCode::FAILURE;
        }
    };

    // 템플릿 치환
    let service_content = SYSTEMD_TEMPLATE
        .replace("{user}", &user)
        .replace("{exec_path}", &quote_systemd_arg(&exec_path))
        .replace("{ctl_path}", &quote_systemd_arg(&ctl_path))
        .replace("{home}", &quote_systemd_arg(home))
        .replace("{home_env}", &escape_systemd_value(&home.to_string_lossy()));

    // 서비스 파일 경로
    let service_path = PathBuf::from(format!("/etc/systemd/system/{}.service", name));

    let previous = match std::fs::read(&service_path) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            eprintln!("Error: failed to read {}: {error}", service_path.display());
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = write_atomic(&service_path, service_content.as_bytes()) {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = run_systemctl(&["daemon-reload"]) {
        let rollback = match previous {
            Some(contents) => write_atomic(&service_path, &contents),
            None => std::fs::remove_file(&service_path)
                .map_err(|rollback_error| rollback_error.to_string()),
        };
        if let Err(rollback_error) = rollback {
            eprintln!("Error: {error}; rollback failed: {rollback_error}");
        } else {
            eprintln!("Error: {error}");
        }
        return ExitCode::FAILURE;
    }

    println!(
        "Installed systemd service '{}': {}",
        name,
        service_path.display()
    );
    ExitCode::SUCCESS
}

#[cfg(not(target_os = "linux"))]
pub fn cmd_service_install(
    _home: &PathBuf,
    _name: &str,
    _user: Option<String>,
    _server_path: Option<&Path>,
) -> ExitCode {
    eprintln!("Error: systemd service management is only available on Linux");
    eprintln!();
    eprintln!("For macOS, consider using launchd.");
    eprintln!("For Windows, consider using Windows Services.");
    ExitCode::FAILURE
}

/// systemd 서비스 제거
#[cfg(target_os = "linux")]
pub fn cmd_service_uninstall(name: &str) -> ExitCode {
    if let Err(error) = validate_service_name(name) {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }
    let service_path = PathBuf::from(format!("/etc/systemd/system/{}.service", name));

    if !service_path.exists() {
        println!("Systemd service '{}' is not installed", name);
        return ExitCode::SUCCESS;
    }
    if let Err(error) = run_systemctl(&["disable", "--now", name]) {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = std::fs::remove_file(&service_path) {
        eprintln!(
            "Error: failed to remove {}: {error}",
            service_path.display()
        );
        return ExitCode::FAILURE;
    }
    if let Err(error) = run_systemctl(&["daemon-reload"]) {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }
    println!("Uninstalled systemd service '{}'", name);
    ExitCode::SUCCESS
}

#[cfg(not(target_os = "linux"))]
pub fn cmd_service_uninstall(_name: &str) -> ExitCode {
    eprintln!("Error: systemd service management is only available on Linux");
    ExitCode::FAILURE
}

/// systemd 서비스 상태
#[cfg(target_os = "linux")]
pub fn cmd_service_status(name: &str) -> ExitCode {
    if let Err(error) = validate_service_name(name) {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }
    println!("Checking systemd service: {}", name);
    println!();

    // systemctl status 실행
    let output = Command::new("systemctl").args(["status", name]).output();

    match output {
        Ok(result) => {
            println!("{}", String::from_utf8_lossy(&result.stdout));
            if !result.stderr.is_empty() {
                eprintln!("{}", String::from_utf8_lossy(&result.stderr));
            }

            if result.status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("Error: Failed to check service status: {}", e);
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
fn validate_service_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("service name must contain only ASCII letters, digits, '-' or '_'".to_string());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn escape_systemd_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "linux")]
fn quote_systemd_arg(path: &Path) -> String {
    format!("\"{}\"", escape_systemd_value(&path.to_string_lossy()))
}

#[cfg(target_os = "linux")]
fn run_systemctl(arguments: &[&str]) -> Result<(), String> {
    let output = Command::new("systemctl")
        .args(arguments)
        .output()
        .map_err(|error| format!("failed to execute systemctl: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!(
                "systemctl {} failed with {}",
                arguments.join(" "),
                output.status
            )
        } else {
            format!("systemctl {} failed: {stderr}", arguments.join(" "))
        })
    }
}

#[cfg(target_os = "linux")]
fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    use std::io::Write;

    let parent = path
        .parent()
        .ok_or_else(|| format!("service path has no parent: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid service path: {}", path.display()))?;
    let partial = parent.join(format!(".{file_name}.partial-{}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)
            .map_err(|error| format!("failed to create {}: {error}", partial.display()))?;
        file.write_all(contents)
            .map_err(|error| format!("failed to write {}: {error}", partial.display()))?;
        file.sync_all()
            .map_err(|error| format!("failed to sync {}: {error}", partial.display()))?;
        drop(file);
        std::fs::rename(&partial, path).map_err(|error| {
            format!(
                "failed to install {} as {}: {error}",
                partial.display(),
                path.display()
            )
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

#[cfg(not(target_os = "linux"))]
pub fn cmd_service_status(_name: &str) -> ExitCode {
    eprintln!("Error: systemd service management is only available on Linux");
    ExitCode::FAILURE
}
