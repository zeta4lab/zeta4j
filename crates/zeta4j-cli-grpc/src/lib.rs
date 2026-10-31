// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Zeta4j CLI gRPC Protocol
//!
//! CLI용 gRPC 서비스 프로토콜 정의
//!
//! ## 서비스 목록
//!
//! - `AdminService`: 데이터베이스 관리

// Macros (must be declared before other modules that use them)
#[macro_use]
mod macros;
pub mod credential;

/// Generated gRPC code - Admin
pub mod admin {
    tonic::include_proto!("zeta4j.admin");
}

// Admin service re-exports
pub use admin::admin_service_client::AdminServiceClient;
pub use admin::admin_service_server::{AdminService, AdminServiceServer};

// =============================================================================
// Client Module
// =============================================================================

pub mod client;

pub use client::{AdminClient, GrpcClientConfig, GrpcClientError};
