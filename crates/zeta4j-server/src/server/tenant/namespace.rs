// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Namespace Module (M6.3)
//!
//! 네임스페이스 및 테넌트 관리

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::database::DatabaseId;
use super::resource::ResourceLimits;

// ============================================================================
// Tenant ID
// ============================================================================

/// 테넌트 고유 식별자
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(Uuid);

impl TenantId {
    /// 새 ID 생성
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// UUID에서 생성
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// 시스템 테넌트 ID (고정)
    pub fn system() -> Self {
        Self(Uuid::nil())
    }

    /// 내부 UUID 반환
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// 시스템 테넌트 여부
    pub fn is_system(&self) -> bool {
        self.0.is_nil()
    }
}

impl Default for TenantId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// Tenant Status
// ============================================================================

/// 테넌트 상태
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TenantStatus {
    /// 활성
    #[default]
    Active,
    /// 일시 중지
    Suspended,
    /// 삭제 대기
    PendingDeletion,
}

impl std::fmt::Display for TenantStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TenantStatus::Active => write!(f, "active"),
            TenantStatus::Suspended => write!(f, "suspended"),
            TenantStatus::PendingDeletion => write!(f, "pending_deletion"),
        }
    }
}

// ============================================================================
// Isolation Level
// ============================================================================

/// 격리 레벨
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IsolationLevel {
    /// 완전 격리 (크로스 DB 쿼리 불가)
    Full,
    /// 테넌트 내 공유 (같은 테넌트 내 크로스 DB 가능)
    #[default]
    TenantShared,
    /// 읽기 공유 (다른 테넌트 DB 읽기 가능)
    ReadShared,
}

impl std::fmt::Display for IsolationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IsolationLevel::Full => write!(f, "full"),
            IsolationLevel::TenantShared => write!(f, "tenant_shared"),
            IsolationLevel::ReadShared => write!(f, "read_shared"),
        }
    }
}

// ============================================================================
// Tenant Config
// ============================================================================

/// 테넌트 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantConfig {
    /// 최대 데이터베이스 수
    pub max_databases: u32,
    /// 격리 레벨
    pub isolation_level: IsolationLevel,
    /// 네트워크 격리
    pub network_isolation: bool,
    /// 기본 리소스 제한
    pub default_resource_limits: ResourceLimits,
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            max_databases: 10,
            isolation_level: IsolationLevel::TenantShared,
            network_isolation: false,
            default_resource_limits: ResourceLimits::default(),
        }
    }
}

/// 테넌트 설정 빌더
#[derive(Debug, Default)]
pub struct TenantConfigBuilder {
    config: TenantConfig,
}

impl TenantConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_databases(mut self, max: u32) -> Self {
        self.config.max_databases = max;
        self
    }

    pub fn isolation_level(mut self, level: IsolationLevel) -> Self {
        self.config.isolation_level = level;
        self
    }

    pub fn network_isolation(mut self, enabled: bool) -> Self {
        self.config.network_isolation = enabled;
        self
    }

    pub fn default_resource_limits(mut self, limits: ResourceLimits) -> Self {
        self.config.default_resource_limits = limits;
        self
    }

    pub fn build(self) -> TenantConfig {
        self.config
    }
}

// ============================================================================
// Tenant
// ============================================================================

/// 테넌트
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    /// 테넌트 ID
    pub id: TenantId,
    /// 테넌트 이름
    pub name: String,
    /// 소유 데이터베이스
    pub databases: Vec<DatabaseId>,
    /// 테넌트 설정
    pub config: TenantConfig,
    /// 생성 시간
    pub created_at: DateTime<Utc>,
    /// 상태
    pub status: TenantStatus,
    /// 메타데이터
    pub metadata: HashMap<String, String>,
}

impl Tenant {
    /// 새 테넌트 생성
    pub fn new(name: impl Into<String>, config: TenantConfig) -> Self {
        Self {
            id: TenantId::new(),
            name: name.into(),
            databases: Vec::new(),
            config,
            created_at: Utc::now(),
            status: TenantStatus::Active,
            metadata: HashMap::new(),
        }
    }

    /// 시스템 테넌트 생성
    pub fn system() -> Self {
        Self {
            id: TenantId::system(),
            name: "system".to_string(),
            databases: Vec::new(),
            config: TenantConfig {
                max_databases: u32::MAX,
                isolation_level: IsolationLevel::Full,
                network_isolation: false,
                default_resource_limits: ResourceLimits::default(),
            },
            created_at: Utc::now(),
            status: TenantStatus::Active,
            metadata: HashMap::new(),
        }
    }

