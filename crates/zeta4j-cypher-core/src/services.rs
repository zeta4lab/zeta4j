//! 외부 서비스 트레이트 정의
//!
//! zeta4j-cypher가 독립적으로 컴파일될 수 있도록 외부 서비스의 트레이트를 정의합니다.
//! 실제 구현은 zeta4j-server에서 제공됩니다.
//!
//! ## v1.12.0 비동기 API 통일
//!
//! 모든 서비스 트레이트가 async/await 패턴을 사용합니다.
//! - AdminService: 관리 상태 조회와 변경을 하나의 canonical 경계로 통합

use async_trait::async_trait;

/// Admin service capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdminCapability {
    UserManagement,
    DatabaseManagement,
}

/// Canonical admin service error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminServiceError {
    Storage(String),
    NotFound { entity: &'static str, name: String },
    AlreadyExists { entity: &'static str, name: String },
    Unsupported(String),
    Invariant(String),
    BootstrapExpired,
    BootstrapConsumed,
    InvalidCredential,
    InvalidPassword,
}

impl std::fmt::Display for AdminServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(message) => write!(f, "{}", message),
            Self::NotFound { entity, name } => write!(f, "{} '{}' not found", entity, name),
            Self::AlreadyExists { entity, name } => {
                write!(f, "{} '{}' already exists", entity, name)
            }
            Self::Unsupported(message) => write!(f, "{}", message),
            Self::Invariant(message) => write!(f, "{}", message),
            Self::BootstrapExpired => f.write_str("bootstrap credential expired"),
            Self::BootstrapConsumed => f.write_str("bootstrap credential already consumed"),
            Self::InvalidCredential => f.write_str("invalid bootstrap credential"),
            Self::InvalidPassword => f.write_str("new password does not meet policy"),
        }
    }
}

impl std::error::Error for AdminServiceError {}

pub type AdminServiceResult<T> = Result<T, AdminServiceError>;

/// Cypher admin 명령이 사용하는 단일 canonical service 경계.
#[async_trait]
pub trait AdminService: Send + Sync {
    fn supports(&self, capability: AdminCapability) -> bool;

    async fn list_users(&self) -> AdminServiceResult<Vec<UserInfo>>;
    async fn get_user(&self, username: &str) -> AdminServiceResult<Option<UserInfo>>;
    async fn create_user(
        &self,
        username: &str,
        password: &str,
        password_change_required: bool,
    ) -> AdminServiceResult<()>;
    async fn delete_user(&self, username: &str) -> AdminServiceResult<()>;
    async fn change_password(&self, username: &str, new_password: &str) -> AdminServiceResult<()>;
    async fn complete_bootstrap(
        &self,
        _username: &str,
        _current_password: &str,
        _new_password: &str,
    ) -> AdminServiceResult<()> {
        Err(AdminServiceError::Unsupported(
            "bootstrap completion is unavailable".to_string(),
        ))
    }
    async fn set_suspended(&self, username: &str, suspended: bool) -> AdminServiceResult<()>;
    async fn set_password_change_required(
        &self,
        username: &str,
        required: bool,
    ) -> AdminServiceResult<()>;

    async fn list_databases(&self) -> AdminServiceResult<Vec<DatabaseInfo>>;
    async fn get_database(&self, name: &str) -> AdminServiceResult<Option<DatabaseInfo>>;
    async fn create_database(&self, name: &str) -> AdminServiceResult<()>;
    async fn drop_database(&self, name: &str) -> AdminServiceResult<()>;
    async fn start_database(&self, name: &str) -> AdminServiceResult<()>;
    async fn stop_database(&self, name: &str) -> AdminServiceResult<()>;
}

/// 사용자 정보
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub username: String,
    pub password_change_required: bool,
    pub suspended: bool,
    pub home_database: Option<String>,
}

/// 데이터베이스 정보
#[derive(Debug, Clone)]
pub struct DatabaseInfo {
    pub name: String,
    pub database_type: String,
    pub address: Option<String>,
    pub role: Option<String>,
    pub requested_status: String,
    pub current_status: String,
    pub is_default: bool,
    pub store_size: Option<u64>,
}
