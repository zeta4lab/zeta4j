//! System Database Service
//!
//! 시스템 메타데이터 관리를 위한 통합 서비스
//!
//! 구성요소:
//! - CatalogManager: 데이터베이스 카탈로그
//! - SecurityManager: 사용자 인증
//! - SchemaManager: 인덱스/제약조건
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::server::system::SystemDatabaseService;
//!
//! let system_db = SystemDatabaseService::open("/data/.zeta4j")?;
//!
//! // 카탈로그 조회
//! let databases = system_db.catalog().list()?;
//!
//! // 사용자 인증
//! let user = system_db.security().authenticate("admin", "password")?;
//!
//! // 인덱스 조회
//! let indexes = system_db.schema().list_indexes("zeta4j")?;
//! ```

use std::path::Path;
use std::sync::Arc;

use tracing::info;

use super::catalog::CatalogManager;
use super::error::SystemResult;
use super::schema::SchemaManager;
use super::security::SecurityManager;
use super::storage::SystemStorage;

/// 시스템 데이터베이스 서비스
///
/// 모든 시스템 메타데이터 관리를 위한 통합 서비스
pub struct SystemDatabaseService {
    /// 데이터베이스 카탈로그 관리자
    catalog: CatalogManager,
    /// 보안 관리자 (사용자 인증) - Arc로 래핑하여 트레이트 반환 지원
    security: Arc<SecurityManager>,
    /// 스키마 관리자 (인덱스/제약조건)
    schema: SchemaManager,
    /// 스토리지
    storage: Arc<SystemStorage>,
    /// 모든 프로토콜 표면의 database creation 검증과 등록을 직렬화한다.
    database_creation_lock: tokio::sync::Mutex<()>,
}

impl SystemDatabaseService {
    /// 시스템 데이터베이스 열기
    ///
    /// `base_path`는 데이터 디렉토리 (예: ~/.zeta4j/databases)
    /// 시스템 DB는 `base_path/system`에 저장됨
    pub fn open(base_path: impl AsRef<Path>) -> SystemResult<Self> {
        let base_path = base_path.as_ref();
        let system_path = base_path.join("system");

        info!("Opening system database at: {}", system_path.display());

        std::fs::create_dir_all(&system_path).map_err(|error| {
            super::SystemError::storage(format!("system directory rejected: {error}"))
        })?;

        let storage = Arc::new(SystemStorage::open(&system_path)?);
        let catalog = CatalogManager::new(storage.clone());
        let security = Arc::new(SecurityManager::try_new(storage.clone())?);
        let schema = SchemaManager::new(storage.clone());

        let service = Self {
            catalog,
            security,
            schema,
            storage,
            database_creation_lock: tokio::sync::Mutex::new(()),
        };

        // 기본 데이터베이스 등록 (zeta4j)
        service
            .catalog
            .ensure_default_database(base_path.join("zeta4j"))?;

        info!("System database opened successfully");
        Ok(service)
    }

    /// 카탈로그 관리자 접근
    pub fn catalog(&self) -> &CatalogManager {
        &self.catalog
    }

    /// 보안 관리자 접근
    pub fn security(&self) -> &SecurityManager {
        &self.security
    }

    /// 보안 관리자 Arc 접근
    pub fn security_arc(&self) -> Arc<SecurityManager> {
        self.security.clone()
    }

    /// 스키마 관리자 접근
    pub fn schema(&self) -> &SchemaManager {
        &self.schema
    }

    /// 스토리지 접근
    pub fn storage(&self) -> &Arc<SystemStorage> {
        &self.storage
    }

    pub async fn lock_database_creation(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.database_creation_lock.lock().await
    }

    /// 시스템 정보 요약
    pub fn summary(&self) -> SystemResult<SystemSummary> {
        let databases = self.catalog.count()?;
        let users = self.security.list_users()?.len();
        let indexes = self.schema.list_all_indexes()?.len();
        let constraints = self.schema.list_all_constraints()?.len();

        Ok(SystemSummary {
            databases,
            users,
            indexes,
            constraints,
        })
    }
}

/// 시스템 요약 정보
#[derive(Debug, Clone)]
pub struct SystemSummary {
    pub databases: usize,
    pub users: usize,
    pub indexes: usize,
    pub constraints: usize,
}

/// Arc로 감싼 시스템 데이터베이스 서비스
pub type SharedSystemDatabaseService = Arc<SystemDatabaseService>;
