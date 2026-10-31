// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server database initialization utilities.

use std::sync::Arc;

use log::info;

use async_trait::async_trait;
use secrecy::ExposeSecret;
use zeta4j_server::config::ConfigRegistry;
use zeta4j_server::engine::StorageType;
use zeta4j_server::server::fulltext::{DatabaseFulltextSearchProvider, SharedFulltextProvider};
use zeta4j_server::server::system::{
    CatalogDatabaseState, DatabaseEntry, SystemDatabaseService, SystemError,
};
use zeta4j_server::server::tenant::{
    TenantDatabaseConfig, TenantDatabaseManager, TenantDatabaseManagerConfig,
};
use zeta4j_server::server::vector::{DatabaseVectorSearchProvider, SharedVectorProvider};

use super::ValidatedStartupConfig;

/// Database initialization errors
#[derive(Debug)]
pub enum DatabaseError {
    /// System database initialization failed
    SystemDb(String),
    /// Tenant database manager initialization failed
    TenantDb(String),
    /// Default engine not found
    DefaultEngineNotFound,
}

impl std::fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseError::SystemDb(msg) => {
                write!(f, "Failed to initialize system database: {}", msg)
            }
            DatabaseError::TenantDb(msg) => {
                write!(f, "Failed to initialize database manager: {}", msg)
            }
            DatabaseError::DefaultEngineNotFound => {
                write!(f, "Default database engine not found")
            }
        }
    }
}

impl std::error::Error for DatabaseError {}

/// Database services container
pub struct DatabaseServices {
    /// System database service (catalog, security, schema)
    pub system_db_service: Arc<SystemDatabaseService>,
    /// Tenant database manager (multi-database support)
    pub db_manager: Arc<TenantDatabaseManager>,
    /// Default database engine
    pub default_engine: Arc<zeta4j_server::engine::GraphEngine>,
    /// Fulltext search provider (CALL db.index.fulltext.* 프로시저용)
    pub fulltext_provider: SharedFulltextProvider,
    /// Vector search provider (CALL db.index.vector.queryNodes).
    pub vector_provider: SharedVectorProvider,
    /// 공유 인덱스 캐시 with L2 persistence (서버 재시작 후에도 캐시 유지)
    /// Arc로 래핑하여 여러 쿼리 실행기 간 공유
    pub shared_index_cache: Arc<zeta4j_cache::IndexCache>,
}

/// Runtime adapter used by every Cypher protocol surface.
pub struct RuntimeAdminService {
    system: Arc<SystemDatabaseService>,
    databases: Arc<TenantDatabaseManager>,
    bolt_address: String,
    bootstrap_completion_lock: tokio::sync::Mutex<()>,
}

impl RuntimeAdminService {
    pub fn new(
        system: Arc<SystemDatabaseService>,
        databases: Arc<TenantDatabaseManager>,
        bolt_address: impl Into<String>,
    ) -> Self {
        Self {
            system,
            databases,
            bolt_address: bolt_address.into(),
            bootstrap_completion_lock: tokio::sync::Mutex::new(()),
        }
    }

    fn system_error(error: SystemError) -> zeta4j_cypher_executor::AdminServiceError {
        use zeta4j_cypher_executor::AdminServiceError;
        match error {
            SystemError::UserNotFound(name) => AdminServiceError::NotFound {
                entity: "User",
                name,
            },
            SystemError::UserAlreadyExists(name) => AdminServiceError::AlreadyExists {
                entity: "User",
                name,
            },
            SystemError::DatabaseNotFound(name) => AdminServiceError::NotFound {
                entity: "Database",
                name,
            },
            SystemError::DatabaseAlreadyExists(name) => AdminServiceError::AlreadyExists {
                entity: "Database",
                name,
            },
            SystemError::BootstrapExpired => AdminServiceError::BootstrapExpired,
            SystemError::BootstrapConsumed => AdminServiceError::BootstrapConsumed,
            SystemError::InvalidPassword => AdminServiceError::InvalidPassword,
            SystemError::AuthenticationFailed(_) => AdminServiceError::InvalidCredential,
            other => AdminServiceError::Storage(other.to_string()),
        }
    }

