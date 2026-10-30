//! Database Management 메서드들.

use tonic::{Request, Response, Status};

use zeta4j_cli_grpc::admin::{
    CreateDatabaseRequest, DatabaseInfo, DatabaseListResponse, DatabaseRequest,
    DatabaseStatusResponse, DropDatabaseRequest, DropIndexRequest, Empty, StatusResponse,
};

use super::AdminGrpcService;
use crate::server::system::{CatalogDatabaseState, DatabaseEntry};
use crate::server::tenant::TenantDatabaseManager;

impl AdminGrpcService {
    /// 데이터베이스 목록 조회
    pub async fn list_databases_impl(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<DatabaseListResponse>, Status> {
        let catalog = self.state.system_service().catalog();

        let db_list = catalog
            .list()
            .map_err(|e| Status::internal(e.to_string()))?;

        let mut databases = Vec::with_capacity(db_list.len());

        for db in db_list {
            let runtime_state = if let Some(db_manager) = self.state.db_manager() {
                match db_manager.get(&db.name).await {
                    Ok(database) => database.state().await.to_string(),
                    Err(_) => db.state.to_string(),
                }
            } else {
                db.state.to_string()
            };
            // 노드/엣지 수: db_manager에서 엔진을 가져와 조회
            let (node_count, edge_count) = if let Some(db_manager) = self.state.db_manager() {
                if let Some(engine) = db_manager.get_engine(&db.name).await {
                    let nodes = engine
                        .node_count()
                        .await
                        .map_err(|error| Status::internal(error.to_string()))?
                        as u64;
                    let edges = engine.edge_count().await as u64;
                    (nodes, edges)
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };

            // 풀텍스트 인덱스 정보
            let has_fulltext_index = if let Some(db_manager) = self.state.db_manager() {
                match db_manager.get(&db.name).await {
                    Ok(database) => database
                        .storage_context
                        .as_ref()
                        .map(|context| context.fulltext().has_any_active())
                        .transpose()
                        .map_err(|error| Status::internal(error.to_string()))?
                        .unwrap_or(false),
                    Err(_) => false,
                }
            } else {
                false
            };

            let size_bytes = self
                .state
                .db_manager()
                .and_then(|manager| manager.database_path(&db.name))
                .map(|path| directory_size(&path))
                .transpose()
                .map_err(Status::internal)?
                .unwrap_or(0);

            databases.push(DatabaseInfo {
                name: db.name,
                status: runtime_state,
                node_count,
                edge_count,
                size_bytes,
                created_at: db.created_at.to_rfc3339(),
                updated_at: db.updated_at.to_rfc3339(),
                has_fulltext_index,
            });
        }

        Ok(Response::new(DatabaseListResponse { databases }))
    }

    /// 데이터베이스 생성
    pub async fn create_database_impl(
        &self,
        request: Request<CreateDatabaseRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();
        if let Some(response) = Self::validate_database_creation(&req)? {
            return Ok(response);
        }
        let _creation_guard = self.state.system_service().lock_database_creation().await;
        let catalog = self.state.system_service().catalog();

        // Validation and registration share one cross-protocol lock.
        if catalog.exists(&req.name).map_err(|error| {
            Status::internal(format!("Failed to read database catalog: {error}"))
        })? {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Database '{}' already exists", req.name),
            }));
        }

