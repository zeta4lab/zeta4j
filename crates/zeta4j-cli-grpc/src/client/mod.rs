// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! gRPC Client Module
//!
//! CLI에서 사용하는 gRPC 클라이언트 유틸리티

mod admin_client;
mod config;

pub use admin_client::AdminClient;
pub(crate) use config::BearerInterceptor;
pub use config::{GrpcClientConfig, GrpcClientError};

pub(crate) type AuthenticatedChannel =
    tonic::service::interceptor::InterceptedService<tonic::transport::Channel, BearerInterceptor>;
