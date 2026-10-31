// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 라우팅 모듈
//!
//! 클러스터 환경에서 자동 라우팅을 지원합니다.
//!
//! # 개요
//!
//! 라우팅 드라이버는 `zeta4j://` 스킴을 사용하여 클러스터에 연결합니다.
//! 읽기 트랜잭션은 팔로워로, 쓰기 트랜잭션은 리더로 자동 라우팅됩니다.
//!
//! # 예시
//!
//! ```ignore
//! use zeta4j_server::driver::routing::{RoutingDriver, RoutingPolicy};
//! use zeta4j_server::driver::{AuthToken, SessionConfig, AccessMode};
//!
//! // 라우팅 드라이버 생성
//! let driver = RoutingDriver::new(
//!     "zeta4j://server1:7687,server2:7687",
//!     AuthToken::basic("zeta4j", "password")
//! )?;
//!
//! // 읽기 세션 (팔로워로 라우팅)
//! let read_session = driver.session(
//!     SessionConfig::builder()
//!         .with_default_access_mode(AccessMode::Read)
//!         .build()
//! ).await?;
//!
//! // 쓰기 세션 (리더로 라우팅)
//! let write_session = driver.session(
//!     SessionConfig::builder()
//!         .with_default_access_mode(AccessMode::Write)
//!         .build()
//! ).await?;
//!
//! driver.close().await?;
//! ```

mod driver;
mod policy;
mod table;

pub use driver::{RoutingDriver, RoutingDriverMetrics, is_routing_uri, parse_routing_uri};
pub use policy::{RoutingPolicy, ServerSelector};
pub use table::{RoutingTable, ServerRole};
