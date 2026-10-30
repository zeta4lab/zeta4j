//! Database Catalog Manager
//!
//! 데이터베이스 카탈로그 관리
//!
//! 기능:
//! - 데이터베이스 등록/조회/삭제
//! - 데이터베이스 상태 관리
//! - 데이터베이스 통계 업데이트

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use zeta4j_graph::engine::StorageType;

use super::error::{SystemError, SystemResult};
use super::storage::{SystemStorage, cf};

/// 데이터베이스 엔트리
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseEntry {
    /// 데이터베이스 UUID
    pub uuid: Uuid,
    /// 데이터베이스 이름
    pub name: String,
    /// 생성 시간
    pub created_at: DateTime<Utc>,
    /// 생성자
    pub created_by: String,
    /// 데이터베이스 상태
    pub state: CatalogDatabaseState,
    /// 스토리지 타입
    pub storage_type: CatalogStorageType,
    /// 데이터 경로
    pub data_path: PathBuf,
    /// 리소스 제한
    pub limits: DatabaseLimits,
    /// 옵션
    pub options: DatabaseOptions,
    /// 기본 데이터베이스 여부
    pub is_default: bool,
    /// 마지막 수정 시간
    pub updated_at: DateTime<Utc>,
}

impl DatabaseEntry {
    /// 새 데이터베이스 엔트리 생성
    pub fn new(name: impl Into<String>, data_path: impl Into<PathBuf>) -> Self {
        let now = Utc::now();
        Self {
            uuid: Uuid::now_v7(),
            name: name.into(),
            created_at: now,
            created_by: "system".to_string(),
            state: CatalogDatabaseState::Creating,
            storage_type: CatalogStorageType::Disk,
            data_path: data_path.into(),
            limits: DatabaseLimits::default(),
            options: DatabaseOptions::default(),
            is_default: false,
            updated_at: now,
        }
    }

    /// 기본 데이터베이스 (zeta4j) 생성
    pub fn default_database(data_path: impl Into<PathBuf>) -> Self {
        let mut entry = Self::new("zeta4j", data_path);
        entry.is_default = true;
        entry.state = CatalogDatabaseState::Online;
        entry.created_by = "system".to_string();
        entry
    }

    /// 온라인 여부
    pub fn is_online(&self) -> bool {
        matches!(self.state, CatalogDatabaseState::Online)
    }
}

/// 데이터베이스 상태
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogDatabaseState {
    /// 생성 중
    Creating,
    /// 시작 중
    Starting,
    /// 온라인
    Online,
    /// 중지 중
    Stopping,
    /// 오프라인
    Offline,
    /// 에러
    Error(String),
    /// 삭제됨
    Dropped,
}

impl std::fmt::Display for CatalogDatabaseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::Starting => write!(f, "starting"),
            Self::Online => write!(f, "online"),
            Self::Stopping => write!(f, "stopping"),
            Self::Offline => write!(f, "offline"),
            Self::Error(msg) => write!(f, "error: {}", msg),
            Self::Dropped => write!(f, "dropped"),
        }
    }
}

/// 스토리지 타입 (카탈로그용)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogStorageType {
    Disk,
}

impl From<StorageType> for CatalogStorageType {
    fn from(st: StorageType) -> Self {
        match st {
            StorageType::Disk => Self::Disk,
        }
    }
}

impl From<CatalogStorageType> for StorageType {
    fn from(cst: CatalogStorageType) -> Self {
        match cst {
            CatalogStorageType::Disk => Self::Disk,
        }
    }
}

/// 데이터베이스 제한
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DatabaseLimits {
    /// 최대 노드 수
    pub max_nodes: Option<u64>,
    /// 최대 엣지 수
    pub max_edges: Option<u64>,
    /// 최대 스토리지 크기 (바이트)
    pub max_storage_bytes: Option<u64>,
    /// 초당 최대 트랜잭션 수
    pub max_transactions_per_sec: Option<u32>,
}

/// 데이터베이스 옵션
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DatabaseOptions {
    /// 읽기 전용
    pub read_only: bool,
}

/// 카탈로그 매니저
#[derive(Clone)]
pub struct CatalogManager {
    storage: Arc<SystemStorage>,
}

