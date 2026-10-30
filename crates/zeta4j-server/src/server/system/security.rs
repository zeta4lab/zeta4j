//! Security Manager
//!
//! 사용자 인증 관리
//!
//! 기능:
//! - 사용자 관리 (생성, 수정, 삭제)
//! - 인증 (비밀번호 검증)
//!
//! 동시성 안전:
//! - 사용자별 락을 사용하여 Check-Then-Act race condition 방지
//! - Read-Modify-Write 원자성 보장

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::info;
use zeta4j_credential::{
    AuthSession, CredentialStatus, CredentialSubjectId, Principal, PrincipalKind, PrincipalStatus,
    RefreshFamily, SessionStatus, StableId, close_session,
};

use crate::server::auth::password::{hash_password, verify_password, verify_password_outcome};

use super::error::{SystemError, SystemResult};
use super::storage::{SystemStorage, SystemWriteTransaction, cf};

// ============================================================================
// User
// ============================================================================

/// 사용자 정보
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// Immutable persisted credential principal identifier.
    pub principal_id: CredentialSubjectId,
    /// 사용자 이름
    pub username: String,
    /// 비밀번호 해시 (Argon2)
    pub password_hash: String,
    /// 생성 시간
    pub created_at: DateTime<Utc>,
    /// 수정 시간
    pub updated_at: DateTime<Utc>,
    /// 비밀번호 변경 필요
    pub password_change_required: bool,
    /// Initial bootstrap credential expiry.
    #[serde(default)]
    pub bootstrap_expires_at: Option<DateTime<Utc>>,
    /// Durable bootstrap generation. Zero means a normal user.
    #[serde(default)]
    pub bootstrap_generation: u64,
    /// 계정 정지
    pub suspended: bool,
    /// 홈 데이터베이스
    pub home_database: Option<String>,
    /// Incremented whenever authentication or authorization state changes.
    pub security_generation: u64,
}

impl User {
    /// 새 사용자 생성
    pub fn new(username: impl Into<String>, password_hash: String) -> Self {
        let now = Utc::now();
        Self {
            principal_id: CredentialSubjectId::new(),
            username: username.into(),
            password_hash,
            created_at: now,
            updated_at: now,
            password_change_required: false,
            bootstrap_expires_at: None,
            bootstrap_generation: 0,
            suspended: false,
            home_database: None,
            security_generation: 1,
        }
    }
}

// ============================================================================

/// 보안 관리자
///
/// 동시성 안전: 사용자별 락을 사용하여 race condition 방지
pub struct SecurityManager {
    storage: Arc<SystemStorage>,
    dummy_password_hash: Option<String>,
    /// 사용자별 락 (Check-Then-Act 및 Read-Modify-Write 원자성 보장)
    user_locks: DashMap<String, Arc<Mutex<()>>>,
}

impl SecurityManager {
    pub fn try_new(storage: Arc<SystemStorage>) -> SystemResult<Self> {
        let dummy_password_hash = hash_password("Zeta4j-Dummy-Password-For-Constant-Work!234")
            .map_err(|_| {
                SystemError::Internal(
                    "canonical dummy password verifier initialization failed".to_string(),
                )
            })?;
        Ok(Self {
            storage,
            dummy_password_hash: Some(dummy_password_hash),
            user_locks: DashMap::new(),
        })
    }

    /// Test and embedded constructor. Production startup uses `try_new` and
    /// fails before listeners if the fixed-work verifier cannot be prepared.
    /// 새 보안 관리자 생성
    pub fn new(storage: Arc<SystemStorage>) -> Self {
        Self {
            storage,
            dummy_password_hash: hash_password("Zeta4j-Dummy-Password-For-Constant-Work!234").ok(),
            user_locks: DashMap::new(),
        }
    }

    /// Shared durable system storage used by the canonical credential service.
    pub fn storage(&self) -> Arc<SystemStorage> {
        self.storage.clone()
    }