    /// 데이터베이스 추가
    pub fn add_database(&mut self, db_id: DatabaseId) -> bool {
        if self.databases.len() >= self.config.max_databases as usize {
            return false;
        }
        if !self.databases.contains(&db_id) {
            self.databases.push(db_id);
        }
        true
    }

    /// 데이터베이스 제거
    pub fn remove_database(&mut self, db_id: &DatabaseId) -> bool {
        let len = self.databases.len();
        self.databases.retain(|id| id != db_id);
        self.databases.len() < len
    }

    /// 데이터베이스 소유 여부
    pub fn owns_database(&self, db_id: &DatabaseId) -> bool {
        self.databases.contains(db_id)
    }

    /// 활성 상태 여부
    pub fn is_active(&self) -> bool {
        matches!(self.status, TenantStatus::Active)
    }

    /// 시스템 테넌트 여부
    pub fn is_system(&self) -> bool {
        self.id.is_system()
    }
}

// ============================================================================
// Namespace
// ============================================================================

/// 네임스페이스
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    /// 네임스페이스 이름
    pub name: String,
    /// 소속 테넌트
    pub tenant_id: TenantId,
    /// 포함 데이터베이스 (이름 → ID)
    pub databases: HashMap<String, DatabaseId>,
    /// 앨리어스 (앨리어스 → 실제 DB 이름)
    pub aliases: HashMap<String, String>,
    /// 생성 시간
    pub created_at: DateTime<Utc>,
}

impl Namespace {
    /// 새 네임스페이스 생성
    pub fn new(name: impl Into<String>, tenant_id: TenantId) -> Self {
        Self {
            name: name.into(),
            tenant_id,
            databases: HashMap::new(),
            aliases: HashMap::new(),
            created_at: Utc::now(),
        }
    }

    /// 데이터베이스 추가
    pub fn add_database(&mut self, name: impl Into<String>, db_id: DatabaseId) {
        self.databases.insert(name.into(), db_id);
    }

    /// 데이터베이스 제거
    pub fn remove_database(&mut self, name: &str) -> Option<DatabaseId> {
        // 관련 앨리어스도 제거
        self.aliases.retain(|_, target| target != name);
        self.databases.remove(name)
    }

    /// 앨리어스 설정
    pub fn set_alias(&mut self, alias: impl Into<String>, db_name: impl Into<String>) -> bool {
        let db_name = db_name.into();
        if !self.databases.contains_key(&db_name) {
            return false;
        }
        self.aliases.insert(alias.into(), db_name);
        true
    }

    /// 앨리어스 제거
    pub fn remove_alias(&mut self, alias: &str) -> bool {
        self.aliases.remove(alias).is_some()
    }

    /// 이름으로 데이터베이스 ID 조회 (앨리어스 해석 포함)
    pub fn resolve(&self, name: &str) -> Option<DatabaseId> {
        // 먼저 직접 이름으로 찾기
        if let Some(id) = self.databases.get(name) {
            return Some(*id);
        }

        // 앨리어스로 찾기
        if let Some(real_name) = self.aliases.get(name) {
            return self.databases.get(real_name).copied();
        }

        None
    }

    /// 모든 데이터베이스 이름 (앨리어스 포함)
    pub fn all_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.databases.keys().map(|s| s.as_str()).collect();
        names.extend(self.aliases.keys().map(|s| s.as_str()));
        names
    }
}

// ============================================================================
// Tenant Error
// ============================================================================

/// 테넌트 관련 에러
#[derive(Debug, Error)]
pub enum TenantError {
    #[error("Tenant not found: {0}")]
    TenantNotFound(String),

    #[error("Tenant already exists: {0}")]
    TenantAlreadyExists(String),

    #[error("Cannot delete system tenant")]
    CannotDeleteSystem,

    #[error("Tenant is suspended: {0}")]
    TenantSuspended(String),

    #[error("Namespace not found: {0}")]
    NamespaceNotFound(String),

    #[error("Namespace already exists: {0}")]
    NamespaceAlreadyExists(String),

    #[error("Database limit reached for tenant: {0}")]
    DatabaseLimitReached(String),

    #[error("Cross-database access denied: {0}")]
    CrossDbAccessDenied(String),

    #[error("Invalid tenant name: {0}")]
    InvalidName(String),

    #[error("Operation not permitted: {0}")]
    NotPermitted(String),
}

// ============================================================================
// Namespace Manager
// ============================================================================

