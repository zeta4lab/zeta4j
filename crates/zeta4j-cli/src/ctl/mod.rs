// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Zeta4j Controller 모듈
//!
//! 서버 시작/종료/상태 관리 도구의 핵심 로직을 제공합니다.
//!
//! 단일 `z4j server` 명령 트리에서 사용됩니다.

pub mod cli;
pub mod commands;
pub mod dispatcher;
pub mod utils;

pub use cli::*;
pub use commands::*;
pub use utils::*;

use std::fs;
use std::process::ExitCode;

use crate::traits::{Context, Output};

pub fn execute(args: cli::Args) -> ExitCode {
    let home = utils::get_home_dir(args.home);

    let ctx = Context::builder().home_dir(home.clone()).build();
    let output = Output::new(&ctx);

    if let Err(e) = fs::create_dir_all(&home) {
        output.error(&format!("Failed to create home directory: {}", e));
        return ExitCode::FAILURE;
    }

    args.command.execute(&ctx)
}
