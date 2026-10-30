//! Zeta4j CLI Common Library
//!
//! 모든 에디션에서 공유하는 CLI 유틸리티
//!
//! # 모듈
//!
//! - `traits`: 공통 커맨드 인터페이스 및 컨텍스트
//! - `dispatcher`: 공통 디스패처 유틸리티
//! - `shell`: Cypher Shell
//! - `ctl`: Server Controller
//! - `admin`: Admin Tool
//! - `completion`: Shell 자동완성 생성
//! - `error`: 사용자 친화적 에러 처리
//! - `verbose`: Verbose 및 디버그 모드

pub mod admin;
pub mod app;
pub mod auth;
pub mod completion;
pub mod ctl;
pub mod dispatcher;
pub mod error;
pub mod shell;
pub mod traits;
pub mod verbose;

pub use traits::{
    AsyncCommand, Command, CommandResult, CommandRunner, Context, ContextBuilder, Output, cli_error,
};

// ZetaError 재export (CLI에서 직접 사용)
pub use zeta4j_common::error::ZetaError;
