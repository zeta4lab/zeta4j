// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 공통 디스패처 유틸리티
//!
//! CLI 명령 디스패처를 위한 공통 트레이트와 헬퍼
//!
//! # 개요
//!
//! 동기 디스패처 유형을 지원합니다:
//! - `SyncDispatcher`: 동기식 명령 실행
//!
//! # 예제
//!
//! ```ignore
//! use zeta4j_cli::dispatcher::{Dispatcher, DispatchResult};
//!
//! enum MyCommand {
//!     Start { port: u16 },
//!     Stop,
//! }
//!
//! impl Dispatcher for MyCommand {
//!     fn dispatch(&self) -> DispatchResult {
//!         match self {
//!             MyCommand::Start { port } => start_server(*port),
//!             MyCommand::Stop => stop_server(),
//!         }
//!     }
//! }
//! ```

use std::process::ExitCode;

use crate::error::CliResult;

// ============================================================================
// Dispatch Result Types
// ============================================================================

/// 디스패치 결과 (ExitCode 기반)
pub type DispatchResult = ExitCode;

/// 디스패치 결과 (Result 기반)
pub type DispatchResultExt<T = ()> = CliResult<T>;

// ============================================================================
// Sync Dispatcher
// ============================================================================

/// 동기식 디스패처 트레이트
///
/// 동기식으로 명령을 실행하는 디스패처
pub trait SyncDispatcher {
    /// 명령 이름 (로깅/디버깅용)
    fn name(&self) -> &'static str;

    /// 명령 디스패치 및 실행
    fn dispatch(&self) -> DispatchResult;
}

/// 동기 명령 핸들러
///
/// 개별 명령을 처리하는 핸들러
pub trait SyncHandler {
    /// 핸들러 이름
    fn name(&self) -> &'static str;

    /// 명령 실행
    fn handle(&self) -> DispatchResult;
}

// ============================================================================
// Dispatch Helpers
// ============================================================================

/// 에러를 ExitCode로 변환
pub fn error_to_exit_code<E: std::fmt::Display>(error: E) -> ExitCode {
    eprintln!("Error: {}", error);
    ExitCode::FAILURE
}

/// Result를 ExitCode로 변환
pub fn result_to_exit_code<T, E: std::fmt::Display>(result: Result<T, E>) -> ExitCode {
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => error_to_exit_code(e),
    }
}

/// CliResult를 ExitCode로 변환 (ZetaError 활용)
pub fn cli_result_to_exit_code<T>(result: CliResult<T>) -> ExitCode {
    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            e.exit_code()
        }
    }
}

// ============================================================================
// Command Registry
// ============================================================================

/// 명령 레지스트리
///
/// 동적으로 명령을 등록하고 디스패치
pub struct CommandRegistry<H> {
    handlers: Vec<(String, H)>,
}

impl<H> Default for CommandRegistry<H> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H> CommandRegistry<H> {
    /// 새 레지스트리 생성
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// 핸들러 등록
    pub fn register(mut self, name: impl Into<String>, handler: H) -> Self {
        self.handlers.push((name.into(), handler));
        self
    }

    /// 핸들러 조회
    pub fn get(&self, name: &str) -> Option<&H> {
        self.handlers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, h)| h)
    }

    /// 등록된 명령 이름 목록
    pub fn names(&self) -> Vec<&str> {
        self.handlers.iter().map(|(n, _)| n.as_str()).collect()
    }
}

impl<H: SyncHandler> CommandRegistry<H> {
    /// 동기 명령 디스패치
    pub fn dispatch_sync(&self, name: &str) -> DispatchResult {
        match self.get(name) {
            Some(handler) => handler.handle(),
            None => {
                eprintln!("Error: Unknown command '{}'", name);
                ExitCode::FAILURE
            }
        }
    }
}
