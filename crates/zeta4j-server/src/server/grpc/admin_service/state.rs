// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Admin 서비스 상태 정의.

use std::sync::Arc;
use tonic::Status;

use crate::server::bolt::BoltServerStats;
use crate::server::system::{
    LocalCredentialStore, LocalSecurityService, SecurityMutation, SharedSystemDatabaseService,
};
use crate::server::tenant::TenantDatabaseManager;
use zeta4j_graph::config::Edition;

/// Admin 서비스 상태
pub struct AdminAppState {
    /// 시스템 데이터베이스 서비스
    system_service: SharedSystemDatabaseService,
    /// 테넌트 데이터베이스 매니저 (dump/load에 사용)
    db_manager: Option<Arc<TenantDatabaseManager>>,
    started_at: std::time::Instant,
    config_content: Option<String>,
    bolt_stats: Option<Arc<BoltServerStats>>,
    /// 서버 에디션
    edition: Edition,
}

impl AdminAppState {
    /// 새 Admin 상태 생성
    pub fn new(system_service: SharedSystemDatabaseService) -> Self {
        Self {
            system_service,
            db_manager: None,
            started_at: std::time::Instant::now(),
            config_content: None,
            bolt_stats: None,
            edition: Edition::Base,
        }
    }

    /// DB 매니저와 함께 Admin 상태 생성
    pub fn with_db_manager(
        system_service: SharedSystemDatabaseService,
        db_manager: Arc<TenantDatabaseManager>,
    ) -> Self {
        Self {
            system_service,
            db_manager: Some(db_manager),
            started_at: std::time::Instant::now(),
            config_content: None,
            bolt_stats: None,
            edition: Edition::Base,
        }
    }

    /// 에디션 설정
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.edition = edition;
        self
    }

    /// 시스템 서비스 반환
    pub fn system_service(&self) -> &SharedSystemDatabaseService {
        &self.system_service
    }

    /// DB 매니저 반환
    pub fn db_manager(&self) -> Option<&Arc<TenantDatabaseManager>> {
        self.db_manager.as_ref()
    }

    pub fn uptime(&self) -> std::time::Duration {
        self.started_at.elapsed()
    }

    pub fn config_content(&self) -> Option<&str> {
        self.config_content.as_deref()
    }

    pub fn bolt_stats(&self) -> Option<&Arc<BoltServerStats>> {
        self.bolt_stats.as_ref()
    }

    /// 서버 에디션 반환
    pub fn edition(&self) -> Edition {
        self.edition
    }
}

/// gRPC Admin Service
pub struct AdminGrpcService {
    /// Admin 상태
    pub(super) state: Arc<AdminAppState>,
    credential_security: Option<Arc<LocalSecurityService>>,
    credential_tokens: Option<Arc<crate::server::auth::CredentialTokenService>>,
}

impl AdminGrpcService {
    /// 새 gRPC Admin 서비스 생성
    pub fn new(state: Arc<AdminAppState>) -> Self {
        Self {
            state,
            credential_security: None,
            credential_tokens: None,
        }
    }

    /// 시스템 서비스로부터 생성
    pub fn from_system_service(system_service: SharedSystemDatabaseService) -> Self {
        Self {
            state: Arc::new(AdminAppState::new(system_service)),
            credential_security: None,
            credential_tokens: None,
        }
    }

    /// 시스템 서비스와 DB 매니저로부터 생성
    pub fn with_db_manager(
        system_service: SharedSystemDatabaseService,
        db_manager: Arc<TenantDatabaseManager>,
    ) -> Self {
        Self {
            state: Arc::new(AdminAppState::with_db_manager(system_service, db_manager)),
            credential_security: None,
            credential_tokens: None,
        }
    }

