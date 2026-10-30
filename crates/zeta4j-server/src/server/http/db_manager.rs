//! Database Manager (M2.3)
//!
//! 다중 데이터베이스 관리
//! 데이터베이스 생성, 삭제, 시작, 중지

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

use zeta4j_common::error::ZetaError;
use zeta4j_graph::engine::GraphEngine;

use super::admin_types::{DatabaseInfo, DatabaseStatus};

/// 데이터베이스 관리 에러
#[derive(Debug, Error)]
pub enum DbManagerError {
    #[error("Database not found: {0}")]
    DatabaseNotFound(String),

    #[error("Database already exists: {0}")]
    DatabaseAlreadyExists(String),

    #[error("Cannot delete system database")]
    CannotDeleteSystem,

    #[error("Cannot delete default database")]
    CannotDeleteDefault,

    #[error("Database is already {0}")]
    InvalidState(String),

    #[error("Invalid database name: {0}")]
    InvalidName(String),

    #[error("Engine error: {0}")]
    EngineError(String),
}

/// DbManagerError → ZetaError 변환 (Phase 5)
impl From<DbManagerError> for ZetaError {
    fn from(err: DbManagerError) -> Self {
        match err {
            DbManagerError::DatabaseNotFound(name) => {
                ZetaError::not_found(format!("Database not found: {}", name))
            }
            DbManagerError::DatabaseAlreadyExists(name) => {
                ZetaError::invalid_argument(format!("Database already exists: {}", name))
            }
            DbManagerError::CannotDeleteSystem => {
                ZetaError::invalid_argument("Cannot delete system database")
            }
            DbManagerError::CannotDeleteDefault => {
                ZetaError::invalid_argument("Cannot delete default database")
            }
            DbManagerError::InvalidState(state) => {
                ZetaError::invalid_argument(format!("Database is already {}", state))
            }
            DbManagerError::InvalidName(name) => {
                ZetaError::invalid_argument(format!("Invalid database name: {}", name))
            }
            DbManagerError::EngineError(msg) => ZetaError::internal(msg),
        }
    }
}

/// 데이터베이스 인스턴스
pub struct DatabaseInstance {
    pub name: String,
    pub status: DatabaseStatus,
    pub engine: Option<Arc<GraphEngine>>,
    pub created_at: DateTime<Utc>,
    pub last_start_time: Option<DateTime<Utc>>,
    pub last_stop_time: Option<DateTime<Utc>>,
    pub config: DatabaseConfig,
    pub stats: DatabaseStats,
    pub is_default: bool,
    pub is_system: bool,
}

impl DatabaseInstance {
    /// 새 데이터베이스 인스턴스 생성
    pub fn new(name: String, config: DatabaseConfig) -> Self {
        Self {
            name,
            status: DatabaseStatus::Offline,
            engine: None,
            created_at: Utc::now(),
            last_start_time: None,
            last_stop_time: None,
            config,
            stats: DatabaseStats::default(),
            is_default: false,
            is_system: false,
        }
    }

    /// 기본 데이터베이스 생성
    pub fn default_database() -> Self {
        let mut db = Self::new("zeta4j".to_string(), DatabaseConfig::default());
        db.is_default = true;
        db.status = DatabaseStatus::Online;
        db.last_start_time = Some(Utc::now());
        db
    }

    /// 시스템 데이터베이스 생성
    pub fn system_database() -> Self {
        let mut db = Self::new("system".to_string(), DatabaseConfig::default());
        db.is_system = true;
        db.status = DatabaseStatus::Online;
        db.last_start_time = Some(Utc::now());
        db
    }