    fn tenant_error(
        error: zeta4j_server::server::tenant::TenantDbError,
    ) -> zeta4j_cypher_executor::AdminServiceError {
        use zeta4j_cypher_executor::AdminServiceError;
        use zeta4j_server::server::tenant::TenantDbError;
        match error {
            TenantDbError::DatabaseNotFound(name) => AdminServiceError::NotFound {
                entity: "Database",
                name,
            },
            TenantDbError::DatabaseAlreadyExists(name) => AdminServiceError::AlreadyExists {
                entity: "Database",
                name,
            },
            other => AdminServiceError::Storage(other.to_string()),
        }
    }

    fn user_info(user: zeta4j_server::server::system::User) -> zeta4j_cypher_executor::UserInfo {
        zeta4j_cypher_executor::UserInfo {
            username: user.username,
            password_change_required: user.password_change_required,
            suspended: user.suspended,
            home_database: user.home_database,
        }
    }
}

#[async_trait]
impl zeta4j_cypher_executor::AdminService for RuntimeAdminService {
    fn supports(&self, capability: zeta4j_cypher_executor::AdminCapability) -> bool {
        let _ = capability;
        true
    }

    async fn list_users(
        &self,
    ) -> zeta4j_cypher_executor::AdminServiceResult<Vec<zeta4j_cypher_executor::UserInfo>> {
        self.system
            .security()
            .list_users()
            .map(|users| users.into_iter().map(Self::user_info).collect())
            .map_err(Self::system_error)
    }

    async fn get_user(
        &self,
        username: &str,
    ) -> zeta4j_cypher_executor::AdminServiceResult<Option<zeta4j_cypher_executor::UserInfo>> {
        self.system
            .security()
            .get_user(username)
            .map(|user| user.map(Self::user_info))
            .map_err(Self::system_error)
    }

    async fn create_user(
        &self,
        username: &str,
        password: &str,
        password_change_required: bool,
    ) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        self.system
            .security()
            .create_user_with_requirement(username, password, password_change_required)
            .map_err(Self::system_error)
    }

    async fn delete_user(&self, username: &str) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        self.system
            .security()
            .delete_user(username)
            .map_err(Self::system_error)
    }

    async fn change_password(
        &self,
        username: &str,
        new_password: &str,
    ) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        self.system
            .security()
            .change_password(username, new_password)
            .map_err(Self::system_error)
    }

    async fn complete_bootstrap(
        &self,
        username: &str,
        current_password: &str,
        new_password: &str,
    ) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        let _guard = self.bootstrap_completion_lock.lock().await;
        self.system
            .security()
            .complete_bootstrap(username, current_password, new_password)
            .map(|_| ())
            .map_err(Self::system_error)
    }

    async fn set_suspended(
        &self,
        username: &str,
        suspended: bool,
    ) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        self.system
            .security()
            .set_suspended(username, suspended)
            .map_err(Self::system_error)
    }

    async fn set_password_change_required(
        &self,
        username: &str,
        required: bool,
    ) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        self.system
            .security()
            .set_password_change_required(username, required)
            .map_err(Self::system_error)
    }

    async fn list_databases(
        &self,
    ) -> zeta4j_cypher_executor::AdminServiceResult<Vec<zeta4j_cypher_executor::DatabaseInfo>> {
        let catalog = self.system.catalog().list().map_err(Self::system_error)?;
        let mut result = Vec::with_capacity(catalog.len());
        for entry in catalog {
            let runtime = self.databases.get(&entry.name).await.map_err(|error| {
                zeta4j_cypher_executor::AdminServiceError::Invariant(format!(
                    "catalog database '{}' has no runtime state: {}",
                    entry.name, error
                ))
            })?;
            result.push(zeta4j_cypher_executor::DatabaseInfo {
                name: entry.name,
                database_type: "standard".to_string(),
                address: Some(self.bolt_address.clone()),
                role: Some("standalone".to_string()),
                requested_status: entry.state.to_string(),
                current_status: runtime.state().await.to_string(),
                is_default: entry.is_default,
                store_size: None,
            });
        }
        Ok(result)
    }

    async fn get_database(
        &self,
        name: &str,
    ) -> zeta4j_cypher_executor::AdminServiceResult<Option<zeta4j_cypher_executor::DatabaseInfo>>
    {
        Ok(self
            .list_databases()
            .await?
            .into_iter()
            .find(|database| database.name == name))
    }

    async fn create_database(&self, name: &str) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        let _creation_guard = self.system.lock_database_creation().await;
        if self
            .system
            .catalog()
            .exists(name)
            .map_err(Self::system_error)?
        {
            return Err(zeta4j_cypher_executor::AdminServiceError::AlreadyExists {
                entity: "Database",
                name: name.to_string(),
            });
        }
        let path = self.databases.database_path(name).ok_or_else(|| {
            zeta4j_cypher_executor::AdminServiceError::Invariant(format!(
                "database path cannot be resolved for '{}'",
                name
            ))
        })?;
        self.system
            .catalog()
            .register(DatabaseEntry::new(name, path))
            .map_err(Self::system_error)?;
        if let Err(error) = self
            .databases
            .create(name, TenantDatabaseConfig::default())
            .await
        {
            let _ = self.system.catalog().unregister(name);
            return Err(Self::tenant_error(error));
        }
        if let Err(error) = self
            .system
            .catalog()
            .set_state(name, CatalogDatabaseState::Online)
        {
            let _ = self.databases.delete(name).await;
            let _ = self.system.catalog().unregister(name);
            return Err(Self::system_error(error));
        }
        Ok(())
    }

    async fn drop_database(&self, name: &str) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        let entry = self
            .system
            .catalog()
            .get(name)
            .map_err(Self::system_error)?
            .ok_or_else(|| zeta4j_cypher_executor::AdminServiceError::NotFound {
                entity: "Database",
                name: name.to_string(),
            })?;
        if entry.is_default {
            return Err(zeta4j_cypher_executor::AdminServiceError::Unsupported(
                "the default database cannot be dropped".to_string(),
            ));
        }
        self.databases
            .delete(name)
            .await
            .map_err(Self::tenant_error)?;
        self.system
            .catalog()
            .unregister(name)
            .map_err(Self::system_error)
    }

    async fn start_database(&self, name: &str) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        self.databases
            .start(name)
            .await
            .map_err(Self::tenant_error)?;
        if let Err(error) = self
            .system
            .catalog()
            .set_state(name, CatalogDatabaseState::Online)
        {
            let _ = self.databases.stop(name).await;
            return Err(Self::system_error(error));
        }
        Ok(())
    }

    async fn stop_database(&self, name: &str) -> zeta4j_cypher_executor::AdminServiceResult<()> {
        self.databases
            .stop(name)
            .await
            .map_err(Self::tenant_error)?;
        if let Err(error) = self
            .system
            .catalog()
            .set_state(name, CatalogDatabaseState::Offline)
        {
            let _ = self.databases.start(name).await;
            return Err(Self::system_error(error));
        }
        Ok(())
    }
}

