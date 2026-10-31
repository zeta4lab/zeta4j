// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! gRPC Services for Zeta4j Server
//!
//! ## Services
//!
//! - AdminService: 데이터베이스/사용자 관리 gRPC 서비스
//!
//! ## Authentication
//!
//! - GrpcAuthInterceptor: JWT 인증 인터셉터

mod admin_service;
mod interceptor;

pub use admin_service::{AdminAppState, AdminGrpcService};
pub use interceptor::{
    GrpcAuthInterceptor, GrpcAuthLayer, GrpcAuthService, SecurityGenerationGuard, access_status,
};
