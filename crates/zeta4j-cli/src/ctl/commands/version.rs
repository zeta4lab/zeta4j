// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! version 명령 구현

use std::process::ExitCode;

/// 버전 정보 출력
pub fn cmd_version(json: bool) -> ExitCode {
    let version = env!("CARGO_PKG_VERSION");
    let git_commit = option_env!("GIT_COMMIT").unwrap_or("unknown");
    let build_date = option_env!("BUILD_DATE").unwrap_or("unknown");
    let target = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);

    if json {
        println!("{{");
        println!("  \"version\": \"{}\",", version);
        println!("  \"git_commit\": \"{}\",", git_commit);
        println!("  \"build_date\": \"{}\",", build_date);
        println!("  \"target\": \"{}\"", target);
        println!("}}");
    } else {
        println!("Zeta4j Controller v{}", version);
        println!("  Git:    {}", git_commit);
        println!("  Built:  {}", build_date);
        println!("  Target: {}", target);
    }

    ExitCode::SUCCESS
}