    /// DatabaseInfo로 변환
    pub fn to_info(&self, address: &str) -> DatabaseInfo {
        DatabaseInfo {
            name: self.name.clone(),
            address: address.to_string(),
            role: "primary".to_string(),
            status: self.status.to_string(),
            requested_status: self.status.to_string(),
            default: self.is_default,
            home: self.is_default,
            current_primaries_count: 1,
            current_secondaries_count: 0,
            creation_time: self.created_at.to_rfc3339(),
            last_start_time: self.last_start_time.map(|t| t.to_rfc3339()),
            last_stop_time: self.last_stop_time.map(|t| t.to_rfc3339()),
            store_size: self.stats.store_size,
            tx_count: self.stats.tx_count,
        }
    }
}

/// 데이터베이스 설정
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DatabaseConfig {
    pub max_size: Option<u64>,
    pub read_only: bool,
}

/// 데이터베이스 통계
#[derive(Debug, Clone, Default)]
pub struct DatabaseStats {
    pub node_count: u64,
    pub relationship_count: u64,
    pub store_size: u64,
    pub tx_count: u64,
}

/// 데이터베이스 관리자 설정
#[derive(Debug, Clone)]
pub struct DatabaseManagerConfig {
    pub default_database: String,
    pub address: String,
    pub max_databases: usize,
}

impl Default for DatabaseManagerConfig {
    fn default() -> Self {
        Self {
            default_database: "zeta4j".to_string(),
            address: "localhost:7687".to_string(),
            max_databases: 100,
        }
    }
}

/// 데이터베이스 관리자
pub struct DatabaseManager {
    databases: RwLock<HashMap<String, DatabaseInstance>>,
    config: DatabaseManagerConfig,
}

impl DatabaseManager {
    /// 새 데이터베이스 관리자 생성
    pub fn new(config: DatabaseManagerConfig) -> Self {
        let mut databases = HashMap::new();

        // 기본 데이터베이스 생성
        let default_db = DatabaseInstance::default_database();
        databases.insert(default_db.name.clone(), default_db);

        // 시스템 데이터베이스 생성
        let system_db = DatabaseInstance::system_database();
        databases.insert(system_db.name.clone(), system_db);

        Self {
            databases: RwLock::new(databases),
            config,
        }
    }

    /// 기존 엔진으로 데이터베이스 관리자 생성
    pub fn with_engine(config: DatabaseManagerConfig, engine: Arc<GraphEngine>) -> Self {
        let mut databases = HashMap::new();

        // 기본 데이터베이스 생성 (엔진 포함)
        let mut default_db = DatabaseInstance::default_database();
        default_db.engine = Some(engine);
        databases.insert(default_db.name.clone(), default_db);

        // 시스템 데이터베이스 생성
        let system_db = DatabaseInstance::system_database();
        databases.insert(system_db.name.clone(), system_db);

        Self {
            databases: RwLock::new(databases),
            config,
        }
    }

    /// 데이터베이스 목록 조회
    pub async fn list(&self) -> Vec<DatabaseInfo> {
        let databases = self.databases.read().await;
        databases
            .values()
            .map(|db| db.to_info(&self.config.address))
            .collect()
    }

    /// 데이터베이스 조회
    pub async fn get(&self, name: &str) -> Result<DatabaseInfo, DbManagerError> {
        let databases = self.databases.read().await;
        databases
            .get(name)
            .map(|db| db.to_info(&self.config.address))
            .ok_or_else(|| DbManagerError::DatabaseNotFound(name.to_string()))
    }

    /// 데이터베이스 생성
    pub async fn create(&self, name: &str, config: DatabaseConfig) -> Result<(), DbManagerError> {
        // 이름 검증
        if !Self::is_valid_name(name) {
            return Err(DbManagerError::InvalidName(name.to_string()));
        }

        let mut databases = self.databases.write().await;

        // 이미 존재하는지 확인
        if databases.contains_key(name) {
            return Err(DbManagerError::DatabaseAlreadyExists(name.to_string()));
        }

        // 최대 개수 확인
        if databases.len() >= self.config.max_databases {
            return Err(DbManagerError::EngineError(
                "Maximum number of databases reached".to_string(),
            ));
        }

        // 새 데이터베이스 생성
        let db = DatabaseInstance::new(name.to_string(), config);
        databases.insert(name.to_string(), db);

        Ok(())
    }