    /// 실제 런타임 설정과 Bolt 계측을 연결한다.
    pub fn with_runtime(self, config_content: String, bolt_stats: Arc<BoltServerStats>) -> Self {
        let old = self.state;
        Self {
            state: Arc::new(AdminAppState {
                system_service: old.system_service.clone(),
                db_manager: old.db_manager.clone(),
                started_at: old.started_at,
                config_content: Some(config_content),
                bolt_stats: Some(bolt_stats),
                edition: old.edition,
            }),
            credential_security: self.credential_security,
            credential_tokens: self.credential_tokens,
        }
    }

    /// 에디션 설정 (빌더 패턴)
    pub fn with_edition(self, edition: Edition) -> Self {
        let old = self.state;
        Self {
            state: Arc::new(
                AdminAppState {
                    system_service: old.system_service.clone(),
                    db_manager: old.db_manager.clone(),
                    started_at: old.started_at,
                    config_content: old.config_content.clone(),
                    bolt_stats: old.bolt_stats.clone(),
                    edition: old.edition,
                }
                .with_edition(edition),
            ),
            credential_security: self.credential_security,
            credential_tokens: self.credential_tokens,
        }
    }

    pub fn with_credentials(
        mut self,
        credential_security: Arc<LocalSecurityService>,
        credential_tokens: Arc<crate::server::auth::CredentialTokenService>,
    ) -> Self {
        self.credential_security = Some(credential_security);
        self.credential_tokens = Some(credential_tokens);
        self
    }

    pub(super) fn credential_security(&self) -> Result<&Arc<LocalSecurityService>, Status> {
        self.credential_security
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("credential lifecycle is not enabled"))
    }

    pub(super) fn credential_tokens(
        &self,
    ) -> Result<&Arc<crate::server::auth::CredentialTokenService>, Status> {
        self.credential_tokens
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("credential lifecycle is not enabled"))
    }

    pub(super) fn apply_security_mutation(&self, mutation: SecurityMutation) -> Result<(), Status> {
        self.state
            .apply_security_mutation(mutation)
            .map_err(Status::internal)
    }
}

impl AdminAppState {
    pub fn apply_security_mutation(&self, mutation: SecurityMutation) -> Result<(), String> {
        let result = match mutation {
            SecurityMutation::CreateUser { user } => {
                self.system_service.security().apply_user_creation(user)
            }
            SecurityMutation::DeleteUser {
                username,
                occurred_at,
            } => self
                .system_service
                .security()
                .apply_user_deletion(&username, occurred_at),
            SecurityMutation::SetUserPassword {
                username,
                password_hash,
                require_change,
                updated_at,
            } => self.system_service.security().apply_password_update(
                &username,
                password_hash,
                require_change,
                updated_at,
            ),
            credential_mutation => {
                return LocalCredentialStore::new(self.system_service.storage().clone())
                    .apply_security_mutation(credential_mutation)
                    .map(|_| ())
                    .map_err(|error| error.to_string());
            }
        };
        result.map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use zeta4j_cli_grpc::admin::CreateDatabaseRequest;

    use super::*;
    use crate::server::system::SystemDatabaseService;
    use crate::server::tenant::TenantDatabaseManagerConfig;

    #[tokio::test]
    async fn concurrent_database_creations_register_exactly_once()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let system = Arc::new(SystemDatabaseService::open(
            directory.path().join("system"),
        )?);
        let manager = Arc::new(
            TenantDatabaseManager::new_with_storage(
                TenantDatabaseManagerConfig::default()
                    .with_data_path(directory.path().join("databases")),
            )
            .await?,
        );
        let state = Arc::new(AdminAppState::with_db_manager(system, manager));
        let service = AdminGrpcService::new(state);
        let request = || {
            tonic::Request::new(CreateDatabaseRequest {
                name: "serialized_creation".to_string(),
                description: None,
            })
        };

        let (first, second) = tokio::join!(
            service.create_database_impl(request()),
            service.create_database_impl(request())
        );
        let responses = [first?.into_inner(), second?.into_inner()];

        assert_eq!(
            responses.iter().filter(|response| response.success).count(),
            1
        );
        assert_eq!(
            responses
                .iter()
                .filter(|response| response.message.contains("already exists"))
                .count(),
            1
        );
        Ok(())
    }
}