/// Database initializer
pub struct DatabaseInit<'a> {
    config: &'a ValidatedStartupConfig,
    registry: Option<&'a ConfigRegistry>,
}

impl<'a> DatabaseInit<'a> {
    /// Create a new DatabaseInit
    pub fn new(config: &'a ValidatedStartupConfig, registry: Option<&'a ConfigRegistry>) -> Self {
        Self { config, registry }
    }

    /// Initialize all database services
    pub async fn initialize(&self) -> Result<DatabaseServices, DatabaseError> {
        // 1. System database initialization
        let system_db_service = self.init_system_db()?;

        // 2. Tenant database manager initialization
        let db_manager = self.init_tenant_db_manager().await?;

        // 3. Get default engine
        let default_engine = db_manager
            .default_engine()
            .await
            .ok_or(DatabaseError::DefaultEngineNotFound)?;

        info!("Tenant database manager initialized (default database: zeta4j)");

        // 4. Initialize fulltext search provider
        let fulltext_provider: SharedFulltextProvider =
            Arc::new(DatabaseFulltextSearchProvider::new(db_manager.clone()));
        let vector_provider: SharedVectorProvider =
            Arc::new(DatabaseVectorSearchProvider::new(db_manager.clone()));

        // 5. Initialize index cache with L2 persistence (Arc로 래핑하여 공유)
        let shared_index_cache = Arc::new(self.init_index_cache()?);

        Ok(DatabaseServices {
            system_db_service,
            db_manager,
            default_engine,
            fulltext_provider,
            vector_provider,
            shared_index_cache,
        })
    }