    /// 사용자별 락 획득
    fn get_user_lock(&self, username: &str) -> Arc<Mutex<()>> {
        self.user_locks
            .entry(username.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    // ========================================================================
    // Key Helpers
    // ========================================================================

    fn user_key(username: &str) -> Vec<u8> {
        format!("user:{}", username).into_bytes()
    }

    fn principal_key(principal_id: &CredentialSubjectId) -> Vec<u8> {
        format!("principal:{}", principal_id.as_str()).into_bytes()
    }

    fn refresh_family_key(family_id: &zeta4j_credential::TokenFamilyId) -> Vec<u8> {
        format!("family:{}", family_id.as_str()).into_bytes()
    }

    fn invalidate_subject_sessions(
        &self,
        transaction: &SystemWriteTransaction<'_>,
        subject: &CredentialSubjectId,
        updated_at: DateTime<Utc>,
    ) -> SystemResult<usize> {
        let sessions = transaction.scan_prefix::<AuthSession>(cf::SESSIONS, b"session:")?;
        let mut invalidated = 0_usize;
        for (session_key, session) in sessions {
            if session.subject != *subject || session.status != SessionStatus::Active {
                continue;
            }
            let closed = close_session(
                &session,
                SessionStatus::PrincipalChanged,
                updated_at.timestamp(),
            )
            .map_err(|failure| {
                SystemError::Internal(format!(
                    "failed to invalidate credential session: {failure}"
                ))
            })?;
            let family_key = Self::refresh_family_key(&session.family_id);
            if let Some(mut family) =
                transaction.get::<RefreshFamily>(cf::REFRESH_FAMILIES, &family_key)?
            {
                family.status = CredentialStatus::Revoked;
                family.version = family.version.saturating_add(1);
                transaction.put(cf::REFRESH_FAMILIES, &family_key, &family)?;
            }
            transaction.put(cf::SESSIONS, &session_key, &closed)?;
            invalidated = invalidated.saturating_add(1);
        }
        Ok(invalidated)
    }

    fn update_user_security_state(
        &self,
        username: &str,
        updated_at: DateTime<Utc>,
        update: impl FnOnce(&mut User) -> SystemResult<()>,
    ) -> SystemResult<()> {
        let key = Self::user_key(username);
        self.storage.write_transaction(|transaction| {
            let mut user: User = transaction
                .get(cf::USERS, &key)?
                .ok_or_else(|| SystemError::UserNotFound(username.to_string()))?;
            update(&mut user)?;
            user.updated_at = updated_at;
            user.security_generation = next_generation(user.security_generation);
            self.invalidate_subject_sessions(transaction, &user.principal_id, updated_at)?;
            transaction.put(cf::USERS, &key, &user)?;
            Ok(())
        })
    }

    fn delete_user_state(&self, username: &str, updated_at: DateTime<Utc>) -> SystemResult<()> {
        let key = Self::user_key(username);
        self.storage.write_transaction(|transaction| {
            let Some(user): Option<User> = transaction.get(cf::USERS, &key)? else {
                return Ok(());
            };
            self.invalidate_subject_sessions(transaction, &user.principal_id, updated_at)?;
            transaction.delete(cf::PRINCIPALS, &Self::principal_key(&user.principal_id))?;
            transaction.delete(cf::USERS, &key)?;
            Ok(())
        })
    }

    // ========================================================================
    // Initialization
    // ========================================================================

    /// Provision the only initial administrator for an empty security store.
    pub fn provision_bootstrap_admin(
        &self,
        username: &str,
        password: &str,
        expires_at: DateTime<Utc>,
    ) -> SystemResult<User> {
        if username.trim().is_empty() {
            return Err(SystemError::InvalidConfiguration(
                "bootstrap username cannot be empty".to_string(),
            ));
        }
        if expires_at <= Utc::now() {
            return Err(SystemError::BootstrapExpired);
        }
        if !self.list_users()?.is_empty() {
            return Err(SystemError::AlreadyInitialized);
        }

        let password_hash = hash_password(password)
            .map_err(|error| SystemError::Internal(format!("Password hash error: {error}")))?;
        let mut user = User::new(username, password_hash);
        user.password_change_required = true;
        user.bootstrap_expires_at = Some(expires_at);
        let digest = Sha256::digest(username.as_bytes());
        let mut generation_bytes = [0_u8; 8];
        generation_bytes.copy_from_slice(&digest[..8]);
        user.bootstrap_generation = u64::from_be_bytes(generation_bytes).max(1);
        user.home_database = Some("zeta4j".to_string());
        self.create_user_internal(user.clone())?;
        info!("Provisioned one-time bootstrap administrator");
        Ok(user)
    }

    // ========================================================================
    // User Management
    // ========================================================================

    /// 사용자 생성 (내부)
    fn create_user_internal(&self, user: User) -> SystemResult<()> {
        let key = Self::user_key(&user.username);
        let principal_key = Self::principal_key(&user.principal_id);
        self.storage.write_transaction(|transaction| {
            if transaction.get::<User>(cf::USERS, &key)?.is_some() {
                return Err(SystemError::UserAlreadyExists(user.username.clone()));
            }
            if transaction
                .get::<String>(cf::PRINCIPALS, &principal_key)?
                .is_some()
            {
                return Err(SystemError::InvalidConfiguration(
                    "credential principal identifier is already assigned".to_string(),
                ));
            }
            transaction.put(cf::USERS, &key, &user)?;
            transaction.put(cf::PRINCIPALS, &principal_key, &user.username)?;
            Ok(())
        })
    }

    /// 사용자 생성
    ///
    /// 동시성 안전: 사용자별 락을 사용하여 Check-Then-Act race condition 방지
    pub fn create_user(&self, username: &str, password: &str) -> SystemResult<()> {
        self.create_user_with_requirement(username, password, false)
    }

    /// 사용자와 초기 비밀번호 변경 요구를 하나의 record로 생성한다.
    pub fn create_user_with_requirement(
        &self,
        username: &str,
        password: &str,
        password_change_required: bool,
    ) -> SystemResult<()> {
        // 사용자별 락 획득 (동시성 안전)
        let lock = self.get_user_lock(username);
        let _guard = lock
            .lock()
            .map_err(|e| SystemError::Internal(format!("Failed to acquire user lock: {}", e)))?;

        let key = Self::user_key(username);

        // 중복 확인 (락 내에서 수행 - 안전)
        if self.storage.exists(cf::USERS, &key)? {
            return Err(SystemError::UserAlreadyExists(username.to_string()));
        }

        let password_hash = hash_password(password)
            .map_err(|e| SystemError::Internal(format!("Password hash error: {}", e)))?;

        let mut user = User::new(username, password_hash);
        user.password_change_required = password_change_required;
        self.create_user_internal(user)?;

        info!("Created user: {}", username);
        Ok(())
    }

    /// 사용자 조회
    pub fn get_user(&self, username: &str) -> SystemResult<Option<User>> {
        let key = Self::user_key(username);
        self.storage.get(cf::USERS, &key)
    }

    /// Resolve an immutable credential principal without scanning usernames.
    pub fn get_user_by_principal(
        &self,
        principal_id: &CredentialSubjectId,
    ) -> SystemResult<Option<User>> {
        let Some(username): Option<String> = self
            .storage
            .get(cf::PRINCIPALS, &Self::principal_key(principal_id))?
        else {
            return Ok(None);
        };
        let user = self.get_user(&username)?;
        match user {
            Some(user) if user.principal_id == *principal_id => Ok(Some(user)),
            Some(_) => Err(SystemError::InvalidConfiguration(
                "credential principal index does not match user state".to_string(),
            )),
            None => Err(SystemError::InvalidConfiguration(
                "credential principal index references a missing user".to_string(),
            )),
        }
    }

    /// 사용자 목록
    pub fn list_users(&self) -> SystemResult<Vec<User>> {
        let entries: Vec<(Vec<u8>, User)> = self.storage.scan_prefix(cf::USERS, b"user:")?;
        Ok(entries.into_iter().map(|(_, user)| user).collect())
    }

    /// Return whether a one-time bootstrap credential still blocks normal readiness.
    pub fn has_pending_bootstrap(&self) -> SystemResult<bool> {
        Ok(self
            .list_users()?
            .iter()
            .any(Self::is_pending_bootstrap_user))
    }

    /// 사용자 수정
    ///
    /// 동시성 안전: 사용자별 락을 사용하여 Read-Modify-Write 원자성 보장
    pub fn update_user(&self, user: User) -> SystemResult<()> {
        let lock = self.get_user_lock(&user.username);
        let _guard = lock
            .lock()
            .map_err(|e| SystemError::Internal(format!("Failed to acquire user lock: {}", e)))?;

        let key = Self::user_key(&user.username);

        self.storage.write_transaction(|transaction| {
            let current: User = transaction
                .get(cf::USERS, &key)?
                .ok_or_else(|| SystemError::UserNotFound(user.username.clone()))?;
            if current.principal_id != user.principal_id {
                return Err(SystemError::InvalidConfiguration(
                    "credential principal identifier is immutable".to_string(),
                ));
            }
            transaction.put(cf::USERS, &key, &user)?;
            Ok(())
        })
    }

    /// Apply an already-hashed user record from a security mutation.
    pub fn apply_user_creation(&self, user: User) -> SystemResult<()> {
        let lock = self.get_user_lock(&user.username);
        let _guard = lock.lock().map_err(|error| {
            SystemError::Internal(format!("Failed to acquire user lock: {error}"))
        })?;
        self.create_user_internal(user)
    }

    /// Apply a pre-hashed password update from a security mutation.
    pub fn apply_password_update(
        &self,
        username: &str,
        password_hash: String,
        require_change: bool,
        updated_at: DateTime<Utc>,
    ) -> SystemResult<()> {
        self.update_user_locked(username, updated_at, move |user| {
            if Self::is_pending_bootstrap_user(user) {
                return Err(SystemError::InvalidConfiguration(
                    "bootstrap password can only be changed by credential completion".to_string(),
                ));
            }
            user.password_hash = password_hash;
            user.password_change_required = require_change;
            if !require_change {
                user.bootstrap_expires_at = None;
            }
            Ok(())
        })
    }

    fn update_user_locked(
        &self,
        username: &str,
        updated_at: DateTime<Utc>,
        update: impl FnOnce(&mut User) -> SystemResult<()>,
    ) -> SystemResult<()> {
        let lock = self.get_user_lock(username);
        let _guard = lock.lock().map_err(|error| {
            SystemError::Internal(format!("Failed to acquire user lock: {error}"))
        })?;
        self.update_user_security_state(username, updated_at, update)
    }

    fn ensure_bootstrap_transition(user: &User, required: bool) -> SystemResult<()> {
        if !required && Self::is_pending_bootstrap_user(user) {
            return Err(SystemError::InvalidConfiguration(
                "bootstrap state can only be cleared by credential completion".to_string(),
            ));
        }
        Ok(())
    }

    fn is_pending_bootstrap_user(user: &User) -> bool {
        user.password_change_required
            && user.bootstrap_generation > 0
            && user.bootstrap_expires_at.is_some()
    }

    /// Apply an idempotent user deletion from a security mutation.
    pub fn apply_user_deletion(
        &self,
        username: &str,
        occurred_at: DateTime<Utc>,
    ) -> SystemResult<()> {
        let lock = self.get_user_lock(username);
        let _guard = lock.lock().map_err(|error| {
            SystemError::Internal(format!("Failed to acquire user lock: {error}"))
        })?;
        self.ensure_not_last_user(username)?;
        self.delete_user_state(username, occurred_at)
    }

    /// 사용자 삭제
    ///
    /// 동시성 안전: 사용자별 락을 사용하여 삭제 작업 직렬화
    pub fn delete_user(&self, username: &str) -> SystemResult<()> {
        let lock = self.get_user_lock(username);
        let _guard = lock
            .lock()
            .map_err(|e| SystemError::Internal(format!("Failed to acquire user lock: {}", e)))?;
        self.ensure_not_last_user(username)?;

        if self.get_user(username)?.is_none() {
            return Err(SystemError::UserNotFound(username.to_string()));
        }

        self.delete_user_state(username, Utc::now())?;
        info!("Deleted user: {}", username);
        Ok(())
    }

    /// Deleting the only user would leave the server without any credential.
    fn ensure_not_last_user(&self, username: &str) -> SystemResult<()> {
        if self.get_user(username)?.is_none() {
            return Ok(());
        }
        if self.list_users()?.len() <= 1 {
            return Err(SystemError::CannotDeleteLastUser);
        }
        Ok(())
    }

    /// 비밀번호 변경
    ///
    /// 동시성 안전: 사용자별 락을 사용하여 Read-Modify-Write 원자성 보장
    pub fn change_password(&self, username: &str, new_password: &str) -> SystemResult<()> {
        self.change_password_with_requirement(username, new_password, false)
    }

    /// 비밀번호와 다음 로그인 변경 요구를 한 번의 사용자 레코드 갱신으로 저장한다.
    pub fn change_password_with_requirement(
        &self,
        username: &str,
        new_password: &str,
        require_change: bool,
    ) -> SystemResult<()> {
        let lock = self.get_user_lock(username);
        let _guard = lock
            .lock()
            .map_err(|e| SystemError::Internal(format!("Failed to acquire user lock: {}", e)))?;

        let password_hash = hash_password(new_password)
            .map_err(|e| SystemError::Internal(format!("Password hash error: {}", e)))?;
        self.update_user_security_state(username, Utc::now(), move |user| {
            if Self::is_pending_bootstrap_user(user) {
                return Err(SystemError::InvalidConfiguration(
                    "bootstrap password can only be changed by credential completion".to_string(),
                ));
            }
            user.password_hash = password_hash;
            user.password_change_required = require_change;
            Ok(())
        })?;
        info!("Changed password for user: {}", username);
        Ok(())
    }

    /// Atomically consume an initial bootstrap credential and install the
    /// administrator's permanent password.
    pub fn complete_bootstrap(
        &self,
        username: &str,
        current_password: &str,
        new_password: &str,
    ) -> SystemResult<User> {
        let lock = self.get_user_lock(username);
        let _guard = lock.lock().map_err(|error| {
            SystemError::Internal(format!("Failed to acquire user lock: {error}"))
        })?;
        let password_hash = hash_password(new_password)
            .map_err(|error| SystemError::Internal(format!("Password hash error: {error}")))?;
        self.update_user_security_state(username, Utc::now(), move |user| {
            Self::validate_bootstrap_user(user, current_password)?;
            user.password_hash = password_hash;
            user.password_change_required = false;
            user.bootstrap_expires_at = None;
            Ok(())
        })?;
        info!("Completed one-time bootstrap credential");
        self.get_user(username)?
            .ok_or_else(|| SystemError::Internal("completed bootstrap user is missing".to_string()))
    }

    fn validate_bootstrap_user(user: &User, current_password: &str) -> SystemResult<()> {
        if !Self::is_pending_bootstrap_user(user) {
            return Err(SystemError::BootstrapConsumed);
        }
        let Some(expiry) = user.bootstrap_expires_at else {
            return Err(SystemError::BootstrapConsumed);
        };
        if expiry <= Utc::now() {
            return Err(SystemError::BootstrapExpired);
        }
        match verify_password(current_password, &user.password_hash) {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(SystemError::AuthenticationFailed(
                "Invalid credential".to_string(),
            )),
        }
    }

    /// 사용자 정지/활성화
    ///
    /// 동시성 안전: 사용자별 락을 사용하여 Read-Modify-Write 원자성 보장
    pub fn set_suspended(&self, username: &str, suspended: bool) -> SystemResult<()> {
        let lock = self.get_user_lock(username);
        let _guard = lock
            .lock()
            .map_err(|e| SystemError::Internal(format!("Failed to acquire user lock: {}", e)))?;

        self.update_user_security_state(username, Utc::now(), move |user| {
            user.suspended = suspended;
            Ok(())
        })?;
        info!("User '{}' suspended: {}", username, suspended);
        Ok(())
    }

    /// 비밀번호 변경 필요 설정
    ///
    /// 동시성 안전: 사용자별 락을 사용하여 Read-Modify-Write 원자성 보장
    pub fn set_password_change_required(&self, username: &str, required: bool) -> SystemResult<()> {
        let lock = self.get_user_lock(username);
        let _guard = lock
            .lock()
            .map_err(|e| SystemError::Internal(format!("Failed to acquire user lock: {}", e)))?;

        self.update_user_security_state(username, Utc::now(), move |user| {
            Self::ensure_bootstrap_transition(user, required)?;
            user.password_change_required = required;
            Ok(())
        })?;
        info!("User '{}' password_change_required: {}", username, required);
        Ok(())
    }

    // ========================================================================
    // Authentication
    // ========================================================================

    /// 인증
    pub fn authenticate(&self, username: &str, password: &str) -> SystemResult<User> {
        let user = self.get_user(username)?;
        let selected_hash = user
            .as_ref()
            .map(|user| user.password_hash.as_str())
            .or(self.dummy_password_hash.as_deref())
            .ok_or_else(|| {
                SystemError::Internal(
                    "canonical dummy password verifier is unavailable".to_string(),
                )
            })?;
        let verification = verify_password_outcome(password, selected_hash);
        let valid = verification.as_ref().is_ok_and(|outcome| {
            matches!(
                outcome,
                zeta4j_credential::PasswordVerification::Valid
                    | zeta4j_credential::PasswordVerification::ValidNeedsUpgrade
            )
        });
        let Some(user) = user else {
            return Err(SystemError::AuthenticationFailed(
                "Invalid credential".to_string(),
            ));
        };
        if !valid || user.suspended {
            return Err(SystemError::AuthenticationFailed(
                "Invalid credential".to_string(),
            ));
        }
        if matches!(
            verification,
            Ok(zeta4j_credential::PasswordVerification::ValidNeedsUpgrade)
        ) {
            self.upgrade_password_hash(username, &user.password_hash, password)?;
        }

        if user.password_change_required {
            if Self::is_pending_bootstrap_user(&user)
                && user
                    .bootstrap_expires_at
                    .is_some_and(|expiry| expiry <= Utc::now())
            {
                return Err(SystemError::BootstrapExpired);
            }
            return Err(SystemError::PasswordChangeRequired);
        }

        Ok(user)
    }

    /// 비밀번호 검증
    pub fn verify_password(&self, username: &str, password: &str) -> SystemResult<bool> {
        let user = self.get_user(username)?;
        let selected_hash = user
            .as_ref()
            .map(|user| user.password_hash.as_str())
            .or(self.dummy_password_hash.as_deref())
            .ok_or_else(|| {
                SystemError::Internal(
                    "canonical dummy password verifier is unavailable".to_string(),
                )
            })?;
        let valid = verify_password(password, selected_hash).is_ok_and(|valid| valid);
        Ok(valid && user.is_some_and(|user| !user.suspended))
    }

    fn upgrade_password_hash(
        &self,
        username: &str,
        expected_hash: &str,
        password: &str,
    ) -> SystemResult<()> {
        let replacement = hash_password(password).map_err(|_| {
            SystemError::Internal("canonical password hash upgrade failed".to_string())
        })?;
        let lock = self.get_user_lock(username);
        let _guard = lock.lock().map_err(|error| {
            SystemError::Internal(format!("Failed to acquire user lock: {error}"))
        })?;
        let key = Self::user_key(username);
        self.storage.write_transaction(|transaction| {
            let Some(mut user): Option<User> = transaction.get(cf::USERS, &key)? else {
                return Ok(());
            };
            if user.password_hash == expected_hash {
                user.password_hash = replacement;
                user.updated_at = Utc::now();
                transaction.put(cf::USERS, &key, &user)?;
            }
            Ok(())
        })
    }

    /// Resolves a user subject from the current persisted security state.
    pub fn resolve_user_principal(&self, username: &str) -> SystemResult<Principal> {
        let user = self
            .get_user(username)?
            .ok_or_else(|| SystemError::AuthenticationFailed("Invalid credential".to_string()))?;
        let principal_id = StableId::new(user.principal_id.as_str())
            .map_err(|error| SystemError::Internal(error.to_string()))?;
        let tenant_id =
            StableId::new("system").map_err(|error| SystemError::Internal(error.to_string()))?;
        let status = if user.suspended {
            PrincipalStatus::Suspended
        } else if Self::is_pending_bootstrap_user(&user) {
            PrincipalStatus::BootstrapPending
        } else {
            PrincipalStatus::Active
        };
        Ok(Principal {
            id: principal_id,
            kind: PrincipalKind::User,
            status,
            tenant_id,
            security_generation: user.security_generation,
        })
    }
}

fn next_generation(current: u64) -> u64 {
    current.saturating_add(1).max(1)
}

#[cfg(test)]
mod tests {
    use zeta4j_credential::{
        AuthSession, CorrelationId, CredentialEvent, CredentialEventKind, CredentialKind,
        CredentialSecret, CredentialStatus, KeyVersion, RefreshFamily, SessionId, SessionStatus,
        TokenFamilyId, TokenId, digest_secret,
    };

    use super::super::credential::{CredentialStore, LocalCredentialStore};
    use super::*;

    #[test]
    fn immutable_principal_index_survives_reopen() -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let principal_id = {
            let storage = Arc::new(SystemStorage::open(temp.path())?);
            let security = SecurityManager::new(storage);
            security.create_user("indexed-user", "Valid!Password234")?;
            let user = security
                .get_user("indexed-user")?
                .ok_or_else(|| std::io::Error::other("created user is missing"))?;
            let resolved = security
                .get_user_by_principal(&user.principal_id)?
                .ok_or_else(|| std::io::Error::other("principal index is missing"))?;
            assert_eq!(resolved.username, "indexed-user");
            user.principal_id
        };

        let reopened = SecurityManager::new(Arc::new(SystemStorage::open(temp.path())?));
        assert_eq!(
            reopened
                .get_user_by_principal(&principal_id)?
                .map(|user| user.username),
            Some("indexed-user".to_string())
        );
        Ok(())
    }

    #[test]
    fn unknown_wrong_and_suspended_passwords_share_public_result()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = SecurityManager::new(storage);
        security.create_user("password-user", "Correct!Password234")?;
        let unknown = security
            .authenticate("unknown-user", "Wrong!Password234")
            .map(|_| ())
            .map_err(|error| error.to_string());
        let wrong = security
            .authenticate("password-user", "Wrong!Password234")
            .map(|_| ())
            .map_err(|error| error.to_string());
        security.set_suspended("password-user", true)?;
        let suspended = security
            .authenticate("password-user", "Correct!Password234")
            .map(|_| ())
            .map_err(|error| error.to_string());
        assert_eq!(unknown, wrong);
        assert_eq!(wrong, suspended);
        assert_eq!(
            unknown,
            Err("Authentication failed: Invalid credential".to_string())
        );
        Ok(())
    }

