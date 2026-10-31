// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! System 메서드들.

use sysinfo::System;
use tonic::{Request, Response, Status};

use zeta4j_cli_grpc::admin::{
    ConfigResponse, Empty, ServiceStatus, SystemStatusResponse, VersionResponse,
};

use super::AdminGrpcService;

impl AdminGrpcService {
    /// 시스템 상태 조회
    pub async fn get_system_status_impl(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<SystemStatusResponse>, Status> {
        let summary = self
            .state
            .system_service()
            .summary()
            .map_err(|e| Status::internal(e.to_string()))?;

        let services = vec![
            ServiceStatus {
                name: "system_database".to_string(),
                status: zeta4j_common::STATUS_RUNNING.to_string(),
                message: Some(format!(
                    "{} databases, {} users",
                    summary.databases, summary.users
                )),
            },
            ServiceStatus {
                name: "tenant_database_manager".to_string(),
                status: zeta4j_common::STATUS_RUNNING.to_string(),
                message: Some(format!("{} runtime databases", summary.databases)),
            },
            ServiceStatus {
                name: "admin_grpc".to_string(),
                status: zeta4j_common::STATUS_RUNNING.to_string(),
                message: None,
            },
        ];

        let system = System::new_all();
        let memory_total_bytes = system.total_memory();
        let pid = sysinfo::get_current_pid()
            .map_err(|error| Status::internal(format!("failed to read process id: {error}")))?;
        let memory_used_bytes = system
            .process(pid)
            .ok_or_else(|| Status::internal("server process metrics are unavailable"))?
            .memory();
        let active_connections = self
            .state
            .bolt_stats()
            .map(|stats| stats.snapshot().connections_active)
            .unwrap_or(0);
        let active_transactions = if let Some(manager) = self.state.db_manager() {
            manager
                .list()
                .await
                .iter()
                .filter_map(|database| database.engine.as_ref())
                .map(|engine| engine.transaction_manager().active_transaction_count() as u64)
                .sum()
        } else {
            0
        };

        Ok(Response::new(SystemStatusResponse {
            edition: self.state.edition().name().to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            status: zeta4j_common::STATUS_RUNNING.to_string(),
            uptime_seconds: u32::try_from(self.state.uptime().as_secs()).unwrap_or(u32::MAX),
            memory_used_bytes,
            memory_total_bytes,
            active_connections: u32::try_from(active_connections).unwrap_or(u32::MAX),
            active_transactions: u32::try_from(active_transactions).unwrap_or(u32::MAX),
            services,
        }))
    }

    /// 설정 조회
    pub async fn get_config_impl(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<ConfigResponse>, Status> {
        let config_content = self
            .state
            .config_content()
            .ok_or_else(|| Status::failed_precondition("runtime configuration is unavailable"))?;

        Ok(Response::new(ConfigResponse {
            format: "toml".to_string(),
            content: config_content.to_string(),
        }))
    }

    /// 버전 정보 조회
    pub async fn get_version_impl(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<VersionResponse>, Status> {
        Ok(Response::new(VersionResponse {
            version: env!("CARGO_PKG_VERSION").to_string(),
            edition: self.state.edition().name().to_string(),
            commit_hash: option_env!("GIT_HASH").unwrap_or("unknown").to_string(),
            build_date: option_env!("BUILD_DATE").unwrap_or("unknown").to_string(),
            rust_version: option_env!("RUSTC_VERSION")
                .unwrap_or("unknown")
                .to_string(),
        }))
    }
}