    /// 데이터베이스 삭제
    pub async fn delete(&self, name: &str) -> Result<(), DbManagerError> {
        let mut databases = self.databases.write().await;

        // 시스템 DB 삭제 불가
        if name == "system" {
            return Err(DbManagerError::CannotDeleteSystem);
        }

        // 기본 DB 삭제 불가
        if name == self.config.default_database {
            return Err(DbManagerError::CannotDeleteDefault);
        }

        if databases.remove(name).is_none() {
            return Err(DbManagerError::DatabaseNotFound(name.to_string()));
        }

        Ok(())
    }

    /// 데이터베이스 시작
    pub async fn start(&self, name: &str) -> Result<(), DbManagerError> {
        let mut databases = self.databases.write().await;

        let db = databases
            .get_mut(name)
            .ok_or_else(|| DbManagerError::DatabaseNotFound(name.to_string()))?;

        if db.status == DatabaseStatus::Online {
            return Err(DbManagerError::InvalidState("online".to_string()));
        }

        db.status = DatabaseStatus::Starting;
        // 실제로는 여기서 엔진 초기화 수행
        db.status = DatabaseStatus::Online;
        db.last_start_time = Some(Utc::now());

        Ok(())
    }

    /// 데이터베이스 중지
    pub async fn stop(&self, name: &str) -> Result<(), DbManagerError> {
        let mut databases = self.databases.write().await;

        let db = databases
            .get_mut(name)
            .ok_or_else(|| DbManagerError::DatabaseNotFound(name.to_string()))?;

        if db.status == DatabaseStatus::Offline {
            return Err(DbManagerError::InvalidState("offline".to_string()));
        }

        db.status = DatabaseStatus::Stopping;
        // 실제로는 여기서 엔진 종료 수행
        db.status = DatabaseStatus::Offline;
        db.last_stop_time = Some(Utc::now());

        Ok(())
    }

    /// 기본 데이터베이스 반환
    pub async fn default_database(&self) -> Option<DatabaseInfo> {
        self.get(&self.config.default_database).await.ok()
    }

    /// 데이터베이스 엔진 가져오기
    pub async fn get_engine(&self, name: &str) -> Option<Arc<GraphEngine>> {
        let databases = self.databases.read().await;
        databases.get(name).and_then(|db| db.engine.clone())
    }

    /// 기본 데이터베이스 엔진 가져오기
    pub async fn default_engine(&self) -> Option<Arc<GraphEngine>> {
        self.get_engine(&self.config.default_database).await
    }

    /// 데이터베이스 통계 업데이트
    pub async fn update_stats(
        &self,
        name: &str,
        stats: DatabaseStats,
    ) -> Result<(), DbManagerError> {
        let mut databases = self.databases.write().await;

        let db = databases
            .get_mut(name)
            .ok_or_else(|| DbManagerError::DatabaseNotFound(name.to_string()))?;

        db.stats = stats;
        Ok(())
    }

    /// 데이터베이스 이름 유효성 검사
    fn is_valid_name(name: &str) -> bool {
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

    /// 데이터베이스 존재 여부
    pub async fn exists(&self, name: &str) -> bool {
        let databases = self.databases.read().await;
        databases.contains_key(name)
    }

    /// 데이터베이스 상태 확인
    pub async fn is_online(&self, name: &str) -> bool {
        let databases = self.databases.read().await;
        databases
            .get(name)
            .map(|db| db.status == DatabaseStatus::Online)
            .unwrap_or(false)
    }
}

impl Default for DatabaseManager {
    fn default() -> Self {
        Self::new(DatabaseManagerConfig::default())
    }
}

/// Arc로 감싼 DB 관리자
pub type SharedDatabaseManager = Arc<DatabaseManager>;
