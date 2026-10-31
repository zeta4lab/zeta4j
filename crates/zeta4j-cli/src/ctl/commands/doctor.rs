// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Unified environment and running-server diagnosis.

use std::path::Path;
use std::process::ExitCode;

use crate::ctl::utils::{
    check_bolt_handshake, check_health, check_http_ready, is_process_running, read_pid_file,
};

use super::check::{cmd_check_ports, cmd_check_resources};
use super::config::cmd_config_check;
use super::doctor_report::collect_doctor_report;
use super::runtime_state::read_runtime_state;

/// Diagnose configuration, resources, and either the running server or port readiness.
pub fn cmd_doctor(home: &Path, output: Option<&Path>) -> ExitCode {
    println!("Zeta4j Doctor");
    println!("=============");
    println!();

    let config_ok = cmd_config_check(home, None) == ExitCode::SUCCESS;
    println!();
    let resources_ok = cmd_check_resources(home) == ExitCode::SUCCESS;
    println!();

    let process_ok = match read_pid_file(home) {
        Some(pid) if is_process_running(pid) => {
            let state = match read_runtime_state(home) {
                Ok(state) if state.pid == pid => state,
                Ok(state) => {
                    eprintln!(
                        "Error: runtime state PID {} does not match running PID {pid}",
                        state.pid
                    );
                    return ExitCode::FAILURE;
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    return ExitCode::FAILURE;
                }
            };
            println!("Running server: PID {pid}");
            let tcp_http = check_health(&state.http_host, state.http_port);
            let http_ready = check_http_ready(&state.http_host, state.http_port);
            let tcp_bolt = check_health(&state.bolt_host, state.bolt_port);
            let bolt_handshake = check_bolt_handshake(&state.bolt_host, state.bolt_port);
            let grpc_ready = check_health(&state.grpc_host, state.grpc_port);
            println!(
                "  HTTP {}: {}",
                state.http_port,
                if tcp_http && http_ready {
                    "healthy"
                } else {
                    "unhealthy"
                }
            );
            println!(
                "  Bolt {}: {}",
                state.bolt_port,
                if tcp_bolt && bolt_handshake {
                    "healthy"
                } else {
                    "unhealthy"
                }
            );
            println!(
                "  gRPC {}: {}",
                state.grpc_port,
                if grpc_ready { "healthy" } else { "unhealthy" }
            );
            tcp_http && http_ready && tcp_bolt && bolt_handshake && grpc_ready
        }
        Some(pid) => {
            eprintln!("Error: stale PID file refers to non-running process {pid}");
            false
        }
        None => {
            println!("Server is stopped; checking whether default ports are available.");
            cmd_check_ports(9044, 9045) == ExitCode::SUCCESS && !check_health("127.0.0.1", 9046)
        }
    };

    let collection_ok = if let Some(output) = output {
        println!();
        collect_doctor_report(home, output) == ExitCode::SUCCESS
    } else {
        true
    };

    println!();
    if config_ok && resources_ok && process_ok && collection_ok {
        println!("Doctor result: PASSED");
        ExitCode::SUCCESS
    } else {
        eprintln!("Doctor result: FAILED");
        ExitCode::FAILURE
    }
}