/// 네임스페이스 관리자
pub struct NamespaceManager {
    /// 테넌트 맵 (ID → 테넌트)
    tenants: RwLock<HashMap<TenantId, Tenant>>,
    /// 테넌트 이름 → ID 매핑
    tenant_names: RwLock<HashMap<String, TenantId>>,
    /// 네임스페이스 맵 (이름 → 네임스페이스)
    namespaces: RwLock<HashMap<String, Namespace>>,
}

impl NamespaceManager {
    /// 새 관리자 생성
    pub fn new() -> Self {
        let mut tenants = HashMap::new();
        let mut tenant_names = HashMap::new();

        // 시스템 테넌트 생성
        let system_tenant = Tenant::system();
        tenant_names.insert(system_tenant.name.clone(), system_tenant.id);
        tenants.insert(system_tenant.id, system_tenant);

        Self {
            tenants: RwLock::new(tenants),
            tenant_names: RwLock::new(tenant_names),
            namespaces: RwLock::new(HashMap::new()),
        }
    }

    // ========================================================================
    // Tenant Management
    // ========================================================================

    /// 테넌트 생성
    pub async fn create_tenant(
        &self,
        name: &str,
        config: TenantConfig,
    ) -> Result<TenantId, TenantError> {
        // 이름 검증
        if !Self::is_valid_name(name) {
            return Err(TenantError::InvalidName(name.to_string()));
        }

        let mut tenants = self.tenants.write().await;
        let mut tenant_names = self.tenant_names.write().await;

        // 이미 존재하는지 확인
        if tenant_names.contains_key(name) {
            return Err(TenantError::TenantAlreadyExists(name.to_string()));
        }

        let tenant = Tenant::new(name, config);
        let tenant_id = tenant.id;

        tenant_names.insert(tenant.name.clone(), tenant_id);
        tenants.insert(tenant_id, tenant);

        Ok(tenant_id)
    }

    /// 테넌트 삭제
    pub async fn delete_tenant(&self, tenant_id: TenantId) -> Result<(), TenantError> {
        if tenant_id.is_system() {
            return Err(TenantError::CannotDeleteSystem);
        }

        let mut tenants = self.tenants.write().await;
        let mut tenant_names = self.tenant_names.write().await;

        let tenant = tenants
            .remove(&tenant_id)
            .ok_or_else(|| TenantError::TenantNotFound(tenant_id.to_string()))?;

        tenant_names.remove(&tenant.name);

        // 관련 네임스페이스 삭제
        let mut namespaces = self.namespaces.write().await;
        namespaces.retain(|_, ns| ns.tenant_id != tenant_id);

        Ok(())
    }

    /// 테넌트 조회
    pub async fn get_tenant(&self, tenant_id: TenantId) -> Option<Tenant> {
        let tenants = self.tenants.read().await;
        tenants.get(&tenant_id).cloned()
    }

    /// 이름으로 테넌트 조회
    pub async fn get_tenant_by_name(&self, name: &str) -> Option<Tenant> {
        let tenant_names = self.tenant_names.read().await;
        let tenant_id = tenant_names.get(name)?;

        let tenants = self.tenants.read().await;
        tenants.get(tenant_id).cloned()
    }

    /// 테넌트 목록
    pub async fn list_tenants(&self) -> Vec<Tenant> {
        let tenants = self.tenants.read().await;
        tenants.values().cloned().collect()
    }

    /// 테넌트 상태 변경
    pub async fn set_tenant_status(
        &self,
        tenant_id: TenantId,
        status: TenantStatus,
    ) -> Result<(), TenantError> {
        if tenant_id.is_system() && !matches!(status, TenantStatus::Active) {
            return Err(TenantError::NotPermitted(
                "Cannot change system tenant status".to_string(),
            ));
        }

        let mut tenants = self.tenants.write().await;
        let tenant = tenants
            .get_mut(&tenant_id)
            .ok_or_else(|| TenantError::TenantNotFound(tenant_id.to_string()))?;

        tenant.status = status;
        Ok(())
    }

    /// 테넌트에 데이터베이스 추가
    pub async fn add_database_to_tenant(
        &self,
        tenant_id: TenantId,
        db_id: DatabaseId,
    ) -> Result<(), TenantError> {
        let mut tenants = self.tenants.write().await;
        let tenant = tenants
            .get_mut(&tenant_id)
            .ok_or_else(|| TenantError::TenantNotFound(tenant_id.to_string()))?;

        if !tenant.add_database(db_id) {
            return Err(TenantError::DatabaseLimitReached(tenant.name.clone()));
        }

        Ok(())
    }