impl CatalogManager {
    /// 새 카탈로그 매니저 생성
    pub fn new(storage: Arc<SystemStorage>) -> Self {
        Self { storage }
    }

    /// 키 생성
    fn make_key(name: &str) -> Vec<u8> {
        format!("db:{}", name).into_bytes()
    }

    /// 데이터베이스 등록
    pub fn register(&self, entry: DatabaseEntry) -> SystemResult<()> {
        let key = Self::make_key(&entry.name);

        // 중복 확인
        if self.storage.exists(cf::CATALOG, &key)? {
            return Err(SystemError::DatabaseAlreadyExists(entry.name));
        }

        self.storage.put(cf::CATALOG, &key, &entry)?;
        info!("Registered database: {} ({})", entry.name, entry.uuid);

        Ok(())
    }

    /// 데이터베이스 조회
    pub fn get(&self, name: &str) -> SystemResult<Option<DatabaseEntry>> {
        let key = Self::make_key(name);
        self.storage.get(cf::CATALOG, &key)
    }

    /// 데이터베이스 존재 여부
    pub fn exists(&self, name: &str) -> SystemResult<bool> {
        let key = Self::make_key(name);
        self.storage.exists(cf::CATALOG, &key)
    }

    /// 전체 데이터베이스 목록
    pub fn list(&self) -> SystemResult<Vec<DatabaseEntry>> {
        let entries: Vec<(Vec<u8>, DatabaseEntry)> =
            self.storage.scan_prefix(cf::CATALOG, b"db:")?;
        Ok(entries.into_iter().map(|(_, entry)| entry).collect())
    }

    /// 온라인 데이터베이스 목록
    pub fn list_online(&self) -> SystemResult<Vec<DatabaseEntry>> {
        let entries = self.list()?;
        Ok(entries.into_iter().filter(|e| e.is_online()).collect())
    }

    /// 기본 데이터베이스 조회
    pub fn get_default(&self) -> SystemResult<Option<DatabaseEntry>> {
        let entries = self.list()?;
        Ok(entries.into_iter().find(|e| e.is_default))
    }

    /// 데이터베이스 상태 변경
    pub fn set_state(&self, name: &str, state: CatalogDatabaseState) -> SystemResult<()> {
        let key = Self::make_key(name);

        let mut entry: DatabaseEntry = self
            .storage
            .get(cf::CATALOG, &key)?
            .ok_or_else(|| SystemError::DatabaseNotFound(name.to_string()))?;

        entry.state = state;
        entry.updated_at = Utc::now();

        self.storage.put(cf::CATALOG, &key, &entry)?;
        info!("Database '{}' state changed to: {}", name, entry.state);

        Ok(())
    }

    /// 데이터베이스 업데이트
    pub fn update(&self, entry: DatabaseEntry) -> SystemResult<()> {
        let key = Self::make_key(&entry.name);

        if !self.storage.exists(cf::CATALOG, &key)? {
            return Err(SystemError::DatabaseNotFound(entry.name));
        }

        self.storage.put(cf::CATALOG, &key, &entry)?;
        Ok(())
    }

    /// 데이터베이스 삭제 (카탈로그에서만)
    pub fn unregister(&self, name: &str) -> SystemResult<()> {
        let key = Self::make_key(name);

        // 기본 데이터베이스는 삭제 불가
        if let Some(entry) = self.get(name)? {
            if entry.is_default {
                return Err(SystemError::CannotDeleteDefault);
            }
        } else {
            return Err(SystemError::DatabaseNotFound(name.to_string()));
        }

        self.storage.delete(cf::CATALOG, &key)?;
        info!("Unregistered database: {}", name);

        Ok(())
    }

    /// 기본 데이터베이스 초기화 (없으면 생성)
    pub fn ensure_default_database(
        &self,
        data_path: impl Into<PathBuf>,
    ) -> SystemResult<DatabaseEntry> {
        if let Some(entry) = self.get("zeta4j")? {
            return Ok(entry);
        }

        let entry = DatabaseEntry::default_database(data_path);
        self.register(entry.clone())?;
        Ok(entry)
    }

    /// 데이터베이스 수
    pub fn count(&self) -> SystemResult<usize> {
        Ok(self.list()?.len())
    }
}
