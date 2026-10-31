// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 테넌트 데이터베이스 관리자 구현.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::info;
use zeta4j_graph::config::Zeta4jConfig;
use zeta4j_graph::engine::{GraphEngine, GraphEngineConfig, StorageType};
use zeta4j_graph::storage::memory::MemoryLimiterConfig;

use super::{
    DatabaseContext, DatabaseId, DatabaseMetadata, DatabaseSnapshot, DatabaseState, TenantDatabase,
    TenantDatabaseConfig, TenantDbError,
};

/// One catalog entry plus its complete canonical database image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantDatabaseSnapshot {
    pub id: DatabaseId,
    pub name: String,
    pub state: DatabaseState,
    pub config: TenantDatabaseConfig,
    pub metadata: DatabaseMetadata,
    pub database: DatabaseSnapshot,
}

/// 테넌트 데이터베이스 관리자 설정
#[derive(Debug, Clone)]
pub struct TenantDatabaseManagerConfig {
    /// 기본 데이터베이스 이름
    pub default_database: String,
    /// 서버 주소
    pub address: String,
    /// 최대 데이터베이스 수
    pub max_databases: usize,
    /// 기본 데이터 디렉토리 (예: ~/.zeta4j/databases)
    pub base_data_path: Option<PathBuf>,
    /// 스토리지 타입
    pub storage_type: StorageType,
    /// Core 설정 (트랜잭션, WAL, 쿼리 등)
    pub core_config: Option<Zeta4jConfig>,
}

impl Default for TenantDatabaseManagerConfig {
    fn default() -> Self {
        Self {
            default_database: "zeta4j".to_string(),
            address: "localhost:7687".to_string(),
            max_databases: 100,
            base_data_path: None,
            storage_type: StorageType::Disk,
            core_config: None,
        }
    }
}

impl TenantDatabaseManagerConfig {
    /// 데이터 경로 설정
    pub fn with_data_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.base_data_path = Some(path.into());
        self
    }

    /// 스토리지 타입 설정
    pub fn with_storage_type(mut self, storage_type: StorageType) -> Self {
        self.storage_type = storage_type;
        self
    }

    /// 최대 데이터베이스 수 설정
    pub fn with_max_databases(mut self, max: usize) -> Self {
        self.max_databases = max;
        self
    }

    /// Core 설정 (트랜잭션, WAL, 쿼리 등)
    pub fn with_core_config(mut self, config: Zeta4jConfig) -> Self {
        self.core_config = Some(config);
        self
    }

    /// 데이터베이스별 경로 생성 (graphs/{db_name}/)
    pub fn database_path(&self, db_name: &str) -> Option<PathBuf> {
        self.base_data_path
            .as_ref()
            .map(|base| base.join("graphs").join(db_name))
    }

    /// Build the one canonical graph-engine configuration used by initial
    /// creation, existing database load, dynamic creation and snapshot restore.
    pub fn graph_engine_config(&self, path: &Path) -> GraphEngineConfig {
        let mut engine = GraphEngineConfig {
            storage_type: self.storage_type,
            data_path: path.to_string_lossy().to_string(),
            wal_path: path.join("wal.log").to_string_lossy().to_string(),
            ..GraphEngineConfig::default()
        };
        if let Some(core) = &self.core_config {
            engine.wal_max_segment_size = core.wal.max_segment_size;
            engine.wal_max_segments = core.wal.max_segments;
            engine.wal_flush_interval = core.wal.flush_interval;
            engine.lock_timeout_ms = core.transaction.lock_wait_ms;
            engine.mvcc_max_write_sets = core.mvcc.max_write_sets;
            engine.mvcc_snapshot_cache_size = core.mvcc.snapshot_cache_size;
            engine.compaction_min_records = core.compaction.min_records;
            engine.compaction_max_records = core.compaction.max_records;
            engine.index_auto_create = core.index.auto_create;
            engine.index_default_type = core.index.default_type.clone();
            engine.memory_limit =
                core.storage
                    .memory_limit_bytes
                    .map(|max_memory_bytes| MemoryLimiterConfig {
                        max_memory_bytes,
                        ..MemoryLimiterConfig::default()
                    });
        }
        engine
    }
}

