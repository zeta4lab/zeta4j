// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server Error Types

use std::io;
use thiserror::Error;

/// 서버 에러
#[derive(Error, Debug)]
pub enum ServerError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Invalid server state: {0}")]
    InvalidState(&'static str),

    #[error("Bind failed: {0}")]
    BindFailed(String),

    #[error("Shutdown timeout")]
    ShutdownTimeout,

    #[error("Server already running")]
    AlreadyRunning,

    #[error("Server not running")]
    NotRunning,
}

/// 설정 에러
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Invalid value: {0}")]
    InvalidValue(&'static str),

    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    #[error("Value out of range: {0}")]
    OutOfRange(&'static str),
}

/// 연결 에러
#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Handshake failed: {0}")]
    Handshake(String),

    #[error("Timeout: {0}")]
    Timeout(&'static str),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Connection closed by peer")]
    ClosedByPeer,

    #[error("Connection reset")]
    Reset,

    #[error("Invalid state: {0}")]
    InvalidState(&'static str),
}

/// 연결 추적기 에러
#[derive(Error, Debug)]
pub enum TrackerError {
    #[error("Maximum connections reached")]
    MaxConnectionsReached,

    #[error("Connection not found: {0}")]
    NotFound(u64),

    #[error("Per-IP limit reached")]
    PerIpLimitReached,
}
