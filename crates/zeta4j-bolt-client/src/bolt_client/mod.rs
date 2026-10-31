// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Edition-neutral Bolt protocol client.
//!
//! Neo4j Bolt 프로토콜을 사용한 원격 데이터베이스 연결 클라이언트입니다.
//! Zeta4j 서버의 PackStream 및 메시지 타입을 재사용합니다.
//!
//! ## TLS 지원
//!
//! `bolt+s://` 스킴을 사용하여 TLS 암호화 연결을 지원합니다.
//!
//! ```ignore
//! let mut client = BoltClient::new();
//! client.connect("bolt+s://secure.server:7687")?;
//! ```
//!
//! ## 검증된 transport generation
//!
//! 인증서, CA/CRL과 명시적 server name은 `zeta4j-transport-security`에서
//! 한 세대로 검증한 뒤 주입합니다.
//!
//! ```ignore
//! let mut client = BoltClient::new()
//!     .with_transport_config(client_config, "secure.server");
//! client.connect("bolt+s://secure.server:7687")?;
//! ```

mod client;
mod config;
mod error;
mod result;
mod stream;

// Re-exports
pub use client::BoltClient;
pub use config::ReconnectConfig;
pub use error::{BoltClientError, BoltClientResult};

// Re-export PackStreamValue for tests