/// 테넌트 데이터베이스 관리자
pub struct TenantDatabaseManager {
    /// 데이터베이스 맵 (이름 → 데이터베이스)
    databases: RwLock<HashMap<String, Arc<TenantDatabase>>>,
    /// ID → 이름 매핑
    id_to_name: RwLock<HashMap<DatabaseId, String>>,
    /// Lock-short runtime routing projection from GraphEngine identity to its
    /// unified logical database context. Procedure execution is synchronous,
    /// so it cannot await the authoritative database catalog lock.
    engine_contexts: std::sync::RwLock<HashMap<usize, Weak<DatabaseContext>>>,
    /// 설정
    config: TenantDatabaseManagerConfig,
}

impl TenantDatabaseManager {
    /// Capture one canonical database image for a backup.
    pub async fn snapshot_database(
        &self,
        name: &str,
    ) -> Result<TenantDatabaseSnapshot, TenantDbError> {
        let database = self.get(name).await?;
        let context = database.storage_context.as_ref().ok_or_else(|| {
            TenantDbError::Internal(format!(
                "database '{}' has no canonical storage context",
                database.name
            ))
        })?;
        Ok(TenantDatabaseSnapshot {
            id: database.id,
            name: database.name.clone(),
            state: database.state().await,
            config: database.config.clone(),
            metadata: database.metadata.clone(),
            database: context
                .snapshot()
                .map_err(|error| TenantDbError::Internal(error.to_string()))?,
        })
    }

    /// Atomically replace one existing database from a fully validated backup.
    pub async fn restore_database(
        &self,
        expected_name: &str,
        snapshot: &TenantDatabaseSnapshot,
    ) -> Result<(), TenantDbError> {
        if snapshot.name != expected_name {
            return Err(TenantDbError::Internal(
                "backup database identity does not match restore target".to_string(),
            ));
        }
        let database = self.get(expected_name).await?;
        if database.id != snapshot.id {
            return Err(TenantDbError::Internal(
                "backup database generation does not match restore target".to_string(),
            ));
        }
        let context = database.storage_context.as_ref().ok_or_else(|| {
            TenantDbError::Internal(format!(
                "database '{}' has no canonical storage context",
                database.name
            ))
        })?;
        let previous = context
            .snapshot()
            .map_err(|error| TenantDbError::Internal(error.to_string()))?;
        context
            .restore(&snapshot.database)
            .map_err(|error| TenantDbError::Internal(error.to_string()))?;
        if let Some(engine) = &database.engine
            && let Err(restore_error) = engine.reload_canonical_projection().await
        {
            context.restore(&previous).map_err(|rollback_error| {
                TenantDbError::Internal(format!(
                    "database restore projection failed ({restore_error}); canonical rollback failed: {rollback_error}"
                ))
            })?;
            engine
                .reload_canonical_projection()
                .await
                .map_err(|rollback_error| {
                    TenantDbError::Internal(format!(
                        "database restore projection failed ({restore_error}); projection rollback failed: {rollback_error}"
                    ))
                })?;
            return Err(TenantDbError::EngineError(restore_error.to_string()));
        }
        Ok(())
    }

    /// 새 관리자 생성 (메모리 모드, 엔진 없음)
    pub fn new(config: TenantDatabaseManagerConfig) -> Self {
        let mut databases = HashMap::new();
        let mut id_to_name = HashMap::new();

        // 기본 데이터베이스 생성 (zeta4j)
        let mut default_db = TenantDatabase::default_database();
        if let Some(db_path) = config.database_path("zeta4j") {
            default_db.data_path = Some(db_path);
        }
        let default_db = Arc::new(default_db);
        id_to_name.insert(default_db.id, default_db.name.clone());
        databases.insert(default_db.name.clone(), default_db);

        // Note: system DB는 별도의 SystemDatabase (RocksDB)로 관리

        let engine_contexts = Self::engine_context_projection(&databases);
        Self {
            databases: RwLock::new(databases),
            id_to_name: RwLock::new(id_to_name),
            engine_contexts: std::sync::RwLock::new(engine_contexts),
            config,
        }
    }

