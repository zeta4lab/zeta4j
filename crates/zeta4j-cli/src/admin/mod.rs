// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Zeta4j Admin Tool Common Module
//!
//! 모든 에디션에서 공유하는 관리 도구 로직 (gRPC 기반)
//!
//! # 사용법
//!
//! ```rust,ignore
//! fn main() -> std::process::ExitCode {
//!     zeta4j_cli::admin::run()
//! }
//! ```
//!
//! ## gRPC 서버 주소 설정
//!
//! CLI 인수 또는 환경변수로 gRPC 서버 주소를 설정할 수 있습니다:
//!
//! ```bash
//! # CLI 인수
//! z4j admin --address 127.0.0.1:9046 status
//!
//! # 환경변수
//! ZETA4J_GRPC_ADDRESS=127.0.0.1:9046 z4j admin status
//! ```

pub mod cli;
pub mod dispatcher;

use std::process::ExitCode;

pub use cli::{Args, Command, IoFormat};

/// Admin 도구 실행
pub fn execute(args: Args) -> ExitCode {
    args.command.execute(&args)
}
