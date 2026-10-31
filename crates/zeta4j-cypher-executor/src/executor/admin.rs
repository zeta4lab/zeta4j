// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Canonical Cypher admin command executor.

use super::QueryExecutor;
use std::sync::Arc;
use zeta4j_common::{ZetaError, codes};
use zeta4j_cypher_core::ast::AdminCommand;
use zeta4j_cypher_core::services::{
    AdminCapability, AdminService, AdminServiceError, DatabaseInfo,
};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    fn admin_error(code: zeta4j_common::ErrorCode, message: impl Into<String>) -> GraphError {
        GraphError::Zeta(ZetaError::query_execution_with_code(code, message))
    }

    fn map_admin_error(error: AdminServiceError) -> GraphError {
        match error {
            AdminServiceError::Storage(message) => GraphError::Storage { message },
            AdminServiceError::Unsupported(message) => {
                Self::admin_error(codes::cypher::ADMIN_UNSUPPORTED, message)
            }
            AdminServiceError::Invariant(message) => {
                Self::admin_error(codes::cypher::ADMIN_STATE_INVARIANT, message)
            }
            AdminServiceError::NotFound { entity, name } => GraphError::Validation {
                message: format!("{} '{}' not found", entity, name),
            },
            AdminServiceError::AlreadyExists { entity, name } => GraphError::Validation {
                message: format!("{} '{}' already exists", entity, name),
            },
            AdminServiceError::BootstrapExpired => GraphError::Validation {
                message: "bootstrap credential expired".to_string(),
            },
            AdminServiceError::BootstrapConsumed => GraphError::Validation {
                message: "bootstrap credential already consumed".to_string(),
            },
            AdminServiceError::InvalidCredential => GraphError::Validation {
                message: "invalid bootstrap credential".to_string(),
            },
            AdminServiceError::InvalidPassword => GraphError::Validation {
                message: "new password does not meet policy".to_string(),
            },
        }
    }

    fn require_admin_service(&self, capability: AdminCapability) -> Result<Arc<dyn AdminService>> {
        let service = self.admin_service.as_ref().cloned().ok_or_else(|| {
            Self::admin_error(
                codes::cypher::ADMIN_SERVICE_UNAVAILABLE,
                "canonical admin service is not configured",
            )
        })?;
        if !service.supports(capability) {
            return Err(Self::admin_error(
                codes::cypher::ADMIN_UNSUPPORTED,
                format!("admin capability {:?} is not supported", capability),
            ));
        }
        Ok(service)
    }

    fn require_current_user(&self) -> Result<&str> {
        self.current_user.as_deref().ok_or_else(|| {
            Self::admin_error(
                codes::cypher::ADMIN_PRINCIPAL_REQUIRED,
                "an authenticated principal is required",
            )
        })
    }

    fn database_row(database: DatabaseInfo, home_database: Option<&str>) -> Vec<Value> {
        vec![
            Value::String(database.name.clone()),
            Value::String(database.database_type),
            database.address.map_or(Value::Null, Value::String),
            database.role.map_or(Value::Null, Value::String),
            Value::String(database.requested_status),
            Value::String(database.current_status),
            Value::Boolean(database.is_default),
            Value::Boolean(home_database == Some(database.name.as_str())),
        ]
    }

    pub(super) async fn execute_admin_command(
        &self,
        cmd: &AdminCommand,
    ) -> Result<Vec<Vec<Value>>> {
        match cmd {
            AdminCommand::ShowProcedures => self.show_procedures(),
            AdminCommand::CreateUser {
                name,
                password,
                change_required,
                if_not_exists,
            } => {
                let service = self.require_admin_service(AdminCapability::UserManagement)?;
                let _ = self.require_current_user()?;
                if service
                    .get_user(name)
                    .await
                    .map_err(Self::map_admin_error)?
                    .is_some()
                {
                    if *if_not_exists {
                        return Ok(vec![]);
                    }
                    return Err(GraphError::Validation {
                        message: format!("User '{}' already exists", name),
                    });
                }
                service
                    .create_user(name, password, *change_required)
                    .await
                    .map_err(Self::map_admin_error)?;
                Ok(vec![])
            }
            AdminCommand::DropUser { name, if_exists } => {
                let service = self.require_admin_service(AdminCapability::UserManagement)?;
                let _ = self.require_current_user()?;
                if service
                    .get_user(name)
                    .await
                    .map_err(Self::map_admin_error)?
                    .is_none()
                {
                    if *if_exists {
                        return Ok(vec![]);
                    }
                    return Err(GraphError::Validation {
                        message: format!("User '{}' does not exist", name),
                    });
                }
                service
                    .delete_user(name)
                    .await
                    .map_err(Self::map_admin_error)?;
                Ok(vec![])
            }
            AdminCommand::AlterUser {
                name,
                password,
                change_required,
                suspended,
            } => {
                let service = self.require_admin_service(AdminCapability::UserManagement)?;
                let _ = self.require_current_user()?;
                if service
                    .get_user(name)
                    .await
                    .map_err(Self::map_admin_error)?
                    .is_none()
                {
                    return Err(GraphError::Validation {
                        message: format!("User '{}' does not exist", name),
                    });
                }
                if let Some(new_password) = password {
                    service
                        .change_password(name, new_password)
                        .await
                        .map_err(Self::map_admin_error)?;
                }
                if let Some(required) = change_required {
                    service
                        .set_password_change_required(name, *required)
                        .await
                        .map_err(Self::map_admin_error)?;
                }
                if let Some(is_suspended) = suspended {
                    service
                        .set_suspended(name, *is_suspended)
                        .await
                        .map_err(Self::map_admin_error)?;
                }
                Ok(vec![])
            }
            AdminCommand::ShowUsers => {
                let service = self.require_admin_service(AdminCapability::UserManagement)?;
                let mut users = service.list_users().await.map_err(Self::map_admin_error)?;
                users.sort_by(|left, right| left.username.cmp(&right.username));
                Ok(users
                    .into_iter()
                    .map(|user| {
                        vec![
                            Value::String(user.username),
                            Value::Boolean(user.password_change_required),
                            Value::Boolean(user.suspended),
                        ]
                    })
                    .collect())
            }
            AdminCommand::ShowCurrentUser => {
                let service = self.require_admin_service(AdminCapability::UserManagement)?;
                let username = self.require_current_user()?;
                let user = service
                    .get_user(username)
                    .await
                    .map_err(Self::map_admin_error)?
                    .ok_or_else(|| {
                        Self::admin_error(
                            codes::cypher::ADMIN_PRINCIPAL_REQUIRED,
                            format!(
                                "authenticated principal '{}' has no canonical user record",
                                username
                            ),
                        )
                    })?;
                Ok(vec![vec![Value::String(user.username)]])
            }
            AdminCommand::CreateDatabase {
                name,
                if_not_exists,
            } => {
                let service = self.require_admin_service(AdminCapability::DatabaseManagement)?;
                let _ = self.require_current_user()?;
                if service
                    .get_database(name)
                    .await
                    .map_err(Self::map_admin_error)?
                    .is_some()
                {
                    if *if_not_exists {
                        return Ok(vec![]);
                    }
                    return Err(GraphError::Validation {
                        message: format!("Database '{}' already exists", name),
                    });
                }
                service
                    .create_database(name)
                    .await
                    .map_err(Self::map_admin_error)?;
                Ok(vec![])
            }
            AdminCommand::DropDatabase { name, if_exists } => {
                let service = self.require_admin_service(AdminCapability::DatabaseManagement)?;
                let _ = self.require_current_user()?;
                if service
                    .get_database(name)
                    .await
                    .map_err(Self::map_admin_error)?
                    .is_none()
                {
                    if *if_exists {
                        return Ok(vec![]);
                    }
                    return Err(GraphError::Validation {
                        message: format!("Database '{}' does not exist", name),
                    });
                }
                service
                    .drop_database(name)
                    .await
                    .map_err(Self::map_admin_error)?;
                Ok(vec![])
            }
            AdminCommand::ShowDatabases => {
                let service = self.require_admin_service(AdminCapability::DatabaseManagement)?;
                let home_database = if let Some(username) = self.current_user.as_deref() {
                    service
                        .get_user(username)
                        .await
                        .map_err(Self::map_admin_error)?
                        .and_then(|user| user.home_database)
                } else {
                    None
                };
                let mut databases = service
                    .list_databases()
                    .await
                    .map_err(Self::map_admin_error)?;
                databases.sort_by(|left, right| left.name.cmp(&right.name));
                Ok(databases
                    .into_iter()
                    .map(|database| Self::database_row(database, home_database.as_deref()))
                    .collect())
            }
            AdminCommand::ShowDefaultDatabase => {
                let service = self.require_admin_service(AdminCapability::DatabaseManagement)?;
                let defaults: Vec<_> = service
                    .list_databases()
                    .await
                    .map_err(Self::map_admin_error)?
                    .into_iter()
                    .filter(|database| database.is_default)
                    .collect();
                let [database] = defaults.as_slice() else {
                    return Err(Self::admin_error(
                        codes::cypher::ADMIN_STATE_INVARIANT,
                        format!(
                            "expected exactly one default database, found {}",
                            defaults.len()
                        ),
                    ));
                };
                Ok(vec![vec![
                    Value::String(database.name.clone()),
                    Value::String(database.current_status.clone()),
                ]])
            }
            AdminCommand::StartDatabase { name } => {
                let service = self.require_admin_service(AdminCapability::DatabaseManagement)?;
                let _ = self.require_current_user()?;
                service
                    .start_database(name)
                    .await
                    .map_err(Self::map_admin_error)?;
                Ok(vec![vec![
                    Value::String(name.clone()),
                    Value::String("started".to_string()),
                ]])
            }
            AdminCommand::StopDatabase { name } => {
                let service = self.require_admin_service(AdminCapability::DatabaseManagement)?;
                let _ = self.require_current_user()?;
                service
                    .stop_database(name)
                    .await
                    .map_err(Self::map_admin_error)?;
                Ok(vec![vec![
                    Value::String(name.clone()),
                    Value::String("stopped".to_string()),
                ]])
            }
        }
    }

    fn show_procedures(&self) -> Result<Vec<Vec<Value>>> {
        let registry = &self.procedure_registry;
        let mut rows = Vec::new();
        for proc_name in registry.list_names() {
            if let Some(proc) = registry.get(proc_name) {
                let sig = proc.signature();
                let args = sig
                    .arguments
                    .iter()
                    .map(|arg| {
                        let optional = if arg.optional { " = null" } else { "" };
                        format!("{}: {}{}", arg.name, arg.value_type.as_str(), optional)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let yields = sig
                    .yields
                    .iter()
                    .map(|item| format!("{}: {}", item.name, item.value_type.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ");
                rows.push(vec![
                    Value::String(proc_name.to_string()),
                    Value::String(format!("({}) :: ({})", args, yields)),
                    Value::String(sig.description.clone()),
                ]);
            }
        }
        rows.sort_by(|left, right| match (&left[0], &right[0]) {
            (Value::String(left_name), Value::String(right_name)) => left_name.cmp(right_name),
            _ => std::cmp::Ordering::Equal,
        });
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use zeta4j_cypher_core::services::{AdminServiceResult, UserInfo};
    use zeta4j_graph::GraphEngine;

    #[derive(Default)]
    struct MockAdminService {
        users: Vec<UserInfo>,
        databases: Vec<DatabaseInfo>,
    }

    #[async_trait]
    impl AdminService for MockAdminService {
        fn supports(&self, capability: AdminCapability) -> bool {
            let _ = capability;
            true
        }

        async fn list_users(&self) -> AdminServiceResult<Vec<UserInfo>> {
            Ok(self.users.clone())
        }

        async fn get_user(&self, username: &str) -> AdminServiceResult<Option<UserInfo>> {
            Ok(self
                .users
                .iter()
                .find(|user| user.username == username)
                .cloned())
        }

        async fn create_user(
            &self,
            _username: &str,
            _password: &str,
            _password_change_required: bool,
        ) -> AdminServiceResult<()> {
            Ok(())
        }

        async fn delete_user(&self, _username: &str) -> AdminServiceResult<()> {
            Ok(())
        }

        async fn change_password(
            &self,
            _username: &str,
            _new_password: &str,
        ) -> AdminServiceResult<()> {
            Ok(())
        }

        async fn set_suspended(&self, _username: &str, _suspended: bool) -> AdminServiceResult<()> {
            Ok(())
        }

        async fn set_password_change_required(
            &self,
            _username: &str,
            _required: bool,
        ) -> AdminServiceResult<()> {
            Ok(())
        }

        async fn list_databases(&self) -> AdminServiceResult<Vec<DatabaseInfo>> {
            Ok(self.databases.clone())
        }

        async fn get_database(&self, name: &str) -> AdminServiceResult<Option<DatabaseInfo>> {
            Ok(self
                .databases
                .iter()
                .find(|database| database.name == name)
                .cloned())
        }

        async fn create_database(&self, _name: &str) -> AdminServiceResult<()> {
            Ok(())
        }

        async fn drop_database(&self, _name: &str) -> AdminServiceResult<()> {
            Ok(())
        }

        async fn start_database(&self, _name: &str) -> AdminServiceResult<()> {
            Ok(())
        }

        async fn stop_database(&self, _name: &str) -> AdminServiceResult<()> {
            Ok(())
        }
    }

    async fn executor(
        service: Option<Arc<dyn AdminService>>,
        current_user: Option<&str>,
    ) -> std::result::Result<QueryExecutor, Box<dyn std::error::Error>> {
        let engine = Arc::new(GraphEngine::with_temp_dir().await?);
        let mut executor = QueryExecutor::new(engine);
        if let Some(service) = service {
            executor = executor.with_admin_service(service);
        }
        if let Some(username) = current_user {
            executor = executor.with_current_user(username.to_string());
        }
        Ok(executor)
    }

    fn canonical_service() -> Arc<dyn AdminService> {
        Arc::new(MockAdminService {
            users: vec![UserInfo {
                username: "alice".to_string(),
                password_change_required: false,
                suspended: false,
                home_database: Some("alpha".to_string()),
            }],
            databases: vec![DatabaseInfo {
                name: "alpha".to_string(),
                database_type: "standard".to_string(),
                address: Some("db.example:9045".to_string()),
                role: Some("standalone".to_string()),
                requested_status: "online".to_string(),
                current_status: "online".to_string(),
                is_default: true,
                store_size: Some(42),
            }],
        })
    }

    fn assert_admin_code(
        error: GraphError,
        expected: &str,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let GraphError::Zeta(zeta_error) = error else {
            return Err(std::io::Error::other("expected structured admin error").into());
        };
        assert_eq!(zeta_error.code().to_string(), expected);
        Ok(())
    }

    #[tokio::test]
    async fn admin_commands_fail_when_service_is_missing()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let executor = executor(None, None).await?;
        let error = executor
            .execute_admin_command(&AdminCommand::ShowUsers)
            .await
            .err()
            .ok_or_else(|| std::io::Error::other("SHOW USERS unexpectedly succeeded"))?;
        assert_admin_code(error, "CY-ADMIN-001")?;
        Ok(())
    }

    #[tokio::test]
    async fn current_user_requires_a_canonical_principal()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let executor = executor(Some(canonical_service()), None).await?;
        let error = executor
            .execute_admin_command(&AdminCommand::ShowCurrentUser)
            .await
            .err()
            .ok_or_else(|| std::io::Error::other("SHOW CURRENT USER unexpectedly succeeded"))?;
        assert_admin_code(error, "CY-ADMIN-003")?;
        Ok(())
    }

    #[tokio::test]
    async fn show_commands_project_only_canonical_state()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let executor = executor(Some(canonical_service()), Some("alice")).await?;
        let users = executor
            .execute_admin_command(&AdminCommand::ShowUsers)
            .await?;
        assert_eq!(users.len(), 1);
        assert_eq!(users[0][0], Value::String("alice".to_string()));

        let databases = executor
            .execute_admin_command(&AdminCommand::ShowDatabases)
            .await?;
        assert_eq!(databases.len(), 1);
        assert_eq!(
            databases[0],
            vec![
                Value::String("alpha".to_string()),
                Value::String("standard".to_string()),
                Value::String("db.example:9045".to_string()),
                Value::String("standalone".to_string()),
                Value::String("online".to_string()),
                Value::String("online".to_string()),
                Value::Boolean(true),
                Value::Boolean(true),
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn default_database_requires_exactly_one_canonical_entry()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let service: Arc<dyn AdminService> = Arc::new(MockAdminService::default());
        let executor = executor(Some(service), None).await?;
        let error = executor
            .execute_admin_command(&AdminCommand::ShowDefaultDatabase)
            .await
            .err()
            .ok_or_else(|| std::io::Error::other("SHOW DEFAULT DATABASE unexpectedly succeeded"))?;
        assert_admin_code(error, "CY-ADMIN-004")?;
        Ok(())
    }
}