    /// 스토리지와 함께 관리자 생성 (디스크 모드, 엔진 자동 생성)
    ///
    /// zeta4j 데이터베이스의 디렉토리와 엔진을 자동 생성합니다.
    /// 기존에 생성된 데이터베이스도 자동으로 스캔하여 로드합니다.
    /// Note: system DB는 별도의 SystemDatabase (RocksDB)로 관리
    #[allow(clippy::field_reassign_with_default)]
    pub async fn new_with_storage(
        config: TenantDatabaseManagerConfig,
    ) -> Result<Self, TenantDbError> {
        let mut databases = HashMap::new();
        let mut id_to_name = HashMap::new();

        // 기본 데이터베이스 (zeta4j) 생성
        let default_db = Self::create_initial_database(&config, "zeta4j", true, false).await?;
        id_to_name.insert(default_db.id, default_db.name.clone());
        databases.insert(default_db.name.clone(), Arc::new(default_db));

        // 기존 데이터베이스 스캔 및 로드 (RwLock 생성 전에 수행)
        Self::scan_and_load_databases(&config, &mut databases, &mut id_to_name).await;

        let engine_contexts = Self::engine_context_projection(&databases);
        Ok(Self {
            databases: RwLock::new(databases),
            id_to_name: RwLock::new(id_to_name),
            engine_contexts: std::sync::RwLock::new(engine_contexts),
            config,
        })
    }

    /// 기존 데이터베이스 스캔 및 로드 (초기화 시 사용)
    ///
    /// graphs/ 디렉토리를 스캔하여 기존에 생성된 데이터베이스를 로드합니다.
    /// - `metadata.json`과 canonical `database.redb`가 있는 디렉토리만 데이터베이스로 인식
    /// - `metadata.json`의 status가 "online"인 데이터베이스만 활성화
    async fn scan_and_load_databases(
        config: &TenantDatabaseManagerConfig,
        databases: &mut HashMap<String, Arc<TenantDatabase>>,
        id_to_name: &mut HashMap<DatabaseId, String>,
    ) {
        let Some(base_path) = &config.base_data_path else {
            return;
        };

        // graphs/ 디렉토리에서 데이터베이스 스캔
        let graphs_path = base_path.join("graphs");
        if !graphs_path.exists() {
            return;
        }

        let entries = match std::fs::read_dir(&graphs_path) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to read graphs directory: {}", e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let db_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            // 시스템/기본 데이터베이스는 스킵 (이미 처리됨)
            if db_name == "zeta4j" || db_name == "system" {
                continue;
            }

            // 숨김 디렉토리 스킵
            if db_name.starts_with('.') {
                continue;
            }

            // 이미 로드되었는지 확인
            if databases.contains_key(&db_name) {
                continue;
            }

            // A logical database is canonical only when its unified redb exists.
            let database_path = path.join("database.redb");
            if !database_path.exists() {
                info!(
                    "Skipping directory '{}': canonical database.redb not found",
                    db_name
                );
                continue;
            }

            // 메타데이터 파일 읽기
            let metadata_path = path.join("metadata.json");
            let should_load = if metadata_path.exists() {
                match std::fs::read_to_string(&metadata_path) {
                    Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                        Ok(json) => {
                            let status = json
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("offline");
                            status == "online"
                        }
                        Err(_) => false,
                    },
                    Err(_) => false,
                }
            } else {
                // 메타데이터 없으면 로드하지 않음
                false
            };

            if !should_load {
                info!(
                    "Database '{}' found but not loading (status is not 'online')",
                    db_name
                );
                continue;
            }