        self.create_database_locally(req).await
    }

    fn validate_database_creation(
        req: &CreateDatabaseRequest,
    ) -> Result<Option<Response<StatusResponse>>, Status> {
        if !TenantDatabaseManager::is_valid_name(&req.name) {
            return Err(Status::invalid_argument(format!(
                "Invalid database name '{}': use 1-63 ASCII letters, digits, or underscores, starting with a letter",
                req.name
            )));
        }

        if req.name == "system" || req.name == "zeta4j" {
            return Ok(Some(Response::new(StatusResponse {
                success: false,
                message: format!("Cannot create reserved database '{}'", req.name),
            })));
        }
        Ok(None)
    }

    async fn create_database_locally(
        &self,
        req: CreateDatabaseRequest,
    ) -> Result<Response<StatusResponse>, Status> {
        let catalog = self.state.system_service().catalog();
        // 새 데이터베이스 엔트리 생성
        let entry = DatabaseEntry::new(&req.name, format!("databases/{}", req.name));

        match catalog.register(entry) {
            Ok(_) => {
                let Some(db_manager) = self.state.db_manager() else {
                    let _ = catalog.unregister(&req.name);
                    return Ok(Response::new(StatusResponse {
                        success: false,
                        message: "Database manager is unavailable".to_string(),
                    }));
                };
                let db_config = super::super::super::tenant::TenantDatabaseConfig::default();
                if let Err(error) = db_manager.create(&req.name, db_config).await {
                    if let Err(rollback_error) = catalog.unregister(&req.name) {
                        tracing::error!(
                            db_name = %req.name,
                            %rollback_error,
                            %error,
                            "Failed to roll back catalog registration"
                        );
                    }
                    return Ok(Response::new(StatusResponse {
                        success: false,
                        message: format!(
                            "Failed to create database storage for '{}': {error}",
                            req.name
                        ),
                    }));
                }
                if let Err(error) = catalog.set_state(&req.name, CatalogDatabaseState::Online) {
                    if let Err(rollback_error) = db_manager.delete(&req.name).await {
                        tracing::error!(
                            db_name = %req.name,
                            %rollback_error,
                            %error,
                            "Failed to roll back database manager creation"
                        );
                    }
                    let _ = catalog.unregister(&req.name);
                    return Ok(Response::new(StatusResponse {
                        success: false,
                        message: format!("Failed to persist database state: {error}"),
                    }));
                }
                Ok(Response::new(StatusResponse {
                    success: true,
                    message: format!("Database '{}' created", req.name),
                }))
            }
            Err(e) => Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Failed to create database: {}", e),
            })),
        }
    }

    /// 데이터베이스 삭제
    pub async fn drop_database_impl(
        &self,
        request: Request<DropDatabaseRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();

        if req.name == "system" {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: "Cannot drop system database".to_string(),
            }));
        }

        if req.name == "zeta4j" {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: "Cannot drop default database 'zeta4j'".to_string(),
            }));
        }

        let catalog = self.state.system_service().catalog();
        let entry = match catalog.get(&req.name) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                return Ok(Response::new(StatusResponse {
                    success: false,
                    message: format!("Database '{}' not found", req.name),
                }));
            }
            Err(error) => {
                return Ok(Response::new(StatusResponse {
                    success: false,
                    message: format!("Failed to read database catalog: {error}"),
                }));
            }
        };

        let Some(db_manager) = self.state.db_manager() else {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: "Database manager is unavailable".to_string(),
            }));
        };
        let is_online = match db_manager.get(&req.name).await {
            Ok(database) => database.state().await.is_online(),
            Err(_) => false,
        };
        if is_online {
            if !req.force {
                return Ok(Response::new(StatusResponse {
                    success: false,
                    message: format!(
                        "Database '{}' is online; stop it first or pass --force",
                        req.name
                    ),
                }));
            }
            if let Err(error) = db_manager.stop(&req.name).await {
                return Ok(Response::new(StatusResponse {
                    success: false,
                    message: format!("Failed to stop database before deletion: {error}"),
                }));
            }
        }

        let graph_dir = db_manager.database_path(&req.name);
        if let Err(error) = db_manager.delete(&req.name).await {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Failed to remove database from the runtime: {error}"),
            }));
        }
        if let Some(graph_dir) = graph_dir
            && graph_dir.exists()
            && let Err(error) = std::fs::remove_dir_all(&graph_dir)
        {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Failed to remove {}: {error}", graph_dir.display()),
            }));
        }
        if let Err(error) = catalog.unregister(&req.name) {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Failed to remove database catalog entry: {error}"),
            }));
        }

        tracing::info!(database = %req.name, uuid = %entry.uuid, "Database dropped");

        Ok(Response::new(StatusResponse {
            success: true,
            message: format!("Database '{}' dropped", req.name),
        }))
    }

    /// 데이터베이스 상태 조회
    pub async fn get_database_status_impl(
        &self,
        request: Request<DatabaseRequest>,
    ) -> Result<Response<DatabaseStatusResponse>, Status> {
        let req = request.into_inner();
        let catalog = self.state.system_service().catalog();

        let db_name = if req.name.is_empty() {
            "zeta4j"
        } else {
            &req.name
        };

        let db = catalog
            .get(db_name)
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found(format!("Database '{}' not found", db_name)))?;

        // 노드/엣지 수
        let (node_count, edge_count) = if let Some(db_manager) = self.state.db_manager() {
            if let Some(engine) = db_manager.get_engine(db_name).await {
                let nodes = engine
                    .node_count()
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?
                    as u64;
                let edges = engine.edge_count().await as u64;
                (nodes, edges)
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };

        // 풀텍스트 인덱스 정보
        let has_fulltext_index = if let Some(db_manager) = self.state.db_manager() {
            match db_manager.get(db_name).await {
                Ok(database) => database
                    .storage_context
                    .as_ref()
                    .map(|context| context.fulltext().has_any_active())
                    .transpose()
                    .map_err(|error| Status::internal(error.to_string()))?
                    .unwrap_or(false),
                Err(_) => false,
            }
        } else {
            false
        };

        let runtime_status = if let Some(db_manager) = self.state.db_manager() {
            match db_manager.get(db_name).await {
                Ok(database) => database.state().await.to_string(),
                Err(_) => db.state.to_string(),
            }
        } else {
            db.state.to_string()
        };

        let storage_path = self
            .state
            .db_manager()
            .and_then(|manager| manager.database_path(db_name));
        let size_bytes = storage_path
            .as_ref()
            .map(|path| directory_size(path))
            .transpose()
            .map_err(Status::internal)?
            .unwrap_or(0);
        let wal_size = storage_path
            .as_ref()
            .map(|path| matching_file_size(path, |name| name.to_ascii_lowercase().contains("wal")))
            .transpose()
            .map_err(Status::internal)?
            .unwrap_or(0);
        let (active_transactions, memory_usage_bytes) =
            if let Some(db_manager) = self.state.db_manager() {
                match db_manager.get_engine(db_name).await {
                    Some(engine) => (
                        u32::try_from(engine.transaction_manager().active_transaction_count())
                            .unwrap_or(u32::MAX),
                        u64::try_from(engine.stats().await.memory_usage).unwrap_or(u64::MAX),
                    ),
                    None => (0, 0),
                }
            } else {
                (0, 0)
            };

        let database = DatabaseInfo {
            name: db.name.clone(),
            status: runtime_status,
            node_count,
            edge_count,
            size_bytes,
            created_at: db.created_at.to_rfc3339(),
            updated_at: db.updated_at.to_rfc3339(),
            has_fulltext_index,
        };

        Ok(Response::new(DatabaseStatusResponse {
            database: Some(database),
            storage_path: storage_path
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
            wal_size,
            active_transactions,
            memory_usage_bytes,
        }))
    }

    /// 데이터베이스 시작
    pub async fn start_database_impl(
        &self,
        request: Request<DatabaseRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();

        if req.name == "system" {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: "Cannot start/stop system database".to_string(),
            }));
        }

        let Some(db_manager) = self.state.db_manager() else {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: "Database manager is unavailable".to_string(),
            }));
        };
        if let Err(error) = db_manager.start(&req.name).await {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Failed to start database: {error}"),
            }));
        }
        let catalog = self.state.system_service().catalog();
        if let Err(error) = catalog.set_state(&req.name, CatalogDatabaseState::Online) {
            let _ = db_manager.stop(&req.name).await;
            return Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Failed to persist started database state: {error}"),
            }));
        }
        Ok(Response::new(StatusResponse {
            success: true,
            message: format!("Database '{}' started", req.name),
        }))
    }

    /// 데이터베이스 중지
    pub async fn stop_database_impl(
        &self,
        request: Request<DatabaseRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();

        if req.name == "system" {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: "Cannot start/stop system database".to_string(),
            }));
        }

        if req.name == "zeta4j" {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: "Cannot stop default database 'zeta4j'".to_string(),
            }));
        }

        let Some(db_manager) = self.state.db_manager() else {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: "Database manager is unavailable".to_string(),
            }));
        };
        if let Err(error) = db_manager.stop(&req.name).await {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Failed to stop database: {error}"),
            }));
        }
        let catalog = self.state.system_service().catalog();
        if let Err(error) = catalog.set_state(&req.name, CatalogDatabaseState::Offline) {
            let _ = db_manager.start(&req.name).await;
            return Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Failed to persist stopped database state: {error}"),
            }));
        }
        Ok(Response::new(StatusResponse {
            success: true,
            message: format!("Database '{}' stopped", req.name),
        }))
    }

    /// 풀텍스트 인덱스 삭제
    pub async fn drop_fulltext_index_impl(
        &self,
        request: Request<DropIndexRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();
        if req.database.is_empty() || req.name.is_empty() {
            return Err(Status::invalid_argument(
                "database and index name cannot be empty",
            ));
        }
        let db_manager = self
            .state
            .db_manager()
            .ok_or_else(|| Status::failed_precondition("Database manager is unavailable"))?;
        let database = db_manager
            .get(&req.database)
            .await
            .map_err(|error| Status::not_found(error.to_string()))?;
        let context = database.storage_context.as_ref().ok_or_else(|| {
            Status::failed_precondition(format!(
                "database '{}' has no persistent storage context",
                req.database
            ))
        })?;
        let _maintenance_guard = context.lock_index_maintenance().await;
        match context.deactivate_fulltext_index(&req.name) {
            Ok(()) => Ok(Response::new(StatusResponse {
                success: true,
                message: format!(
                    "Fulltext index '{}' dropped from database '{}'",
                    req.name, req.database
                ),
            })),
            Err(error) => Ok(Response::new(StatusResponse {
                success: false,
                message: error.to_string(),
            })),
        }
    }
}

fn directory_size(path: &std::path::Path) -> Result<u64, String> {
    matching_file_size(path, |_| true)
}

fn matching_file_size(
    path: &std::path::Path,
    predicate: impl Fn(&str) -> bool + Copy,
) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0_u64;
    let entries = std::fs::read_dir(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read directory entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", entry.path().display()))?;
        if file_type.is_dir() {
            total = total
                .checked_add(matching_file_size(&entry.path(), predicate)?)
                .ok_or_else(|| format!("directory size overflow under {}", path.display()))?;
        } else if file_type.is_file() && predicate(entry.file_name().to_string_lossy().as_ref()) {
            total = total
                .checked_add(
                    entry
                        .metadata()
                        .map_err(|error| {
                            format!("failed to inspect {}: {error}", entry.path().display())
                        })?
                        .len(),
                )
                .ok_or_else(|| format!("directory size overflow under {}", path.display()))?;
        }
    }
    Ok(total)
}
