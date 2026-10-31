// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 명령 디스패처
//!
//! CtlCommand를 Context 기반으로 실행합니다.

use std::process::ExitCode;

use crate::traits::Context;

use super::cli::{ConfigCommand, CtlCommand, ServiceCommand};
use super::commands;

impl CtlCommand {
    /// 명령 실행
    pub fn execute(&self, ctx: &Context) -> ExitCode {
        let home = ctx.home_dir.clone();

        match self {
            CtlCommand::Start {
                server_path,
                host,
                http_port,
                bolt_port,
                grpc_port,
                timeout,
                replace,
                timezone,
                no_auth,
                profile,
            } => commands::cmd_start(
                home,
                server_path.clone(),
                host.clone(),
                *http_port,
                *bolt_port,
                *grpc_port,
                *timeout,
                *replace,
                timezone.clone(),
                *no_auth,
                *profile,
            ),

            CtlCommand::Stop { force, timeout } => commands::cmd_stop(home, *force, *timeout),

            CtlCommand::Restart {
                server_path,
                host,
                http_port,
                bolt_port,
                grpc_port,
                force,
                timezone,
                no_auth,
                profile,
            } => commands::cmd_restart(
                home,
                server_path.clone(),
                host.clone(),
                *http_port,
                *bolt_port,
                *grpc_port,
                *force,
                timezone.clone(),
                *no_auth,
                *profile,
            ),

            CtlCommand::Status => commands::cmd_status(home),

            CtlCommand::Logs { follow, lines } => commands::cmd_logs(home, *follow, *lines),

            CtlCommand::Console {
                server_path,
                host,
                http_port,
                bolt_port,
                grpc_port,
                replace,
                timezone,
                no_auth,
                profile,
            } => commands::cmd_console(
                home,
                server_path.clone(),
                host.clone(),
                *http_port,
                *bolt_port,
                *grpc_port,
                *replace,
                timezone.clone(),
                *no_auth,
                *profile,
            ),

            CtlCommand::Config(cmd) => cmd.execute(&home),
            CtlCommand::Doctor { output } => commands::cmd_doctor(&home, output.as_deref()),
            CtlCommand::Service(cmd) => cmd.execute(&home),
            CtlCommand::Init {
                force,
                clean,
                yes,
                generate_bootstrap_credential,
                bootstrap_output,
            } => commands::cmd_init(
                home,
                *force,
                *clean,
                *yes,
                *generate_bootstrap_credential,
                bootstrap_output.as_deref(),
            ),
        }
    }

    /// 명령 이름
    pub fn name(&self) -> &'static str {
        match self {
            CtlCommand::Start { .. } => "start",
            CtlCommand::Stop { .. } => "stop",
            CtlCommand::Restart { .. } => "restart",
            CtlCommand::Status => "status",
            CtlCommand::Logs { .. } => "logs",
            CtlCommand::Console { .. } => "console",
            CtlCommand::Config(_) => "config",
            CtlCommand::Doctor { .. } => "doctor",
            CtlCommand::Service(_) => "service",
            CtlCommand::Init { .. } => "init",
        }
    }
}

impl ConfigCommand {
    fn execute(&self, home: &std::path::Path) -> ExitCode {
        match self {
            ConfigCommand::Check { config } => commands::cmd_config_check(home, config.clone()),
            ConfigCommand::Show => commands::cmd_config_show(home),
            ConfigCommand::Init { force } => commands::cmd_config_init(home, *force),
        }
    }
}

impl ServiceCommand {
    fn execute(&self, home: &std::path::Path) -> ExitCode {
        let home = home.to_path_buf();
        match self {
            ServiceCommand::Install {
                name,
                user,
                server_path,
            } => commands::cmd_service_install(&home, name, user.clone(), server_path.as_deref()),
            ServiceCommand::Uninstall { name } => commands::cmd_service_uninstall(name),
            ServiceCommand::Status { name } => commands::cmd_service_status(name),
        }
    }
}