            // 데이터베이스 로드
            match Self::load_database_from_path(config, &db_name, &path).await {
                Ok(db) => {
                    let db = Arc::new(db);
                    id_to_name.insert(db.id, db.name.clone());
                    databases.insert(db_name.clone(), db);
                    info!("Loaded existing database '{}'", db_name);
                }
                Err(e) => {
                    tracing::warn!("Failed to load database '{}': {}", db_name, e);
                }
            }
        }
    }

    /// 경로에서 데이터베이스 로드
    #[allow(clippy::field_reassign_with_default)]
    async fn load_database_from_path(
        config: &TenantDatabaseManagerConfig,
        name: &str,
        path: &std::path::Path,
    ) -> Result<TenantDatabase, TenantDbError> {
        let engine_config = config.graph_engine_config(path);

        let storage_context = Arc::new(
            DatabaseContext::open(name, path)
                .map_err(|error| TenantDbError::Internal(error.to_string()))?,
        );
        let engine = GraphEngine::new(engine_config, storage_context.clone())
            .await
            .map_err(|e| {
                TenantDbError::EngineError(format!("Failed to load engine for {}: {}", name, e))
            })?;

        // TenantDatabase 생성
        let mut db = TenantDatabase::new(name);
        db.engine = Some(Arc::new(engine));
        db.data_path = Some(path.to_path_buf());
        db.storage_context = Some(storage_context);
        db.state = tokio::sync::RwLock::new(DatabaseState::Online);

        // Core 설정 적용
        if let Some(ref core) = config.core_config {
            db.config.tx_timeout_ms = core.transaction.timeout_ms;
            db.config.query_timeout_ms = core.query.timeout_ms;
        }

        Ok(db)
    }

    /// 기존 데이터베이스 로드 (런타임에서 동적 로드용)
    ///
    /// 이 메서드는 서버 시작 후에 새로운 데이터베이스를 동적으로 로드할 때 사용합니다.
    async fn load_existing_database(
        &self,
        name: &str,
        path: &std::path::Path,
    ) -> Result<(), TenantDbError> {
        // 데이터베이스 로드
        let db = Self::load_database_from_path(&self.config, name, path).await?;
        let db = Arc::new(db);

        // 등록
        let mut databases = self.databases.write().await;
        let mut id_to_name = self.id_to_name.write().await;

        id_to_name.insert(db.id, db.name.clone());
        databases.insert(name.to_string(), db.clone());
        self.register_engine_context(&db);

        Ok(())
    }

    /// 초기 데이터베이스 생성 (디렉토리 + 엔진)
    #[allow(clippy::field_reassign_with_default)]
    async fn create_initial_database(
        config: &TenantDatabaseManagerConfig,
        name: &str,
        is_default: bool,
        is_system: bool,
    ) -> Result<TenantDatabase, TenantDbError> {
        let db_path = config.database_path(name);

        // 디렉토리 생성
        if let Some(ref path) = db_path
            && !path.exists()
        {
            std::fs::create_dir_all(path).map_err(|e| {
                TenantDbError::IoError(format!(
                    "Failed to create database directory {}: {}",
                    path.display(),
                    e
                ))
            })?;
            info!("Created database directory: {}", path.display());
        }

        let storage_context = db_path
            .as_ref()
            .map(|path| DatabaseContext::open(name, path))
            .transpose()
            .map_err(|error| TenantDbError::Internal(error.to_string()))?
            .map(Arc::new);

        // 엔진 생성
        let engine = if let Some(ref path) = db_path {
            let engine_config = config.graph_engine_config(path);

            let canonical_store = storage_context.clone().ok_or_else(|| {
                TenantDbError::Internal(format!("storage context for database {name} is missing"))
            })?;
            let engine = GraphEngine::new(engine_config, canonical_store)
                .await
                .map_err(|e| {
                    TenantDbError::EngineError(format!(
                        "Failed to create engine for {}: {}",
                        name, e
                    ))
                })?;
            info!(
                "Created graph engine for database '{}' at {}",
                name,
                path.display()
            );
            Some(Arc::new(engine))
        } else {
            None
        };

        // 데이터베이스 객체 생성
        let mut db = TenantDatabase::new(name);
        db.engine = engine;
        db.data_path = db_path.clone();
        db.storage_context = storage_context;
        db.state = tokio::sync::RwLock::new(DatabaseState::Online);

        // Core 설정 적용 (트랜잭션/쿼리 타임아웃)
        if let Some(ref core) = config.core_config {
            db.config.tx_timeout_ms = core.transaction.timeout_ms;
            db.config.query_timeout_ms = core.query.timeout_ms;
        }

        if is_default {
            db.id = DatabaseId::default_database();
            db.metadata = DatabaseMetadata::default_database();
        } else if is_system {
            db.metadata = DatabaseMetadata::system_database();
        }

        Ok(db)
    }

    fn engine_context_projection(
        databases: &HashMap<String, Arc<TenantDatabase>>,
    ) -> HashMap<usize, Weak<DatabaseContext>> {
        databases
            .values()
            .filter_map(|database| {
                let engine = database.engine.as_ref()?;
                let context = database.storage_context.as_ref()?;
                Some((Self::engine_key(engine), Arc::downgrade(context)))
            })
            .collect()
    }

    fn engine_key(engine: &GraphEngine) -> usize {
        std::ptr::from_ref(engine) as usize
    }

    fn register_engine_context(&self, database: &TenantDatabase) {
        let (Some(engine), Some(context)) = (&database.engine, &database.storage_context) else {
            return;
        };
        let mut contexts = match self.engine_contexts.write() {
            Ok(contexts) => contexts,
            Err(poisoned) => poisoned.into_inner(),
        };
        contexts.insert(Self::engine_key(engine), Arc::downgrade(context));
    }

    fn unregister_engine_context(&self, database: &TenantDatabase) {
        let Some(engine) = &database.engine else {
            return;
        };
        let mut contexts = match self.engine_contexts.write() {
            Ok(contexts) => contexts,
            Err(poisoned) => poisoned.into_inner(),
        };
        contexts.remove(&Self::engine_key(engine));
    }

    /// Resolve the unified logical database context for an already selected
    /// GraphEngine without blocking on the async database catalog.
    pub fn context_for_engine(&self, engine: &GraphEngine) -> Option<Arc<DatabaseContext>> {
        let contexts = match self.engine_contexts.read() {
            Ok(contexts) => contexts,
            Err(poisoned) => poisoned.into_inner(),
        };
        contexts
            .get(&Self::engine_key(engine))
            .and_then(Weak::upgrade)
    }

    /// 데이터베이스별 디렉토리 생성 및 GraphEngine 초기화
    async fn create_database_engine(
        &self,
        db_name: &str,
    ) -> Result<Option<(Arc<GraphEngine>, Arc<DatabaseContext>)>, TenantDbError> {
        // base_data_path가 없으면 엔진 없이 생성 (Memory 모드)
        let Some(db_path) = self.config.database_path(db_name) else {
            return Ok(None);
        };

        // 디렉토리 생성
        if !db_path.exists() {
            std::fs::create_dir_all(&db_path).map_err(|e| {
                TenantDbError::IoError(format!(
                    "Failed to create database directory {}: {}",
                    db_path.display(),
                    e
                ))
            })?;
            info!("Created database directory: {}", db_path.display());
        }

        let engine_config = self.config.graph_engine_config(&db_path);

        // GraphEngine 생성
        let storage_context = Arc::new(
            DatabaseContext::open(db_name, &db_path)
                .map_err(|error| TenantDbError::Internal(error.to_string()))?,
        );
        let engine = GraphEngine::new(engine_config, storage_context.clone())
            .await
            .map_err(|e| {
                TenantDbError::EngineError(format!(
                    "Failed to create engine for {}: {}",
                    db_name, e
                ))
            })?;

        info!(
            "Created graph engine for database '{}' at {}",
            db_name,
            db_path.display()
        );
        Ok(Some((Arc::new(engine), storage_context)))
    }

    /// 데이터베이스 목록 조회
    pub async fn list(&self) -> Vec<Arc<TenantDatabase>> {
        let databases = self.databases.read().await;
        databases.values().cloned().collect()
    }

    /// 이름으로 데이터베이스 조회
    pub async fn get(&self, name: &str) -> Result<Arc<TenantDatabase>, TenantDbError> {
        let databases = self.databases.read().await;
        databases
            .get(name)
            .cloned()
            .ok_or_else(|| TenantDbError::DatabaseNotFound(name.to_string()))
    }

    /// ID로 데이터베이스 조회
    pub async fn get_by_id(&self, id: &DatabaseId) -> Result<Arc<TenantDatabase>, TenantDbError> {
        let id_to_name = self.id_to_name.read().await;
        let name = id_to_name
            .get(id)
            .ok_or_else(|| TenantDbError::DatabaseNotFound(id.to_string()))?;

        let databases = self.databases.read().await;
        databases
            .get(name)
            .cloned()
            .ok_or_else(|| TenantDbError::DatabaseNotFound(name.clone()))
    }

    /// 데이터베이스 생성
    pub async fn create(
        &self,
        name: &str,
        config: TenantDatabaseConfig,
    ) -> Result<Arc<TenantDatabase>, TenantDbError> {
        // 이름 검증
        if !Self::is_valid_name(name) {
            return Err(TenantDbError::InvalidName(name.to_string()));
        }

        let mut databases = self.databases.write().await;
        let mut id_to_name = self.id_to_name.write().await;

        // 이미 존재하는지 확인
        if databases.contains_key(name) {
            return Err(TenantDbError::DatabaseAlreadyExists(name.to_string()));
        }

        // 최대 개수 확인
        if databases.len() >= self.config.max_databases {
            return Err(TenantDbError::MaxDatabasesReached(
                self.config.max_databases,
            ));
        }

        // 데이터베이스별 디렉토리 및 엔진 생성
        let engine = self.create_database_engine(name).await?;
        let db_path = self.config.database_path(name);

        // 새 데이터베이스 생성
        let mut db = TenantDatabase::new(name).with_config(config);
        if let Some((engine, storage_context)) = engine {
            db.engine = Some(engine);
            db.storage_context = Some(storage_context);
            // 엔진이 있으면 Online 상태로 시작
            db.state = tokio::sync::RwLock::new(DatabaseState::Online);
        }
        if let Some(ref path) = db_path {
            db.data_path = Some(path.clone());
            // metadata.json 작성 (서버 재시작 시 자동 로드용)
            let metadata = serde_json::json!({
                "status": "online",
                "name": name,
            });
            let meta_path = path.join("metadata.json");
            if let Ok(content) = serde_json::to_string_pretty(&metadata)
                && let Err(e) = std::fs::write(&meta_path, content)
            {
                tracing::warn!("Failed to write metadata.json for '{}': {}", name, e);
            }
        }

        let db = Arc::new(db);
        id_to_name.insert(db.id, db.name.clone());
        databases.insert(name.to_string(), db.clone());
        self.register_engine_context(&db);

        Ok(db)
    }

    /// 데이터베이스 삭제
    pub async fn delete(&self, name: &str) -> Result<(), TenantDbError> {
        // 시스템 DB 삭제 불가
        if name == "system" {
            return Err(TenantDbError::CannotDeleteSystem);
        }

        // 기본 DB 삭제 불가
        if name == self.config.default_database {
            return Err(TenantDbError::CannotDeleteDefault);
        }

        let mut databases = self.databases.write().await;
        let mut id_to_name = self.id_to_name.write().await;

        let db = databases
            .remove(name)
            .ok_or_else(|| TenantDbError::DatabaseNotFound(name.to_string()))?;

        id_to_name.remove(&db.id);
        self.unregister_engine_context(&db);

        Ok(())
    }

    /// 데이터베이스 시작
    ///
    /// 데이터베이스가 메모리에 없으면 디스크에서 로드합니다.
    ///
    /// 동시성 안전: 상태 전환 락과 CAS 패턴을 사용하여 TOCTOU 취약점 방지
    pub async fn start(&self, name: &str) -> Result<(), TenantDbError> {
        // 먼저 메모리에 있는지 확인
        let db_result = self.get(name).await;

        match db_result {
            Ok(db) => {
                // 상태 전환 락 획득 (TOCTOU 방지)
                let _state_guard = db.acquire_state_transition_lock().await;

                // CAS 패턴으로 상태 전환 (Offline → Starting)
                let current_state = db.state().await;
                if current_state.is_online() {
                    return Err(TenantDbError::InvalidState("already online".to_string()));
                }

                // 전환 중 상태면 동시 요청 거부
                if current_state.is_transitioning() {
                    return Err(TenantDbError::InvalidState(format!(
                        "database is currently {}",
                        current_state
                    )));
                }

                // CAS: Offline → Starting
                if !db
                    .try_set_state(DatabaseState::Offline, DatabaseState::Starting)
                    .await
                {
                    return Err(TenantDbError::InvalidState(
                        "state changed concurrently".to_string(),
                    ));
                }

                if db.engine.is_none() {
                    let error = TenantDbError::InvalidState(
                        "database has no canonical GraphEngine; reopen the database manager from unified storage"
                            .to_string(),
                    );
                    db.set_state(DatabaseState::Error(error.to_string())).await;
                    return Err(error);
                }

                db.set_state(DatabaseState::Online).await;
                Ok(())
            }
            Err(TenantDbError::DatabaseNotFound(_)) => {
                // 메모리에 없음 - 디스크에서 로드 시도
                if let Some(db_path) = self.config.database_path(name)
                    && db_path.exists()
                    && db_path.join("database.redb").exists()
                {
                    self.load_existing_database(name, &db_path).await?;
                    info!("Started database '{}' from disk", name);
                    return Ok(());
                }
                Err(TenantDbError::DatabaseNotFound(name.to_string()))
            }
            Err(e) => Err(e),
        }
    }

    /// 데이터베이스 중지
    ///
    /// 동시성 안전: 상태 전환 락과 CAS 패턴을 사용하여 TOCTOU 취약점 방지
    pub async fn stop(&self, name: &str) -> Result<(), TenantDbError> {
        let db = self.get(name).await?;

        // 상태 전환 락 획득 (TOCTOU 방지)
        let _state_guard = db.acquire_state_transition_lock().await;

        let current_state = db.state().await;
        if !current_state.is_online() {
            return Err(TenantDbError::InvalidState("not online".to_string()));
        }

        // 전환 중 상태면 동시 요청 거부
        if current_state.is_transitioning() {
            return Err(TenantDbError::InvalidState(format!(
                "database is currently {}",
                current_state
            )));
        }

        // CAS: Online → Stopping
        if !db
            .try_set_state(DatabaseState::Online, DatabaseState::Stopping)
            .await
        {
            return Err(TenantDbError::InvalidState(
                "state changed concurrently".to_string(),
            ));
        }

        info!("Stopping database '{}'", name);
        // 실제로는 여기서 엔진 종료 수행
        db.set_state(DatabaseState::Offline).await;
        info!("Database '{}' stopped (offline)", name);

        Ok(())
    }

    /// 기본 데이터베이스 반환
    pub async fn default_database(&self) -> Result<Arc<TenantDatabase>, TenantDbError> {
        self.get(&self.config.default_database).await
    }

    /// 데이터베이스 엔진 가져오기 (온라인 상태일 때만)
    pub async fn get_engine(&self, name: &str) -> Option<Arc<GraphEngine>> {
        match self.get(name).await {
            Ok(db) if db.is_online().await => db.engine.clone(),
            _ => None,
        }
    }

    /// 기본 데이터베이스 엔진 가져오기
    pub async fn default_engine(&self) -> Option<Arc<GraphEngine>> {
        self.get_engine(&self.config.default_database).await
    }

    /// 데이터베이스 존재 여부
    pub async fn exists(&self, name: &str) -> bool {
        let databases = self.databases.read().await;
        databases.contains_key(name)
    }

    /// 데이터베이스 상태 확인
    pub async fn is_online(&self, name: &str) -> bool {
        match self.get(name).await {
            Ok(db) => db.is_online().await,
            Err(_) => false,
        }
    }

    /// 데이터베이스 이름 유효성 검사
    pub fn is_valid_name(name: &str) -> bool {
        if name.is_empty() || name.len() > 63 {
            return false;
        }

        // 첫 글자는 알파벳
        let Some(first) = name.chars().next() else {
            return false;
        };
        if !first.is_ascii_alphabetic() {
            return false;
        }

        // 나머지는 알파벳, 숫자, 언더스코어
        name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    /// 총 데이터베이스 수
    pub async fn count(&self) -> usize {
        let databases = self.databases.read().await;
        databases.len()
    }

    /// 서버 주소
    pub fn address(&self) -> &str {
        &self.config.address
    }

    /// 데이터베이스 디스크 경로
    pub fn database_path(&self, db_name: &str) -> Option<PathBuf> {
        self.config.database_path(db_name)
    }
}