    /// Initialize system database
    fn init_system_db(&self) -> Result<Arc<SystemDatabaseService>, DatabaseError> {
        info!("Initializing system database...");
        info!("Data directory: {}", self.config.data_dir.display());
        info!("Log directory: {}", self.config.log_dir.display());

        let system_db_service = Arc::new(
            SystemDatabaseService::open(&self.config.data_dir)
                .map_err(|e| DatabaseError::SystemDb(e.to_string()))?,
        );

        if system_db_service
            .security()
            .list_users()
            .map_err(|error| DatabaseError::SystemDb(error.to_string()))?
            .is_empty()
            && self.config.auth_enabled
        {
            let credential = self.config.bootstrap_credential().ok_or_else(|| {
                DatabaseError::SystemDb(
                    "validated startup is missing the fresh-state bootstrap credential".to_string(),
                )
            })?;
            system_db_service
                .security()
                .provision_bootstrap_admin(
                    &credential.username,
                    credential.password.expose_secret(),
                    credential.expires_at,
                )
                .map_err(|error| DatabaseError::SystemDb(error.to_string()))?;
        }

        // Print system summary
        if let Ok(summary) = system_db_service.summary() {
            info!(
                "System database: {} database(s), {} user(s), {} index(es)",
                summary.databases, summary.users, summary.indexes
            );
        }

        Ok(system_db_service)
    }

    /// Initialize tenant database manager
    async fn init_tenant_db_manager(&self) -> Result<Arc<TenantDatabaseManager>, DatabaseError> {
        info!("Initializing tenant database manager...");

        // Build configuration
        let mut db_manager_config = TenantDatabaseManagerConfig::default()
            .with_data_path(&self.config.data_dir)
            .with_storage_type(StorageType::Disk)
            .with_max_databases(self.config.max_databases);

        // Apply core config if available
        if let Some(registry) = self.registry {
            let core = registry.core();
            db_manager_config = db_manager_config.with_core_config(core.clone());
            self.log_core_config(core);
        }

        // Create database manager
        let db_manager = Arc::new(
            TenantDatabaseManager::new_with_storage(db_manager_config)
                .await
                .map_err(|e| DatabaseError::TenantDb(e.to_string()))?,
        );

        Ok(db_manager)
    }

    /// Log core configuration details
    fn log_core_config(&self, core: &zeta4j_server::config::Zeta4jConfig) {
        info!(
            "Applied core.toml: tx_timeout={}ms, lock_wait={}ms, query_timeout={}ms",
            core.transaction.timeout_ms, core.transaction.lock_wait_ms, core.query.timeout_ms
        );
        info!(
            "  WAL: segment_size={}MB, max_segments={}, flush_interval={}",
            core.wal.max_segment_size / (1024 * 1024),
            core.wal.max_segments,
            core.wal.flush_interval
        );
        info!(
            "  MVCC: max_write_sets={}, snapshot_cache_size={}",
            core.mvcc.max_write_sets, core.mvcc.snapshot_cache_size
        );
        info!(
            "  Compaction: min_records={}, max_records={}",
            core.compaction.min_records, core.compaction.max_records
        );
        info!(
            "  Index: auto_create={}, default_type={}",
            core.index.auto_create, core.index.default_type
        );
        info!(
            "  Logging: level={}, format={}",
            core.logging.level, core.logging.format
        );
        info!("  Storage: data_dir={}", core.storage.data_dir);
    }

    /// Initialize index cache with L2 persistence
    ///
    /// 인덱스 메타데이터 캐시를 redb 기반 L2 영속화와 함께 초기화합니다.
    /// 서버 재시작 후에도 캐시가 유지되어 워밍업 시간을 단축합니다.
    fn init_index_cache(&self) -> Result<zeta4j_cache::IndexCache, DatabaseError> {
        info!("Initializing index cache with L2 persistence...");

        // 캐시 디렉토리: {data_dir}/cache/index/
        let cache_dir = self.config.data_dir.join("cache").join("index");

        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| DatabaseError::SystemDb(format!("Failed to create cache dir: {}", e)))?;

        let cache_db_path = cache_dir.join("index.redb");
        let db =
            Arc::new(redb::Database::create(&cache_db_path).map_err(|e| {
                DatabaseError::SystemDb(format!("Failed to create cache db: {}", e))
            })?);

        let cache = zeta4j_cache::IndexCache::new(Some(db))
            .map_err(|e| DatabaseError::SystemDb(format!("Failed to init index cache: {}", e)))?;

        info!(
            "Index cache initialized with L2 persistence (path: {})",
            cache_dir.display()
        );

        Ok(cache)
    }
}
