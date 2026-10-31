// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! check 명령 구현

use std::fs;
use std::process::ExitCode;

use crate::ctl::utils::{
    check_health, find_process_using_port, get_disk_space_mb, get_free_memory_mb,
};
use std::path::Path;

#[cfg(unix)]
use crate::ctl::utils::get_ulimit_nofile;

/// 포트 충돌 검사
pub fn cmd_check_ports(http_port: u16, bolt_port: u16) -> ExitCode {
    println!("Checking port availability...");
    println!();

    let mut all_ok = true;

    // HTTP 포트 체크
    print!("  HTTP port {}:  ", http_port);
    if check_health("127.0.0.1", http_port) {
        println!("\x1b[31mIN USE\x1b[0m");
        if let Some(pid) = find_process_using_port(http_port) {
            println!("    └─ Process PID: {}", pid);
        }
        all_ok = false;
    } else {
        println!("\x1b[32mAVAILABLE\x1b[0m");
    }

    // Bolt 포트 체크
    print!("  Bolt port {}:  ", bolt_port);
    if check_health("127.0.0.1", bolt_port) {
        println!("\x1b[31mIN USE\x1b[0m");
        if let Some(pid) = find_process_using_port(bolt_port) {
            println!("    └─ Process PID: {}", pid);
        }
        all_ok = false;
    } else {
        println!("\x1b[32mAVAILABLE\x1b[0m");
    }

    println!();

    if all_ok {
        println!("Port check \x1b[32mPASSED\x1b[0m");
        ExitCode::SUCCESS
    } else {
        println!("Port check \x1b[31mFAILED\x1b[0m");
        println!("Use different ports or stop the conflicting process.");
        ExitCode::FAILURE
    }
}

/// 시스템 리소스 검사
pub fn cmd_check_resources(home: &Path) -> ExitCode {
    println!("Checking system resources...");
    println!();

    let mut all_ok = true;
    let mut warnings = Vec::new();

    // 디스크 공간 체크 (홈 디렉토리)
    print!("  Disk space:    ");
    match get_disk_space_mb() {
        Some(mb) => {
            if mb < 1024 {
                println!("\x1b[31m{} MB\x1b[0m (minimum 1GB recommended)", mb);
                all_ok = false;
            } else if mb < 5120 {
                println!("\x1b[33m{} MB\x1b[0m", mb);
                warnings.push("Low disk space - consider freeing some space");
            } else {
                println!("\x1b[32m{} MB\x1b[0m", mb);
            }
        }
        None => {
            println!("\x1b[33mUNKNOWN\x1b[0m");
            warnings.push("Could not determine disk space");
        }
    }

    // 메모리 체크
    print!("  Free memory:   ");
    match get_free_memory_mb() {
        Some(mb) => {
            if mb < 256 {
                println!("\x1b[31m{} MB\x1b[0m (minimum 512MB recommended)", mb);
                all_ok = false;
            } else if mb < 512 {
                println!("\x1b[33m{} MB\x1b[0m", mb);
                warnings.push("Low memory available");
            } else {
                println!("\x1b[32m{} MB\x1b[0m", mb);
            }
        }
        None => {
            println!("\x1b[33mUNKNOWN\x1b[0m");
            warnings.push("Could not determine free memory");
        }
    }

    // 파일 핸들 제한 체크 (Unix)
    #[cfg(unix)]
    {
        print!("  File handles:  ");
        match get_ulimit_nofile() {
            Some(limit) => {
                if limit < 1024 {
                    println!("\x1b[31m{}\x1b[0m (minimum 1024 recommended)", limit);
                    all_ok = false;
                } else if limit < 65536 {
                    println!("\x1b[33m{}\x1b[0m", limit);
                    warnings.push("Consider increasing ulimit -n for production");
                } else {
                    println!("\x1b[32m{}\x1b[0m", limit);
                }
            }
            None => {
                println!("\x1b[33mUNKNOWN\x1b[0m");
            }
        }
    }

    // 홈 디렉토리 쓰기 권한 체크
    print!("  Write access:  ");
    if fs::create_dir_all(home).is_err() {
        println!("\x1b[31mNO\x1b[0m ({})", home.display());
        all_ok = false;
    } else {
        let test_file = home.join(".write_test");
        match fs::write(&test_file, "test") {
            Ok(_) => {
                let _ = fs::remove_file(&test_file);
                println!("\x1b[32mOK\x1b[0m ({})", home.display());
            }
            Err(_) => {
                println!("\x1b[31mNO\x1b[0m ({})", home.display());
                all_ok = false;
            }
        }
    }

    println!();

    if !warnings.is_empty() {
        println!("\x1b[33mWarnings:\x1b[0m");
        for warning in &warnings {
            println!("  ⚠ {}", warning);
        }
        println!();
    }

    if all_ok {
        println!("Resource check \x1b[32mPASSED\x1b[0m");
        ExitCode::SUCCESS
    } else {
        println!("Resource check \x1b[31mFAILED\x1b[0m");
        ExitCode::FAILURE
    }
}

/// 전체 환경 검사
pub fn cmd_check_all(home: &Path, http_port: u16, bolt_port: u16) -> ExitCode {
    println!("═══════════════════════════════════════════════════════");
    println!("  Zeta4j Environment Check");
    println!("═══════════════════════════════════════════════════════");
    println!();

    let ports_ok = cmd_check_ports(http_port, bolt_port) == ExitCode::SUCCESS;
    println!();

    let resources_ok = cmd_check_resources(home) == ExitCode::SUCCESS;
    println!();

    println!("═══════════════════════════════════════════════════════");
    if ports_ok && resources_ok {
        println!("  Overall: \x1b[32mPASSED\x1b[0m - Ready to start Zeta4j");
    } else {
        println!("  Overall: \x1b[31mFAILED\x1b[0m - Please fix the issues above");
    }
    println!("═══════════════════════════════════════════════════════");

    if ports_ok && resources_ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