impl Default for TenantDatabaseManager {
    fn default() -> Self {
        Self::new(TenantDatabaseManagerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_engine_config_propagates_optional_memory_limit() {
        let path = Path::new("/tmp/zeta4j-memory-config-test");
        let disabled = TenantDatabaseManagerConfig::default().graph_engine_config(path);
        assert!(disabled.memory_limit.is_none());

        let mut core = Zeta4jConfig::default();
        core.storage.memory_limit_bytes = Some(8192);
        let enabled = TenantDatabaseManagerConfig::default()
            .with_core_config(core)
            .graph_engine_config(path);
        assert_eq!(
            enabled
                .memory_limit
                .as_ref()
                .map(|config| config.max_memory_bytes),
            Some(8192)
        );
    }

    #[tokio::test]
    async fn restart_discovers_a_logical_database_from_canonical_redb()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let config = TenantDatabaseManagerConfig::default().with_data_path(temp.path());
        let node_id = {
            let manager = TenantDatabaseManager::new_with_storage(config.clone()).await?;
            let database = manager
                .create("alpha", TenantDatabaseConfig::default())
                .await?;
            let engine = database
                .engine
                .as_ref()
                .ok_or_else(|| std::io::Error::other("alpha engine missing"))?;
            engine.create_node(vec!["Entity".to_string()]).await?
        };

        let reopened = TenantDatabaseManager::new_with_storage(config).await?;
        let engine = reopened
            .get_engine("alpha")
            .await
            .ok_or_else(|| std::io::Error::other("reopened alpha engine missing"))?;
        assert_eq!(
            engine.get_node(node_id).await?.map(|node| node.id()),
            Some(node_id)
        );
        assert!(reopened.context_for_engine(&engine).is_some());
        Ok(())
    }
}