    #[test]
    fn principal_mutation_atomically_revokes_session_and_family()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = SecurityManager::new(storage.clone());
        security.create_user("session-user", "Valid!Password234")?;
        let user = security
            .get_user("session-user")?
            .ok_or_else(|| std::io::Error::other("created user is missing"))?;
        let session_id = SessionId::new();
        let family_id = TokenFamilyId::new();
        let digest = digest_secret(b"refresh", &CredentialSecret::new("refresh-secret"));
        let session = AuthSession {
            id: session_id.clone(),
            subject: user.principal_id.clone(),
            credential_kind: CredentialKind::Password,
            created_at: 1,
            last_refreshed_at: 1,
            absolute_expires_at: i64::MAX,
            idle_expires_at: i64::MAX,
            security_generation: user.security_generation,
            key_version: KeyVersion::new(1)?,
            family_id: family_id.clone(),
            status: SessionStatus::Active,
            closed_at: None,
        };
        let family = RefreshFamily {
            id: family_id.clone(),
            session_id: session_id.clone(),
            current_token_id: TokenId::new(),
            current_digest: digest,
            consumed_digests: Vec::new(),
            status: CredentialStatus::Active,
            expires_at: i64::MAX,
            version: 1,
        };
        let event = CredentialEvent {
            kind: CredentialEventKind::SessionOpened,
            subject: user.principal_id,
            session_id: Some(session_id.clone()),
            family_id: Some(family_id.clone()),
            credential_id: None,
            key_version: None,
            correlation_id: CorrelationId::new(),
            node_id: "test".to_string(),
            occurred_at: 1,
        };
        let credential_store = LocalCredentialStore::new(storage.clone());
        credential_store.open(&session, &family, &event, 16, 10_000)?;