    /// 테넌트에서 데이터베이스 제거
    pub async fn remove_database_from_tenant(
        &self,
        tenant_id: TenantId,
        db_id: &DatabaseId,
    ) -> Result<(), TenantError> {
        let mut tenants = self.tenants.write().await;
        let tenant = tenants
            .get_mut(&tenant_id)
            .ok_or_else(|| TenantError::TenantNotFound(tenant_id.to_string()))?;

        tenant.remove_database(db_id);
        Ok(())
    }

    // ========================================================================
    // Namespace Management
    // ========================================================================

    /// 네임스페이스 생성
    pub async fn create_namespace(
        &self,
        tenant_id: TenantId,
        name: &str,
    ) -> Result<(), TenantError> {
        // 이름 검증
        if !Self::is_valid_name(name) {
            return Err(TenantError::InvalidName(name.to_string()));
        }

        // 테넌트 존재 확인
        {
            let tenants = self.tenants.read().await;
            if !tenants.contains_key(&tenant_id) {
                return Err(TenantError::TenantNotFound(tenant_id.to_string()));
            }
        }

        let mut namespaces = self.namespaces.write().await;

        if namespaces.contains_key(name) {
            return Err(TenantError::NamespaceAlreadyExists(name.to_string()));
        }

        let ns = Namespace::new(name, tenant_id);
        namespaces.insert(name.to_string(), ns);

        Ok(())
    }

    /// 네임스페이스 삭제
    pub async fn delete_namespace(&self, name: &str) -> Result<(), TenantError> {
        let mut namespaces = self.namespaces.write().await;

        if namespaces.remove(name).is_none() {
            return Err(TenantError::NamespaceNotFound(name.to_string()));
        }

        Ok(())
    }

    /// 네임스페이스 조회
    pub async fn get_namespace(&self, name: &str) -> Option<Namespace> {
        let namespaces = self.namespaces.read().await;
        namespaces.get(name).cloned()
    }

    /// 테넌트의 네임스페이스 목록
    pub async fn list_namespaces(&self, tenant_id: TenantId) -> Vec<Namespace> {
        let namespaces = self.namespaces.read().await;
        namespaces
            .values()
            .filter(|ns| ns.tenant_id == tenant_id)
            .cloned()
            .collect()
    }

    /// 네임스페이스에 데이터베이스 추가
    pub async fn add_database_to_namespace(
        &self,
        namespace: &str,
        db_name: &str,
        db_id: DatabaseId,
    ) -> Result<(), TenantError> {
        let mut namespaces = self.namespaces.write().await;

        let ns = namespaces
            .get_mut(namespace)
            .ok_or_else(|| TenantError::NamespaceNotFound(namespace.to_string()))?;

        ns.add_database(db_name, db_id);
        Ok(())
    }

    /// 앨리어스 설정
    pub async fn set_alias(
        &self,
        namespace: &str,
        alias: &str,
        db_name: &str,
    ) -> Result<(), TenantError> {
        let mut namespaces = self.namespaces.write().await;

        let ns = namespaces
            .get_mut(namespace)
            .ok_or_else(|| TenantError::NamespaceNotFound(namespace.to_string()))?;

        if !ns.set_alias(alias, db_name) {
            return Err(TenantError::NotPermitted(format!(
                "Database '{}' not found in namespace",
                db_name
            )));
        }

        Ok(())
    }

    // ========================================================================
    // Access Control
    // ========================================================================

    /// 크로스 DB 접근 검사
    pub async fn can_access_cross_db(&self, from_tenant: TenantId, to_db: DatabaseId) -> bool {
        let tenants = self.tenants.read().await;

        let from = match tenants.get(&from_tenant) {
            Some(t) => t,
            None => return false,
        };

        // 시스템 테넌트는 모든 접근 가능
        if from.is_system() {
            return true;
        }

        // 같은 테넌트의 DB인 경우
        if from.owns_database(&to_db) {
            return true;
        }

        // 격리 레벨에 따른 접근 제어
        match from.config.isolation_level {
            IsolationLevel::Full => false,
            IsolationLevel::TenantShared => {
                // 같은 테넌트만 허용 (이미 위에서 체크됨)
                false
            }
            IsolationLevel::ReadShared => {
                // 읽기 공유는 일단 허용 (실제 쿼리에서 읽기만 허용)
                true
            }
        }
    }

    /// 이름 유효성 검사
    pub fn is_valid_name(name: &str) -> bool {
        if name.is_empty() || name.len() > 63 {
            return false;
        }

        let Some(first) = name.chars().next() else {
            return false;
        };
        if !first.is_ascii_alphabetic() {
            return false;
        }

        name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    }
}

impl Default for NamespaceManager {
    fn default() -> Self {
        Self::new()
    }
}