        security.set_suspended("session-user", true)?;

        assert_eq!(
            credential_store
                .session(&session_id)?
                .map(|session| session.status),
            Some(SessionStatus::PrincipalChanged)
        );
        assert_eq!(
            credential_store
                .family(&family_id)?
                .map(|family| family.status),
            Some(CredentialStatus::Revoked)
        );
        Ok(())
    }

    #[test]
    fn fresh_store_has_no_known_administrator_and_bootstrap_is_one_time()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = SecurityManager::new(storage);
        assert!(security.list_users()?.is_empty());

        security.provision_bootstrap_admin(
            "operator-4f91",
            "Temporary!234",
            Utc::now() + chrono::Duration::minutes(15),
        )?;
        assert!(matches!(
            security.authenticate("operator-4f91", "Temporary!234"),
            Err(SystemError::PasswordChangeRequired)
        ));
        security.complete_bootstrap("operator-4f91", "Temporary!234", "Permanent!567")?;
        assert!(
            security
                .authenticate("operator-4f91", "Permanent!567")
                .is_ok()
        );
        assert!(matches!(
            security.complete_bootstrap("operator-4f91", "Temporary!234", "AnotherPass!890"),
            Err(SystemError::BootstrapConsumed)
        ));
        Ok(())
    }

    #[test]
    fn user_mutations_preserve_unrelated_fields() -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = SecurityManager::new(storage);
        security.apply_user_creation(User::new("mutated", "old-hash".to_string()))?;

        security.apply_password_update("mutated", "new-hash".to_string(), true, Utc::now())?;
        let user = security
            .get_user("mutated")?
            .ok_or("mutated user is missing")?;

        assert_eq!(user.password_hash, "new-hash");
        assert!(user.password_change_required);
        Ok(())
    }

    #[test]
    fn generic_password_change_flag_cannot_consume_bootstrap()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = SecurityManager::new(storage);
        security.provision_bootstrap_admin(
            "operator-13b8",
            "Temporary!234",
            Utc::now() + chrono::Duration::minutes(15),
        )?;

        assert!(matches!(
            security.set_password_change_required("operator-13b8", false),
            Err(SystemError::InvalidConfiguration(_))
        ));
        assert!(matches!(
            security.change_password("operator-13b8", "Replacement!567"),
            Err(SystemError::InvalidConfiguration(_))
        ));
        assert!(matches!(
            security.apply_password_update(
                "operator-13b8",
                "replacement-hash".to_string(),
                false,
                Utc::now(),
            ),
            Err(SystemError::InvalidConfiguration(_))
        ));
        assert!(matches!(
            security.authenticate("operator-13b8", "Temporary!234"),
            Err(SystemError::PasswordChangeRequired)
        ));
        Ok(())
    }

    #[test]
    fn completed_bootstrap_user_can_later_follow_normal_password_change_policy()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = SecurityManager::new(storage);
        security.provision_bootstrap_admin(
            "operator-completed",
            "Temporary!234",
            Utc::now() + chrono::Duration::minutes(15),
        )?;
        security.complete_bootstrap("operator-completed", "Temporary!234", "Permanent!567")?;

        security.set_password_change_required("operator-completed", true)?;
        assert!(matches!(
            security.authenticate("operator-completed", "Permanent!567"),
            Err(SystemError::PasswordChangeRequired)
        ));
        security.set_password_change_required("operator-completed", false)?;
        assert!(
            security
                .authenticate("operator-completed", "Permanent!567")
                .is_ok()
        );
        Ok(())
    }

    #[test]
    fn ordinary_password_change_requirement_is_not_bootstrap_expiry()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = SecurityManager::new(storage);
        security.create_user("reader", "Valid!Password234")?;
        security.set_password_change_required("reader", true)?;

        assert!(matches!(
            security.authenticate("reader", "Valid!Password234"),
            Err(SystemError::PasswordChangeRequired)
        ));
        Ok(())
    }

    #[test]
    fn expired_bootstrap_cannot_authenticate_or_complete() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = SecurityManager::new(storage);
        security.provision_bootstrap_admin(
            "operator-expiry",
            "Temporary!234",
            Utc::now() + chrono::Duration::milliseconds(20),
        )?;
        std::thread::sleep(std::time::Duration::from_millis(40));

        assert!(matches!(
            security.authenticate("operator-expiry", "Temporary!234"),
            Err(SystemError::BootstrapExpired)
        ));
        assert!(matches!(
            security.complete_bootstrap("operator-expiry", "Temporary!234", "Permanent!567",),
            Err(SystemError::BootstrapExpired)
        ));
        Ok(())
    }

    #[test]
    fn concurrent_bootstrap_completion_has_exactly_one_winner()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = Arc::new(SecurityManager::new(storage));
        security.provision_bootstrap_admin(
            "operator-race",
            "Temporary!234",
            Utc::now() + chrono::Duration::minutes(15),
        )?;

        let first_security = security.clone();
        let first = std::thread::spawn(move || {
            first_security.complete_bootstrap("operator-race", "Temporary!234", "Permanent!567")
        });
        let second_security = security.clone();
        let second = std::thread::spawn(move || {
            second_security.complete_bootstrap("operator-race", "Temporary!234", "Alternative!890")
        });
        let results = [
            first.join().map_err(|_| "bootstrap worker panicked")?,
            second.join().map_err(|_| "bootstrap worker panicked")?,
        ];

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(SystemError::BootstrapConsumed)))
                .count(),
            1
        );
        Ok(())
    }
}
