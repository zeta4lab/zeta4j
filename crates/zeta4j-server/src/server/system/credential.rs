//! Durable local credential state and deterministic service seams.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use zeta4j_credential::{
    AccessClaims, ApiKeyRecord, AuthSession, ClaimValidation, CorrelationId, CredentialAudience,
    CredentialEvent, CredentialEventKind, CredentialFailure, CredentialId, CredentialKind,
    CredentialScope, CredentialSecret, CredentialStatus, CredentialSubjectId, KeyVersion,
    PasswordHash, PasswordPolicy, PasswordVerification, RefreshClaims, RefreshFamily,
    RefreshTransition, SecretDigest, SessionId, SessionStatus, SigningKeyStatus,
    SigningVerificationKey, TokenFamilyId, TokenId, TokenType, ValidatedClaims, close_session,
    consume_refresh, digest_secret, revoke_refresh_family, transition_api_key,
    transition_signing_key,
};

use super::{
    SecurityManager, SystemError, SystemResult, User,
    storage::{SystemStorage, cf},
};

pub trait CredentialClock: Send + Sync {
    fn now(&self) -> i64;
}

#[derive(Debug, Default)]
pub struct SystemCredentialClock;

impl CredentialClock for SystemCredentialClock {
    fn now(&self) -> i64 {
        Utc::now().timestamp()
    }
}

pub trait CredentialRandom: Send + Sync {
    fn session_id(&self) -> SessionId;
    fn family_id(&self) -> TokenFamilyId;
    fn token_id(&self) -> TokenId;
    fn credential_id(&self) -> CredentialId;
    fn correlation_id(&self) -> CorrelationId;
    fn secret(&self, bytes: usize) -> SystemResult<CredentialSecret>;
}

#[derive(Debug, Default)]
pub struct SystemCredentialRandom;

impl CredentialRandom for SystemCredentialRandom {
    fn session_id(&self) -> SessionId {
        SessionId::new()
    }

    fn family_id(&self) -> TokenFamilyId {
        TokenFamilyId::new()
    }

    fn token_id(&self) -> TokenId {
        TokenId::new()
    }

    fn credential_id(&self) -> CredentialId {
        CredentialId::new()
    }

    fn correlation_id(&self) -> CorrelationId {
        CorrelationId::new()
    }

    fn secret(&self, bytes: usize) -> SystemResult<CredentialSecret> {
        if !(32..=1024).contains(&bytes) {
            return Err(SystemError::InvalidConfiguration(
                "credential entropy length must be between 32 and 1024 bytes".to_string(),
            ));
        }
        let mut random = vec![0_u8; bytes];
        rand::rng().fill(random.as_mut_slice());
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let encoded: String = random
            .into_iter()
            .map(|byte| char::from(ALPHABET[usize::from(byte & 63)]))
            .collect();
        Ok(CredentialSecret::new(encoded))
    }
}

pub trait CredentialSigner: Send + Sync {
    fn key_version(&self) -> KeyVersion;
    fn verification_key(
        &self,
        status: SigningKeyStatus,
        prepared_at: i64,
    ) -> SigningVerificationKey;
    fn sign_access(&self, claims: &AccessClaims) -> Result<CredentialSecret, CredentialFailure>;
    fn sign_refresh(&self, claims: &RefreshClaims) -> Result<CredentialSecret, CredentialFailure>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePrincipal {
    pub id: CredentialSubjectId,
    pub name: String,
    pub status: CredentialStatus,
    pub security_generation: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialThrottleState {
    pub storage_key: String,
    pub window_started_at: i64,
    pub attempts: u32,
    pub last_seen_at: i64,
    pub last_correlation_id: CorrelationId,
    pub last_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityMutation {
    CreateUser {
        user: User,
    },
    DeleteUser {
        username: String,
        occurred_at: DateTime<Utc>,
    },
    SetUserPassword {
        username: String,
        password_hash: String,
        require_change: bool,
        updated_at: DateTime<Utc>,
    },
    OpenSession {
        session: AuthSession,
        family: RefreshFamily,
        event: CredentialEvent,
        maximum_subject_sessions: usize,
        maximum_global_sessions: usize,
    },
    ConsumeRefresh {
        family_id: TokenFamilyId,
        supplied_digest: SecretDigest,
        successor_token_id: TokenId,
        successor_digest: SecretDigest,
        occurred_at: i64,
        idle_expires_at: i64,
        correlation_id: CorrelationId,
        node_id: String,
    },
    CloseSession {
        id: SessionId,
        subject: CredentialSubjectId,
        reason: SessionStatus,
        occurred_at: i64,
        event: CredentialEvent,
    },
    InstallApiKey {
        principal: ServicePrincipal,
        record: ApiKeyRecord,
        event: CredentialEvent,
    },
    RotateApiKey {
        predecessor_id: CredentialId,
        successor: ApiKeyRecord,
        overlap_until: i64,
        occurred_at: i64,
        event: CredentialEvent,
    },
    RevokeApiKey {
        id: CredentialId,
        occurred_at: i64,
        event: CredentialEvent,
    },
    ConsumeAdmission {
        bucket_key: SecretDigest,
        occurred_at: i64,
        correlation_id: CorrelationId,
        maximum_buckets: usize,
        attempts_per_minute: u32,
    },
    RetireSigningKeys {
        occurred_at: i64,
    },
    CompromiseSigningKey {
        version: KeyVersion,
        occurred_at: i64,
        correlation_id: CorrelationId,
        node_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityMutationOutcome {
    Applied,
    RefreshRotated,
    RefreshReplayRevoked,
    AdmissionGranted,
    AdmissionThrottled,
}

pub trait CredentialStore: Send + Sync {
    fn apply_security_mutation(
        &self,
        mutation: SecurityMutation,
    ) -> SystemResult<SecurityMutationOutcome>;
    fn session(&self, id: &SessionId) -> SystemResult<Option<AuthSession>>;
    fn family(&self, id: &TokenFamilyId) -> SystemResult<Option<RefreshFamily>>;
    fn admission_outcome(&self, correlation_id: &CorrelationId) -> SystemResult<Option<bool>>;
    fn sessions(&self) -> SystemResult<Vec<AuthSession>>;
    fn sessions_for_subject(&self, subject: &CredentialSubjectId)
    -> SystemResult<Vec<AuthSession>>;
    fn signing_keys(&self) -> SystemResult<Vec<SigningVerificationKey>>;
    fn retire_expired_signing_keys(&self, now: i64) -> SystemResult<Vec<SigningVerificationKey>>;
    fn prepare_signing_key(&self, key: &SigningVerificationKey) -> SystemResult<()>;
    fn activate_signing_key(
        &self,
        version: KeyVersion,
        now: i64,
        previous_verify_until: Option<i64>,
    ) -> SystemResult<Vec<SigningVerificationKey>>;
    fn compromise_signing_key(
        &self,
        version: KeyVersion,
        now: i64,
        correlation_id: CorrelationId,
        node_id: &str,
    ) -> SystemResult<usize>;
    fn api_key(&self, id: &CredentialId) -> SystemResult<Option<ApiKeyRecord>>;
    fn api_keys(&self) -> SystemResult<Vec<ApiKeyRecord>>;
    fn service_principal(&self, id: &CredentialSubjectId)
    -> SystemResult<Option<ServicePrincipal>>;
    fn install_api_key(
        &self,
        principal: &ServicePrincipal,
        record: &ApiKeyRecord,
        event: &CredentialEvent,
    ) -> SystemResult<()>;
    fn rotate_api_key(
        &self,
        predecessor_id: &CredentialId,
        successor: &ApiKeyRecord,
        overlap_until: i64,
        now: i64,
        event: &CredentialEvent,
    ) -> SystemResult<ApiKeyRecord>;
    fn revoke_api_key(
        &self,
        id: &CredentialId,
        now: i64,
        event: &CredentialEvent,
    ) -> SystemResult<ApiKeyRecord>;
    fn open(
        &self,
        session: &AuthSession,
        family: &RefreshFamily,
        event: &CredentialEvent,
        maximum_subject_sessions: usize,
        maximum_global_sessions: usize,
    ) -> SystemResult<()>;
    #[allow(clippy::too_many_arguments)]
    fn consume(
        &self,
        family_id: &TokenFamilyId,
        supplied_digest: &SecretDigest,
        successor_token_id: TokenId,
        successor_digest: SecretDigest,
        now: i64,
        idle_expires_at: i64,
        correlation_id: CorrelationId,
        node_id: &str,
    ) -> SystemResult<RefreshTransition>;
    fn close(
        &self,
        session_id: &SessionId,
        reason: SessionStatus,
        now: i64,
        event: &CredentialEvent,
    ) -> SystemResult<AuthSession>;
    fn close_subject_sessions(
        &self,
        subject: &CredentialSubjectId,
        reason: SessionStatus,
        now: i64,
        correlation_id: &CorrelationId,
        node_id: &str,
    ) -> SystemResult<usize>;
}

#[derive(Clone)]
pub struct LocalCredentialStore {
    storage: Arc<SystemStorage>,
}

impl LocalCredentialStore {
    pub fn new(storage: Arc<SystemStorage>) -> Self {
        Self { storage }
    }

    fn session_key(id: &SessionId) -> Vec<u8> {
        format!("session:{}", id.as_str()).into_bytes()
    }

    fn family_key(id: &TokenFamilyId) -> Vec<u8> {
        format!("family:{}", id.as_str()).into_bytes()
    }

    fn signing_key_key(version: KeyVersion) -> Vec<u8> {
        format!("signing-key:{:020}", version.get()).into_bytes()
    }

    fn api_key_key(id: &CredentialId) -> Vec<u8> {
        format!("api-key:{}", id.as_str()).into_bytes()
    }

    fn service_principal_key(id: &CredentialSubjectId) -> Vec<u8> {
        format!("service-principal:{}", id.as_str()).into_bytes()
    }

    fn throttle_key(digest: &SecretDigest) -> String {
        format!("throttle:{}", hex::encode(digest.as_bytes()))
    }

    fn map_failure(failure: CredentialFailure) -> SystemError {
        match failure {
            CredentialFailure::Internal => SystemError::Internal(failure.to_string()),
            _ => SystemError::Credential(failure),
        }
    }

    pub fn apply_security_mutation(
        &self,
        mutation: SecurityMutation,
    ) -> SystemResult<SecurityMutationOutcome> {
        match mutation {
            SecurityMutation::CreateUser { .. }
            | SecurityMutation::DeleteUser { .. }
            | SecurityMutation::SetUserPassword { .. } => Err(SystemError::InvalidConfiguration(
                "principal mutation requires the unified security state boundary".to_string(),
            )),
            SecurityMutation::OpenSession {
                session,
                family,
                event,
                maximum_subject_sessions,
                maximum_global_sessions,
            } => self
                .open(
                    &session,
                    &family,
                    &event,
                    maximum_subject_sessions,
                    maximum_global_sessions,
                )
                .map(|()| SecurityMutationOutcome::Applied),
            SecurityMutation::ConsumeRefresh {
                family_id,
                supplied_digest,
                successor_token_id,
                successor_digest,
                occurred_at,
                idle_expires_at,
                correlation_id,
                node_id,
            } => self
                .consume(
                    &family_id,
                    &supplied_digest,
                    successor_token_id,
                    successor_digest,
                    occurred_at,
                    idle_expires_at,
                    correlation_id,
                    &node_id,
                )
                .map(|transition| match transition {
                    RefreshTransition::Rotated { .. } => SecurityMutationOutcome::RefreshRotated,
                    RefreshTransition::ReplayRevoked { .. } => {
                        SecurityMutationOutcome::RefreshReplayRevoked
                    }
                }),
            SecurityMutation::CloseSession {
                id,
                subject,
                reason,
                occurred_at,
                event,
            } => {
                if event.subject != subject || event.session_id.as_ref() != Some(&id) {
                    return Err(SystemError::InvalidConfiguration(
                        "credential session close state is inconsistent".to_string(),
                    ));
                }
                self.close(&id, reason, occurred_at, &event)
                    .map(|_| SecurityMutationOutcome::Applied)
            }
            SecurityMutation::InstallApiKey {
                principal,
                record,
                event,
            } => self
                .install_api_key(&principal, &record, &event)
                .map(|()| SecurityMutationOutcome::Applied),
            SecurityMutation::RotateApiKey {
                predecessor_id,
                successor,
                overlap_until,
                occurred_at,
                event,
            } => self
                .rotate_api_key(
                    &predecessor_id,
                    &successor,
                    overlap_until,
                    occurred_at,
                    &event,
                )
                .map(|_| SecurityMutationOutcome::Applied),
            SecurityMutation::RevokeApiKey {
                id,
                occurred_at,
                event,
            } => self
                .revoke_api_key(&id, occurred_at, &event)
                .map(|_| SecurityMutationOutcome::Applied),
            SecurityMutation::ConsumeAdmission {
                bucket_key,
                occurred_at,
                correlation_id,
                maximum_buckets,
                attempts_per_minute,
            } => self.consume_admission(
                &bucket_key,
                occurred_at,
                correlation_id,
                maximum_buckets,
                attempts_per_minute,
            ),
            SecurityMutation::RetireSigningKeys { occurred_at } => self
                .retire_expired_signing_keys(occurred_at)
                .map(|_| SecurityMutationOutcome::Applied),
            SecurityMutation::CompromiseSigningKey {
                version,
                occurred_at,
                correlation_id,
                node_id,
            } => self
                .compromise_signing_key(version, occurred_at, correlation_id, &node_id)
                .map(|_| SecurityMutationOutcome::Applied),
        }
    }

    fn consume_admission(
        &self,
        bucket_key: &SecretDigest,
        now: i64,
        correlation_id: CorrelationId,
        maximum_buckets: usize,
        attempts_per_minute: u32,
    ) -> SystemResult<SecurityMutationOutcome> {
        if maximum_buckets == 0 || attempts_per_minute == 0 {
            return Err(SystemError::Credential(CredentialFailure::Policy));
        }
        self.storage.write_transaction(|transaction| {
            let desired_key = Self::throttle_key(bucket_key);
            let existing = transaction
                .get::<CredentialThrottleState>(cf::CREDENTIAL_THROTTLES, desired_key.as_bytes())?;
            let storage_key = if existing.is_some() {
                desired_key
            } else {
                let retained = transaction
                    .scan_prefix::<CredentialThrottleState>(cf::CREDENTIAL_THROTTLES, b"throttle:")?
                    .len();
                if retained < maximum_buckets {
                    desired_key
                } else {
                    "throttle:overflow".to_string()
                }
            };
            let mut state = transaction
                .get::<CredentialThrottleState>(cf::CREDENTIAL_THROTTLES, storage_key.as_bytes())?
                .unwrap_or(CredentialThrottleState {
                    storage_key: storage_key.clone(),
                    window_started_at: now,
                    attempts: 0,
                    last_seen_at: now,
                    last_correlation_id: correlation_id.clone(),
                    last_allowed: true,
                });
            if now.saturating_sub(state.window_started_at) >= 60 {
                state.window_started_at = now;
                state.attempts = 0;
            }
            state.last_seen_at = now;
            state.last_correlation_id = correlation_id;
            state.last_allowed = state.attempts < attempts_per_minute;
            state.attempts = state.attempts.saturating_add(1);
            transaction.put(
                cf::CREDENTIAL_THROTTLES,
                state.storage_key.as_bytes(),
                &state,
            )?;
            if state.last_allowed {
                Ok(SecurityMutationOutcome::AdmissionGranted)
            } else {
                Ok(SecurityMutationOutcome::AdmissionThrottled)
            }
        })
    }
}

impl CredentialStore for LocalCredentialStore {
    fn apply_security_mutation(
        &self,
        mutation: SecurityMutation,
    ) -> SystemResult<SecurityMutationOutcome> {
        LocalCredentialStore::apply_security_mutation(self, mutation)
    }

    fn session(&self, id: &SessionId) -> SystemResult<Option<AuthSession>> {
        self.storage.get(cf::SESSIONS, &Self::session_key(id))
    }

    fn family(&self, id: &TokenFamilyId) -> SystemResult<Option<RefreshFamily>> {
        self.storage
            .get(cf::REFRESH_FAMILIES, &Self::family_key(id))
    }

    fn admission_outcome(&self, correlation_id: &CorrelationId) -> SystemResult<Option<bool>> {
        self.storage
            .scan_prefix::<CredentialThrottleState>(cf::CREDENTIAL_THROTTLES, b"throttle:")
            .map(|rows| {
                rows.into_iter()
                    .map(|(_, state)| state)
                    .find(|state| state.last_correlation_id == *correlation_id)
                    .map(|state| state.last_allowed)
            })
    }

    fn sessions(&self) -> SystemResult<Vec<AuthSession>> {
        self.storage
            .scan_prefix::<AuthSession>(cf::SESSIONS, b"session:")
            .map(|rows| rows.into_iter().map(|(_, session)| session).collect())
    }

    fn sessions_for_subject(
        &self,
        subject: &CredentialSubjectId,
    ) -> SystemResult<Vec<AuthSession>> {
        self.storage
            .scan_prefix::<AuthSession>(cf::SESSIONS, b"session:")
            .map(|rows| {
                rows.into_iter()
                    .map(|(_, session)| session)
                    .filter(|session| session.subject == *subject)
                    .collect()
            })
    }

    fn signing_keys(&self) -> SystemResult<Vec<SigningVerificationKey>> {
        self.storage
            .scan_prefix::<SigningVerificationKey>(cf::SIGNING_KEYS, b"signing-key:")
            .map(|rows| rows.into_iter().map(|(_, key)| key).collect())
    }

    fn retire_expired_signing_keys(&self, now: i64) -> SystemResult<Vec<SigningVerificationKey>> {
        self.storage.write_transaction(|transaction| {
            let mut keys = transaction
                .scan_prefix::<SigningVerificationKey>(cf::SIGNING_KEYS, b"signing-key:")?;
            for (storage_key, key) in &mut keys {
                if key.status == SigningKeyStatus::VerifyOnly
                    && key.verify_until.is_some_and(|deadline| now >= deadline)
                {
                    *key = transition_signing_key(key, SigningKeyStatus::Retired, now, None)
                        .map_err(Self::map_failure)?;
                    transaction.put(cf::SIGNING_KEYS, storage_key, key)?;
                }
            }
            Ok(keys.into_iter().map(|(_, key)| key).collect())
        })
    }

    fn prepare_signing_key(&self, key: &SigningVerificationKey) -> SystemResult<()> {
        if key.status != SigningKeyStatus::Prepared
            || key.public_key.len() != 32
            || key.key_id.is_empty()
        {
            return Err(SystemError::InvalidConfiguration(
                "signing verification key is not a valid prepared key".to_string(),
            ));
        }
        self.storage.write_transaction(|transaction| {
            let keys = transaction
                .scan_prefix::<SigningVerificationKey>(cf::SIGNING_KEYS, b"signing-key:")?;
            if keys.iter().any(|(_, existing)| {
                existing.version == key.version
                    && existing.key_id == key.key_id
                    && existing.public_key == key.public_key
                    && existing.status == SigningKeyStatus::Prepared
            }) {
                return Ok(());
            }
            if keys.iter().any(|(_, existing)| {
                existing.version == key.version || existing.key_id == key.key_id
            }) {
                return Err(SystemError::InvalidConfiguration(
                    "signing key version and key ID cannot be reused".to_string(),
                ));
            }
            if keys
                .iter()
                .map(|(_, existing)| existing.version)
                .max()
                .is_some_and(|maximum| key.version <= maximum)
            {
                return Err(SystemError::InvalidConfiguration(
                    "signing key version must increase monotonically".to_string(),
                ));
            }
            transaction.put(cf::SIGNING_KEYS, &Self::signing_key_key(key.version), key)
        })
    }

    fn activate_signing_key(
        &self,
        version: KeyVersion,
        now: i64,
        previous_verify_until: Option<i64>,
    ) -> SystemResult<Vec<SigningVerificationKey>> {
        self.storage.write_transaction(|transaction| {
            let mut keys = transaction
                .scan_prefix::<SigningVerificationKey>(cf::SIGNING_KEYS, b"signing-key:")?;
            let active_count = keys
                .iter()
                .filter(|(_, key)| key.status == SigningKeyStatus::Active)
                .count();
            if (active_count == 0 && previous_verify_until.is_some())
                || (active_count == 1 && previous_verify_until.is_none())
                || active_count > 1
            {
                return Err(SystemError::InvalidConfiguration(
                    "signing key activation overlap is inconsistent".to_string(),
                ));
            }
            let mut activated = false;
            for (storage_key, key) in &mut keys {
                if key.version == version {
                    *key = transition_signing_key(key, SigningKeyStatus::Active, now, None)
                        .map_err(Self::map_failure)?;
                    activated = true;
                } else if key.status == SigningKeyStatus::Active {
                    *key = transition_signing_key(
                        key,
                        SigningKeyStatus::VerifyOnly,
                        now,
                        previous_verify_until,
                    )
                    .map_err(Self::map_failure)?;
                }
                transaction.put(cf::SIGNING_KEYS, storage_key, key)?;
            }
            if !activated {
                return Err(SystemError::InvalidConfiguration(
                    "prepared signing key version is missing".to_string(),
                ));
            }
            Ok(keys.into_iter().map(|(_, key)| key).collect())
        })
    }

    fn compromise_signing_key(
        &self,
        version: KeyVersion,
        now: i64,
        _correlation_id: CorrelationId,
        _node_id: &str,
    ) -> SystemResult<usize> {
        self.storage.write_transaction(|transaction| {
            let key_storage_key = Self::signing_key_key(version);
            let key: SigningVerificationKey = transaction
                .get(cf::SIGNING_KEYS, &key_storage_key)?
                .ok_or_else(|| {
                    SystemError::InvalidConfiguration("signing key version is missing".to_string())
                })?;
            let compromised =
                transition_signing_key(&key, SigningKeyStatus::Compromised, now, None)
                    .map_err(Self::map_failure)?;
            transaction.put(cf::SIGNING_KEYS, &key_storage_key, &compromised)?;

            let sessions = transaction.scan_prefix::<AuthSession>(cf::SESSIONS, b"session:")?;
            let mut closed_count = 0_usize;
            for (session_key, session) in sessions {
                if session.status != SessionStatus::Active || session.key_version != version {
                    continue;
                }
                let closed = close_session(&session, SessionStatus::Aborted, now)
                    .map_err(Self::map_failure)?;
                let family_key = Self::family_key(&session.family_id);
                if let Some(mut family) =
                    transaction.get::<RefreshFamily>(cf::REFRESH_FAMILIES, &family_key)?
                {
                    family.status = CredentialStatus::Revoked;
                    family.version = family.version.saturating_add(1);
                    transaction.put(cf::REFRESH_FAMILIES, &family_key, &family)?;
                }
                transaction.put(cf::SESSIONS, &session_key, &closed)?;
                closed_count = closed_count.saturating_add(1);
            }
            Ok(closed_count)
        })
    }

    fn api_key(&self, id: &CredentialId) -> SystemResult<Option<ApiKeyRecord>> {
        self.storage.get(cf::API_KEYS, &Self::api_key_key(id))
    }

    fn api_keys(&self) -> SystemResult<Vec<ApiKeyRecord>> {
        self.storage
            .scan_prefix::<ApiKeyRecord>(cf::API_KEYS, b"api-key:")
            .map(|rows| rows.into_iter().map(|(_, record)| record).collect())
    }

    fn service_principal(
        &self,
        id: &CredentialSubjectId,
    ) -> SystemResult<Option<ServicePrincipal>> {
        self.storage
            .get(cf::SERVICE_PRINCIPALS, &Self::service_principal_key(id))
    }

    fn install_api_key(
        &self,
        principal: &ServicePrincipal,
        record: &ApiKeyRecord,
        event: &CredentialEvent,
    ) -> SystemResult<()> {
        if principal.id != record.subject
            || principal.status != CredentialStatus::Active
            || record.status != CredentialStatus::Active
            || event.kind != CredentialEventKind::ApiKeyIssued
            || event.credential_id.as_ref() != Some(&record.id)
        {
            return Err(SystemError::InvalidConfiguration(
                "API key creation state is inconsistent".to_string(),
            ));
        }
        self.storage.write_transaction(|transaction| {
            let principal_key = Self::service_principal_key(&principal.id);
            let api_key = Self::api_key_key(&record.id);
            if transaction
                .get::<ServicePrincipal>(cf::SERVICE_PRINCIPALS, &principal_key)?
                .is_some()
                || transaction
                    .get::<ApiKeyRecord>(cf::API_KEYS, &api_key)?
                    .is_some()
            {
                return Err(SystemError::InvalidConfiguration(
                    "API key or service principal already exists".to_string(),
                ));
            }
            transaction.put(cf::SERVICE_PRINCIPALS, &principal_key, principal)?;
            transaction.put(cf::API_KEYS, &api_key, record)?;
            Ok(())
        })
    }

    fn rotate_api_key(
        &self,
        predecessor_id: &CredentialId,
        successor: &ApiKeyRecord,
        overlap_until: i64,
        now: i64,
        _event: &CredentialEvent,
    ) -> SystemResult<ApiKeyRecord> {
        self.storage.write_transaction(|transaction| {
            let predecessor_key = Self::api_key_key(predecessor_id);
            let mut predecessor: ApiKeyRecord = transaction
                .get(cf::API_KEYS, &predecessor_key)?
                .ok_or(SystemError::Credential(
                    CredentialFailure::InvalidCredential,
                ))?;
            if predecessor.subject != successor.subject
                || predecessor.audience != successor.audience
                || predecessor.scope != successor.scope
                || successor.version != predecessor.version.saturating_add(1)
                || successor.status != CredentialStatus::Active
                || transaction
                    .get::<ApiKeyRecord>(cf::API_KEYS, &Self::api_key_key(&successor.id))?
                    .is_some()
            {
                return Err(SystemError::InvalidConfiguration(
                    "API key rotation state is inconsistent".to_string(),
                ));
            }
            predecessor = transition_api_key(
                &predecessor,
                CredentialStatus::Rotating,
                now,
                Some(overlap_until),
            )
            .map_err(Self::map_failure)?;
            transaction.put(cf::API_KEYS, &predecessor_key, &predecessor)?;
            transaction.put(cf::API_KEYS, &Self::api_key_key(&successor.id), successor)?;
            Ok(predecessor)
        })
    }

    fn revoke_api_key(
        &self,
        id: &CredentialId,
        now: i64,
        _event: &CredentialEvent,
    ) -> SystemResult<ApiKeyRecord> {
        self.storage.write_transaction(|transaction| {
            let key = Self::api_key_key(id);
            let record: ApiKeyRecord =
                transaction
                    .get(cf::API_KEYS, &key)?
                    .ok_or(SystemError::Credential(
                        CredentialFailure::InvalidCredential,
                    ))?;
            let revoked = transition_api_key(&record, CredentialStatus::Revoked, now, None)
                .map_err(Self::map_failure)?;
            transaction.put(cf::API_KEYS, &key, &revoked)?;
            Ok(revoked)
        })
    }

    fn open(
        &self,
        session: &AuthSession,
        family: &RefreshFamily,
        event: &CredentialEvent,
        maximum_subject_sessions: usize,
        maximum_global_sessions: usize,
    ) -> SystemResult<()> {
        if session.status != SessionStatus::Active
            || family.status != CredentialStatus::Active
            || family.session_id != session.id
            || family.id != session.family_id
            || event.kind != CredentialEventKind::SessionOpened
            || event.session_id.as_ref() != Some(&session.id)
        {
            return Err(SystemError::InvalidConfiguration(
                "credential session open transition is inconsistent".to_string(),
            ));
        }
        self.storage.write_transaction(|transaction| {
            let now = session.created_at;
            let sessions = transaction.scan_prefix::<AuthSession>(cf::SESSIONS, b"session:")?;
            let mut subject_active = Vec::new();
            let mut global_active = 0_usize;
            for (key, existing) in sessions {
                if existing.status != SessionStatus::Active
                    || existing.absolute_expires_at <= now
                    || existing.idle_expires_at <= now
                {
                    continue;
                }
                global_active = global_active.saturating_add(1);
                if existing.subject == session.subject {
                    subject_active.push((key, existing));
                }
            }
            // A subject at its cap keeps working: its oldest sessions make room
            // for the new one, so abandoned sessions never lock the account out.
            let evict_count = subject_active
                .len()
                .saturating_add(1)
                .saturating_sub(maximum_subject_sessions);
            subject_active.sort_by_key(|(_, existing)| existing.created_at);
            for (key, existing) in subject_active.into_iter().take(evict_count) {
                let closed = close_session(&existing, SessionStatus::Evicted, now)
                    .map_err(Self::map_failure)?;
                let family_key = Self::family_key(&existing.family_id);
                let family: RefreshFamily = transaction
                    .get(cf::REFRESH_FAMILIES, &family_key)?
                    .ok_or(SystemError::Credential(CredentialFailure::InactiveSession))?;
                let revoked_family = revoke_refresh_family(&family).map_err(Self::map_failure)?;
                transaction.put(cf::SESSIONS, &key, &closed)?;
                transaction.put(cf::REFRESH_FAMILIES, &family_key, &revoked_family)?;
                global_active = global_active.saturating_sub(1);
            }
            if global_active >= maximum_global_sessions {
                return Err(SystemError::Credential(CredentialFailure::Throttled));
            }
            let session_key = Self::session_key(&session.id);
            let family_key = Self::family_key(&family.id);
            if transaction
                .get::<AuthSession>(cf::SESSIONS, &session_key)?
                .is_some()
                || transaction
                    .get::<RefreshFamily>(cf::REFRESH_FAMILIES, &family_key)?
                    .is_some()
            {
                return Err(SystemError::InvalidConfiguration(
                    "credential session or family already exists".to_string(),
                ));
            }
            transaction.put(cf::SESSIONS, &session_key, session)?;
            transaction.put(cf::REFRESH_FAMILIES, &family_key, family)?;
            Ok(())
        })
    }

    fn consume(
        &self,
        family_id: &TokenFamilyId,
        supplied_digest: &SecretDigest,
        successor_token_id: TokenId,
        successor_digest: SecretDigest,
        now: i64,
        idle_expires_at: i64,
        correlation_id: CorrelationId,
        node_id: &str,
    ) -> SystemResult<RefreshTransition> {
        self.storage.write_transaction(|transaction| {
            let family_key = Self::family_key(family_id);
            let family: RefreshFamily = transaction.get(cf::REFRESH_FAMILIES, &family_key)?.ok_or(
                SystemError::Credential(CredentialFailure::InvalidCredential),
            )?;
            let session_key = Self::session_key(&family.session_id);
            let session: AuthSession = transaction
                .get(cf::SESSIONS, &session_key)?
                .ok_or(SystemError::Credential(CredentialFailure::InactiveSession))?;
            let transition = consume_refresh(
                &family,
                &session,
                supplied_digest,
                successor_token_id.clone(),
                successor_digest.clone(),
                now,
                idle_expires_at,
                correlation_id.clone(),
                node_id,
            )
            .map_err(Self::map_failure)?;
            match &transition {
                RefreshTransition::Rotated {
                    family,
                    session,
                    event: _,
                } => {
                    transaction.put(cf::REFRESH_FAMILIES, &family_key, family)?;
                    transaction.put(cf::SESSIONS, &session_key, session)?;
                }
                RefreshTransition::ReplayRevoked {
                    family,
                    session,
                    event: _,
                } => {
                    transaction.put(cf::REFRESH_FAMILIES, &family_key, family)?;
                    transaction.put(cf::SESSIONS, &session_key, session)?;
                }
            }
            Ok(transition)
        })
    }

    fn close(
        &self,
        session_id: &SessionId,
        reason: SessionStatus,
        now: i64,
        _event: &CredentialEvent,
    ) -> SystemResult<AuthSession> {
        self.storage.write_transaction(|transaction| {
            let session_key = Self::session_key(session_id);
            let session: AuthSession = transaction
                .get(cf::SESSIONS, &session_key)?
                .ok_or(SystemError::Credential(CredentialFailure::InactiveSession))?;
            let closed = close_session(&session, reason, now).map_err(Self::map_failure)?;
            let family_key = Self::family_key(&session.family_id);
            let family: RefreshFamily = transaction
                .get(cf::REFRESH_FAMILIES, &family_key)?
                .ok_or(SystemError::Credential(CredentialFailure::InactiveSession))?;
            let revoked_family = revoke_refresh_family(&family).map_err(Self::map_failure)?;
            transaction.put(cf::SESSIONS, &session_key, &closed)?;
            transaction.put(cf::REFRESH_FAMILIES, &family_key, &revoked_family)?;
            Ok(closed)
        })
    }

    fn close_subject_sessions(
        &self,
        subject: &CredentialSubjectId,
        reason: SessionStatus,
        now: i64,
        _correlation_id: &CorrelationId,
        _node_id: &str,
    ) -> SystemResult<usize> {
        self.storage.write_transaction(|transaction| {
            let sessions = transaction.scan_prefix::<AuthSession>(cf::SESSIONS, b"session:")?;
            let mut closed_count = 0_usize;
            for (key, session) in sessions {
                if session.subject != *subject || session.status != SessionStatus::Active {
                    continue;
                }
                let closed = close_session(&session, reason, now).map_err(Self::map_failure)?;
                let family_key = Self::family_key(&session.family_id);
                let family: RefreshFamily = transaction
                    .get(cf::REFRESH_FAMILIES, &family_key)?
                    .ok_or(SystemError::Credential(CredentialFailure::InactiveSession))?;
                let revoked_family = revoke_refresh_family(&family).map_err(Self::map_failure)?;
                transaction.put(cf::SESSIONS, &key, &closed)?;
                transaction.put(cf::REFRESH_FAMILIES, &family_key, &revoked_family)?;
                closed_count = closed_count.saturating_add(1);
            }
            Ok(closed_count)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialRuntimePolicy {
    pub issuer: String,
    pub access_ttl_seconds: i64,
    pub refresh_ttl_seconds: i64,
    pub session_absolute_ttl_seconds: i64,
    pub session_idle_ttl_seconds: i64,
    pub clock_skew_seconds: i64,
    pub maximum_active_sessions_per_subject: usize,
    pub maximum_active_sessions_global: usize,
    pub maximum_admission_buckets: usize,
    pub authentication_attempts_per_minute: u32,
    pub maximum_concurrent_secret_verifications: usize,
}

impl CredentialRuntimePolicy {
    pub fn validate(&self) -> Result<(), CredentialFailure> {
        if self.issuer.is_empty()
            || self.access_ttl_seconds <= 0
            || self.refresh_ttl_seconds < self.access_ttl_seconds
            || self.session_absolute_ttl_seconds < self.refresh_ttl_seconds
            || self.session_idle_ttl_seconds < self.access_ttl_seconds
            || self.session_idle_ttl_seconds > self.session_absolute_ttl_seconds
            || self.clock_skew_seconds < 0
            || self.clock_skew_seconds > self.access_ttl_seconds
            || self.maximum_active_sessions_per_subject == 0
            || self.maximum_active_sessions_per_subject > 10_000
            || self.maximum_active_sessions_global < self.maximum_active_sessions_per_subject
            || self.maximum_active_sessions_global > 1_000_000
            || self.maximum_admission_buckets == 0
            || self.maximum_admission_buckets > 1_000_000
            || self.authentication_attempts_per_minute == 0
            || self.maximum_concurrent_secret_verifications == 0
            || self.maximum_concurrent_secret_verifications > 10_000
        {
            return Err(CredentialFailure::Policy);
        }
        Ok(())
    }
}

impl Default for CredentialRuntimePolicy {
    fn default() -> Self {
        Self {
            issuer: "zeta4j".to_string(),
            access_ttl_seconds: 900,
            refresh_ttl_seconds: 7 * 24 * 60 * 60,
            session_absolute_ttl_seconds: 7 * 24 * 60 * 60,
            session_idle_ttl_seconds: 24 * 60 * 60,
            clock_skew_seconds: 30,
            maximum_active_sessions_per_subject: 16,
            maximum_active_sessions_global: 10_000,
            maximum_admission_buckets: 4096,
            authentication_attempts_per_minute: 30,
            maximum_concurrent_secret_verifications: 32,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AdmissionBucket {
    window_started_at: i64,
    attempts: u32,
    last_seen_at: i64,
}

struct CredentialAdmissionController {
    buckets: Mutex<HashMap<SecretDigest, AdmissionBucket>>,
    overflow_bucket: Mutex<AdmissionBucket>,
    active_secret_verifications: Arc<AtomicUsize>,
    maximum_buckets: usize,
    attempts_per_minute: u32,
    maximum_concurrent_secret_verifications: usize,
}

impl CredentialAdmissionController {
    fn new(policy: &CredentialRuntimePolicy) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            overflow_bucket: Mutex::new(AdmissionBucket {
                window_started_at: 0,
                attempts: 0,
                last_seen_at: 0,
            }),
            active_secret_verifications: Arc::new(AtomicUsize::new(0)),
            maximum_buckets: policy.maximum_admission_buckets,
            attempts_per_minute: policy.authentication_attempts_per_minute.max(1),
            maximum_concurrent_secret_verifications: policy
                .maximum_concurrent_secret_verifications
                .max(1),
        }
    }

    fn admit_secret_verification(
        &self,
        domain: &[u8],
        opaque_input: &str,
        now: i64,
    ) -> Result<SecretVerificationPermit, CredentialFailure> {
        self.admit_action(domain, opaque_input, now)?;

        let incremented = self
            .active_secret_verifications
            .try_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.maximum_concurrent_secret_verifications)
                    .then_some(active.saturating_add(1))
            })
            .is_ok();
        if !incremented {
            return Err(CredentialFailure::Throttled);
        }
        Ok(SecretVerificationPermit {
            active: self.active_secret_verifications.clone(),
        })
    }

    fn admit_action(
        &self,
        domain: &[u8],
        opaque_input: &str,
        now: i64,
    ) -> Result<(), CredentialFailure> {
        let bucket_key = digest_secret(domain, &CredentialSecret::new(opaque_input));
        {
            let mut buckets = self
                .buckets
                .lock()
                .map_err(|_| CredentialFailure::Internal)?;
            if !buckets.contains_key(&bucket_key) && buckets.len() >= self.maximum_buckets {
                drop(buckets);
                let mut overflow = self
                    .overflow_bucket
                    .lock()
                    .map_err(|_| CredentialFailure::Internal)?;
                Self::consume_bucket(&mut overflow, now, self.attempts_per_minute)?;
            } else {
                let bucket = buckets.entry(bucket_key).or_insert(AdmissionBucket {
                    window_started_at: now,
                    attempts: 0,
                    last_seen_at: now,
                });
                Self::consume_bucket(bucket, now, self.attempts_per_minute)?;
            }
        }

        Ok(())
    }

    fn consume_bucket(
        bucket: &mut AdmissionBucket,
        now: i64,
        attempts_per_minute: u32,
    ) -> Result<(), CredentialFailure> {
        if now.saturating_sub(bucket.window_started_at) >= 60 {
            bucket.window_started_at = now;
            bucket.attempts = 0;
        }
        bucket.last_seen_at = now;
        if bucket.attempts >= attempts_per_minute {
            return Err(CredentialFailure::Throttled);
        }
        bucket.attempts = bucket.attempts.saturating_add(1);
        Ok(())
    }

    #[cfg(test)]
    fn retained_bucket_count(&self) -> Result<usize, CredentialFailure> {
        self.buckets
            .lock()
            .map(|buckets| buckets.len())
            .map_err(|_| CredentialFailure::Internal)
    }
}

struct SecretVerificationPermit {
    active: Arc<AtomicUsize>,
}

impl Drop for SecretVerificationPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct IssuedTokenPair {
    access_token: CredentialSecret,
    refresh_token: CredentialSecret,
    pub access_expires_at: i64,
    pub refresh_expires_at: i64,
    pub session_id: SessionId,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedPasswordSession {
    pub user: User,
    pub session_id: SessionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionDescriptor {
    pub id: SessionId,
    pub subject: CredentialSubjectId,
    pub credential_kind: CredentialKind,
    pub created_at: i64,
    pub last_refreshed_at: i64,
    pub absolute_expires_at: i64,
    pub idle_expires_at: i64,
    pub security_generation: u64,
    pub key_version: KeyVersion,
    pub family_id: TokenFamilyId,
    pub status: SessionStatus,
}

impl IssuedTokenPair {
    pub fn access_token(&self) -> &CredentialSecret {
        &self.access_token
    }

    pub fn refresh_token(&self) -> &CredentialSecret {
        &self.refresh_token
    }
}

impl std::fmt::Debug for IssuedTokenPair {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IssuedTokenPair")
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .field("session_id", &self.session_id)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApiKeyDescriptor {
    pub id: CredentialId,
    pub subject: CredentialSubjectId,
    pub name: String,
    pub issuer: String,
    pub audience: CredentialAudience,
    pub scope: BTreeSet<CredentialScope>,
    pub created_at: i64,
    pub not_before: i64,
    pub expires_at: i64,
    pub version: u64,
    pub status: CredentialStatus,
    pub overlap_until: Option<i64>,
}

pub struct IssuedApiKey {
    secret: CredentialSecret,
    pub descriptor: ApiKeyDescriptor,
}

pub struct PreparedApiKeyIssue {
    issued: IssuedApiKey,
    principal: ServicePrincipal,
    record: ApiKeyRecord,
    event: CredentialEvent,
}

impl PreparedApiKeyIssue {
    pub fn into_parts(
        self,
    ) -> (
        IssuedApiKey,
        ServicePrincipal,
        ApiKeyRecord,
        CredentialEvent,
    ) {
        (self.issued, self.principal, self.record, self.event)
    }
}

impl std::fmt::Debug for PreparedApiKeyIssue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedApiKeyIssue")
            .field("issued", &self.issued)
            .field("principal", &self.principal)
            .field("record", &self.record)
            .field("event", &self.event)
            .finish()
    }
}

pub struct PreparedApiKeyRotation {
    issued: IssuedApiKey,
    predecessor_id: CredentialId,
    successor: ApiKeyRecord,
    overlap_until: i64,
    occurred_at: i64,
    event: CredentialEvent,
}

impl PreparedApiKeyRotation {
    pub fn into_parts(
        self,
    ) -> (
        IssuedApiKey,
        CredentialId,
        ApiKeyRecord,
        i64,
        i64,
        CredentialEvent,
    ) {
        (
            self.issued,
            self.predecessor_id,
            self.successor,
            self.overlap_until,
            self.occurred_at,
            self.event,
        )
    }
}

impl std::fmt::Debug for PreparedApiKeyRotation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedApiKeyRotation")
            .field("issued", &self.issued)
            .field("predecessor_id", &self.predecessor_id)
            .field("successor", &self.successor)
            .field("overlap_until", &self.overlap_until)
            .field("occurred_at", &self.occurred_at)
            .field("event", &self.event)
            .finish()
    }
}

impl IssuedApiKey {
    pub fn secret(&self) -> &CredentialSecret {
        &self.secret
    }
}

impl std::fmt::Debug for IssuedApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IssuedApiKey")
            .field("secret", &"[REDACTED]")
            .field("descriptor", &self.descriptor)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedApiKey {
    pub principal: ServicePrincipal,
    pub descriptor: ApiKeyDescriptor,
}

pub struct LocalSecurityService {
    security: Arc<SecurityManager>,
    store: Arc<dyn CredentialStore>,
    clock: Arc<dyn CredentialClock>,
    random: Arc<dyn CredentialRandom>,
    signer: Arc<dyn CredentialSigner>,
    api_key_dummy_hash: PasswordHash,
    policy: CredentialRuntimePolicy,
    admission: CredentialAdmissionController,
    node_id: String,
}

impl std::fmt::Debug for LocalSecurityService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalSecurityService")
            .field("node_id", &self.node_id)
            .finish_non_exhaustive()
    }
}

impl LocalSecurityService {
    pub fn new(
        security: Arc<SecurityManager>,
        store: Arc<dyn CredentialStore>,
        clock: Arc<dyn CredentialClock>,
        random: Arc<dyn CredentialRandom>,
        signer: Arc<dyn CredentialSigner>,
        policy: CredentialRuntimePolicy,
        node_id: impl Into<String>,
    ) -> Result<Self, CredentialFailure> {
        policy.validate()?;
        let node_id = node_id.into();
        if node_id.is_empty() {
            return Err(CredentialFailure::Policy);
        }
        let api_key_dummy_hash = PasswordPolicy::default().hash(&CredentialSecret::new(
            "z4g_cred_00000000000000000000000000000000.DummyApiKeyMaterialForConstantWork234",
        ))?;
        Ok(Self {
            security,
            store,
            clock,
            random,
            signer,
            api_key_dummy_hash,
            admission: CredentialAdmissionController::new(&policy),
            policy,
            node_id,
        })
    }

    pub fn store(&self) -> &Arc<dyn CredentialStore> {
        &self.store
    }

    pub fn clock(&self) -> &Arc<dyn CredentialClock> {
        &self.clock
    }

    pub fn random(&self) -> &Arc<dyn CredentialRandom> {
        &self.random
    }

    pub fn signer(&self) -> &Arc<dyn CredentialSigner> {
        &self.signer
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn local_signing_readiness(&self) -> SigningVerificationKey {
        self.signer
            .verification_key(SigningKeyStatus::Prepared, self.clock.now())
    }

    pub fn local_signer_status(&self) -> Result<SigningKeyStatus, CredentialFailure> {
        let local = self.local_signing_readiness();
        self.store
            .signing_keys()
            .map_err(Self::map_system_failure)?
            .into_iter()
            .find(|key| {
                key.version == local.version
                    && key.key_id == local.key_id
                    && key.public_key == local.public_key
            })
            .map(|key| key.status)
            .ok_or(CredentialFailure::KeyVersion)
    }

    pub fn signing_keys(&self) -> Result<Vec<SigningVerificationKey>, CredentialFailure> {
        let mut keys = self
            .store
            .signing_keys()
            .map_err(Self::map_system_failure)?;
        keys.sort_by_key(|key| key.version);
        Ok(keys)
    }

    pub fn prepare_signing_key_compromise(&self, version: KeyVersion) -> SecurityMutation {
        SecurityMutation::CompromiseSigningKey {
            version,
            occurred_at: self.clock.now(),
            correlation_id: self.random.correlation_id(),
            node_id: self.node_id.clone(),
        }
    }

    pub fn synchronize_token_verifier(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
    ) -> Result<(), CredentialFailure> {
        let keys = self
            .store
            .signing_keys()
            .map_err(Self::map_system_failure)?;
        token_service.replace_verification_keys(keys)
    }

    pub fn retire_expired_signing_keys(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
    ) -> Result<bool, CredentialFailure> {
        let now = self.clock.now();
        let has_expired = self
            .store
            .signing_keys()
            .map_err(Self::map_system_failure)?
            .iter()
            .any(|key| {
                key.status == SigningKeyStatus::VerifyOnly
                    && key.verify_until.is_some_and(|deadline| deadline <= now)
            });
        if !has_expired {
            return Ok(false);
        }
        let outcome = self
            .store
            .apply_security_mutation(SecurityMutation::RetireSigningKeys { occurred_at: now })
            .map_err(Self::map_system_failure)?;
        Self::require_applied(outcome)?;
        self.synchronize_token_verifier(token_service)?;
        Ok(true)
    }

    fn prepare_admission_mutation(
        &self,
        domain: &[u8],
        opaque_input: &CredentialSecret,
    ) -> (SecurityMutation, CorrelationId) {
        let correlation_id = self.random.correlation_id();
        (
            SecurityMutation::ConsumeAdmission {
                bucket_key: digest_secret(domain, opaque_input),
                occurred_at: self.clock.now(),
                correlation_id: correlation_id.clone(),
                maximum_buckets: self.policy.maximum_admission_buckets,
                attempts_per_minute: self.policy.authentication_attempts_per_minute,
            },
            correlation_id,
        )
    }

    fn admit_durable_action(
        &self,
        domain: &[u8],
        opaque_input: &CredentialSecret,
    ) -> Result<(), CredentialFailure> {
        let (mutation, _) = self.prepare_admission_mutation(domain, opaque_input);
        let outcome = self
            .store
            .apply_security_mutation(mutation)
            .map_err(Self::map_system_failure)?;
        Self::require_admission_granted(outcome)
    }

    pub fn login(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        username: &str,
        password: &str,
        audience: CredentialAudience,
    ) -> Result<IssuedTokenPair, CredentialFailure> {
        self.synchronize_token_verifier(token_service)?;
        token_service.ensure_active_signer()?;
        self.admit_durable_action(b"password-account", &CredentialSecret::new(username))?;
        let (issued, mutation) = self.prepare_login(token_service, username, password, audience)?;
        let outcome = self
            .store
            .apply_security_mutation(mutation)
            .map_err(Self::map_system_failure)?;
        Self::require_applied(outcome)?;
        Ok(issued)
    }

    fn prepare_login(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        username: &str,
        password: &str,
        audience: CredentialAudience,
    ) -> Result<(IssuedTokenPair, SecurityMutation), CredentialFailure> {
        let _admission = self.admission.admit_secret_verification(
            b"password-account",
            username,
            self.clock.now(),
        )?;
        let user = self
            .security
            .authenticate(username, password)
            .map_err(Self::map_password_auth_failure)?;
        self.ensure_session_admission(&user)?;
        let now = self.clock.now();
        let session_id = self.random.session_id();
        let family_id = self.random.family_id();
        let access_token_id = self.random.token_id();
        let refresh_token_id = self.random.token_id();
        let access_expires_at = now
            .checked_add(self.policy.access_ttl_seconds)
            .ok_or(CredentialFailure::Policy)?;
        let refresh_expires_at = now
            .checked_add(self.policy.refresh_ttl_seconds)
            .ok_or(CredentialFailure::Policy)?;
        let absolute_expires_at = now
            .checked_add(self.policy.session_absolute_ttl_seconds)
            .ok_or(CredentialFailure::Policy)?;
        let idle_expires_at = now
            .checked_add(self.policy.session_idle_ttl_seconds)
            .ok_or(CredentialFailure::Policy)?
            .min(absolute_expires_at);
        let access_claims = self.access_claims(
            &user,
            audience,
            &session_id,
            &family_id,
            access_token_id,
            now,
            access_expires_at,
        );
        let refresh_claims = self.refresh_claims(
            &user,
            audience,
            &session_id,
            &family_id,
            refresh_token_id.clone(),
            now,
            refresh_expires_at,
        );
        let access_token = token_service.sign_access(&access_claims)?;
        let refresh_token = token_service.sign_refresh(&refresh_claims)?;
        let refresh_digest = digest_secret(b"refresh-token", &refresh_token);
        let session = AuthSession {
            id: session_id.clone(),
            subject: user.principal_id.clone(),
            credential_kind: CredentialKind::Password,
            created_at: now,
            last_refreshed_at: now,
            absolute_expires_at,
            idle_expires_at,
            security_generation: user.security_generation,
            key_version: self.signer.key_version(),
            family_id: family_id.clone(),
            status: SessionStatus::Active,
            closed_at: None,
        };
        let family = RefreshFamily {
            id: family_id.clone(),
            session_id: session_id.clone(),
            current_token_id: refresh_token_id,
            current_digest: refresh_digest,
            consumed_digests: Vec::new(),
            status: CredentialStatus::Active,
            expires_at: refresh_expires_at,
            version: 1,
        };
        let event = CredentialEvent {
            kind: CredentialEventKind::SessionOpened,
            subject: user.principal_id.clone(),
            session_id: Some(session_id.clone()),
            family_id: Some(family_id),
            credential_id: None,
            key_version: Some(self.signer.key_version()),
            correlation_id: self.random.correlation_id(),
            node_id: self.node_id.clone(),
            occurred_at: now,
        };
        let issued = IssuedTokenPair {
            access_token,
            refresh_token,
            access_expires_at,
            refresh_expires_at,
            session_id,
        };
        Ok((
            issued,
            SecurityMutation::OpenSession {
                session,
                family,
                event,
                maximum_subject_sessions: self.policy.maximum_active_sessions_per_subject,
                maximum_global_sessions: self.policy.maximum_active_sessions_global,
            },
        ))
    }

    /// Authenticate a stateful protocol connection without creating bearer
    /// credentials. The returned session is durable and must be closed by the
    /// adapter on LOGOFF or connection termination.
    pub fn open_password_session(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AuthenticatedPasswordSession, CredentialFailure> {
        self.admit_durable_action(b"password-account", &CredentialSecret::new(username))?;
        let (authenticated, mutation) = self.prepare_password_session(username, password, None)?;
        let outcome = self
            .store
            .apply_security_mutation(mutation)
            .map_err(Self::map_system_failure)?;
        Self::require_applied(outcome)?;
        Ok(authenticated)
    }

    pub fn open_password_session_from_client(
        &self,
        username: &str,
        password: &str,
        client_address: &str,
    ) -> Result<AuthenticatedPasswordSession, CredentialFailure> {
        self.admission.admit_action(
            b"password-client-address",
            client_address,
            self.clock.now(),
        )?;
        self.open_password_session(username, password)
    }

    fn prepare_password_session(
        &self,
        username: &str,
        password: &str,
        client_address: Option<&str>,
    ) -> Result<(AuthenticatedPasswordSession, SecurityMutation), CredentialFailure> {
        if let Some(client_address) = client_address {
            self.admission.admit_action(
                b"password-client-address",
                client_address,
                self.clock.now(),
            )?;
        }
        let _admission = self.admission.admit_secret_verification(
            b"password-account",
            username,
            self.clock.now(),
        )?;
        let user = self
            .security
            .authenticate(username, password)
            .map_err(Self::map_password_auth_failure)?;
        self.ensure_session_admission(&user)?;
        let now = self.clock.now();
        let session_id = self.random.session_id();
        let family_id = self.random.family_id();
        let absolute_expires_at = now
            .checked_add(self.policy.session_absolute_ttl_seconds)
            .ok_or(CredentialFailure::Policy)?;
        let idle_expires_at = now
            .checked_add(self.policy.session_idle_ttl_seconds)
            .ok_or(CredentialFailure::Policy)?
            .min(absolute_expires_at);
        let nonce = self.random.secret(32).map_err(Self::map_system_failure)?;
        let session = AuthSession {
            id: session_id.clone(),
            subject: user.principal_id.clone(),
            credential_kind: CredentialKind::Password,
            created_at: now,
            last_refreshed_at: now,
            absolute_expires_at,
            idle_expires_at,
            security_generation: user.security_generation,
            key_version: self.signer.key_version(),
            family_id: family_id.clone(),
            status: SessionStatus::Active,
            closed_at: None,
        };
        let family = RefreshFamily {
            id: family_id.clone(),
            session_id: session_id.clone(),
            current_token_id: self.random.token_id(),
            current_digest: digest_secret(b"stateful-session", &nonce),
            consumed_digests: Vec::new(),
            status: CredentialStatus::Active,
            expires_at: absolute_expires_at,
            version: 1,
        };
        let event = CredentialEvent {
            kind: CredentialEventKind::SessionOpened,
            subject: user.principal_id.clone(),
            session_id: Some(session_id.clone()),
            family_id: Some(family_id),
            credential_id: None,
            key_version: Some(self.signer.key_version()),
            correlation_id: self.random.correlation_id(),
            node_id: self.node_id.clone(),
            occurred_at: now,
        };
        Ok((
            AuthenticatedPasswordSession { user, session_id },
            SecurityMutation::OpenSession {
                session,
                family,
                event,
                maximum_subject_sessions: self.policy.maximum_active_sessions_per_subject,
                maximum_global_sessions: self.policy.maximum_active_sessions_global,
            },
        ))
    }

    pub fn validate_password_session(
        &self,
        session_id: &SessionId,
        subject: &CredentialSubjectId,
        security_generation: u64,
    ) -> Result<(), CredentialFailure> {
        let session = self
            .store
            .session(session_id)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InactiveSession)?;
        session.validate_active(subject, security_generation, self.clock.now())
    }

    pub fn close_password_session(&self, session_id: &SessionId) -> Result<(), CredentialFailure> {
        let mutation = self.prepare_close_session(session_id)?;
        let outcome = self
            .store
            .apply_security_mutation(mutation)
            .map_err(Self::map_system_failure)?;
        Self::require_applied(outcome)
    }

    fn prepare_close_session(
        &self,
        session_id: &SessionId,
    ) -> Result<SecurityMutation, CredentialFailure> {
        let (subject, reason, occurred_at, event) = self.prepare_session_revocation(session_id)?;
        Ok(SecurityMutation::CloseSession {
            id: session_id.clone(),
            subject,
            reason,
            occurred_at,
            event,
        })
    }

    pub fn validate_access(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        token: &CredentialSecret,
        audience: CredentialAudience,
    ) -> Result<(ValidatedClaims, User), CredentialFailure> {
        let validation = ClaimValidation::new(
            self.policy.issuer.clone(),
            audience,
            TokenType::Access,
            CredentialScope::Access,
            self.policy.access_ttl_seconds,
            self.policy.clock_skew_seconds,
        )?;
        let now = self.clock.now();
        let claims = token_service.validate(token, &validation, now)?;
        let session = self
            .store
            .session(&claims.session_id)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InactiveSession)?;
        let user = self
            .security
            .get_user_by_principal(&claims.subject)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InvalidCredential)?;
        if user.suspended || user.password_change_required {
            return Err(CredentialFailure::InvalidCredential);
        }
        session.validate_active(&claims.subject, user.security_generation, now)?;
        if claims.security_generation != user.security_generation
            || session.family_id != claims.family_id
        {
            return Err(CredentialFailure::PrincipalChanged);
        }
        Ok((claims, user))
    }

    pub fn refresh(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        refresh_token: &CredentialSecret,
        audience: CredentialAudience,
    ) -> Result<IssuedTokenPair, CredentialFailure> {
        self.synchronize_token_verifier(token_service)?;
        self.admit_durable_action(b"refresh-token", refresh_token)?;
        let (issued, mutation) = self.prepare_refresh(token_service, refresh_token, audience)?;
        let outcome = self
            .store
            .apply_security_mutation(mutation)
            .map_err(Self::map_system_failure)?;
        Self::require_refresh_rotated(outcome)?;
        Ok(issued)
    }

    fn prepare_refresh(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        refresh_token: &CredentialSecret,
        audience: CredentialAudience,
    ) -> Result<(IssuedTokenPair, SecurityMutation), CredentialFailure> {
        let validation = ClaimValidation::new(
            self.policy.issuer.clone(),
            audience,
            TokenType::Refresh,
            CredentialScope::Refresh,
            self.policy.refresh_ttl_seconds,
            self.policy.clock_skew_seconds,
        )?;
        let now = self.clock.now();
        let claims = token_service.validate(refresh_token, &validation, now)?;
        self.admission
            .admit_action(b"refresh-family", claims.family_id.as_str(), now)?;
        let session = self
            .store
            .session(&claims.session_id)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InactiveSession)?;
        let user = self
            .security
            .get_user_by_principal(&claims.subject)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InvalidCredential)?;
        session.validate_active(&claims.subject, user.security_generation, now)?;
        if claims.security_generation != user.security_generation
            || claims.family_id != session.family_id
        {
            return Err(CredentialFailure::PrincipalChanged);
        }
        let successor_refresh_id = self.random.token_id();
        let successor_access_id = self.random.token_id();
        let access_expires_at = now
            .checked_add(self.policy.access_ttl_seconds)
            .ok_or(CredentialFailure::Policy)?
            .min(session.absolute_expires_at);
        let refresh_expires_at = session.absolute_expires_at;
        let access_claims = self.access_claims(
            &user,
            audience,
            &session.id,
            &session.family_id,
            successor_access_id,
            now,
            access_expires_at,
        );
        let refresh_claims = self.refresh_claims(
            &user,
            audience,
            &session.id,
            &session.family_id,
            successor_refresh_id.clone(),
            now,
            refresh_expires_at,
        );
        let successor_access = token_service.sign_access(&access_claims)?;
        let successor_refresh = token_service.sign_refresh(&refresh_claims)?;
        let successor_digest = digest_secret(b"refresh-token", &successor_refresh);
        let supplied_digest = digest_secret(b"refresh-token", refresh_token);
        let idle_expires_at = now
            .checked_add(self.policy.session_idle_ttl_seconds)
            .ok_or(CredentialFailure::Policy)?
            .min(session.absolute_expires_at);
        let issued = IssuedTokenPair {
            access_token: successor_access,
            refresh_token: successor_refresh,
            access_expires_at,
            refresh_expires_at,
            session_id: session.id,
        };
        let family_id = claims.family_id;
        Ok((
            issued,
            SecurityMutation::ConsumeRefresh {
                family_id: family_id.clone(),
                supplied_digest,
                successor_token_id: successor_refresh_id.clone(),
                successor_digest,
                occurred_at: now,
                idle_expires_at,
                correlation_id: self.random.correlation_id(),
                node_id: self.node_id.clone(),
            },
        ))
    }

    fn require_applied(outcome: SecurityMutationOutcome) -> Result<(), CredentialFailure> {
        if outcome == SecurityMutationOutcome::Applied {
            Ok(())
        } else {
            Err(CredentialFailure::Internal)
        }
    }

    fn require_admission_granted(
        outcome: SecurityMutationOutcome,
    ) -> Result<(), CredentialFailure> {
        match outcome {
            SecurityMutationOutcome::AdmissionGranted => Ok(()),
            SecurityMutationOutcome::AdmissionThrottled => Err(CredentialFailure::Throttled),
            SecurityMutationOutcome::Applied
            | SecurityMutationOutcome::RefreshRotated
            | SecurityMutationOutcome::RefreshReplayRevoked => Err(CredentialFailure::Internal),
        }
    }

    fn require_refresh_rotated(outcome: SecurityMutationOutcome) -> Result<(), CredentialFailure> {
        match outcome {
            SecurityMutationOutcome::RefreshRotated => Ok(()),
            SecurityMutationOutcome::RefreshReplayRevoked => {
                Err(CredentialFailure::InvalidCredential)
            }
            SecurityMutationOutcome::Applied
            | SecurityMutationOutcome::AdmissionGranted
            | SecurityMutationOutcome::AdmissionThrottled => Err(CredentialFailure::Internal),
        }
    }

    pub fn logout(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        access_token: &CredentialSecret,
        audience: CredentialAudience,
    ) -> Result<(), CredentialFailure> {
        let mutation = self.prepare_logout(token_service, access_token, audience)?;
        let outcome = self
            .store
            .apply_security_mutation(mutation)
            .map_err(Self::map_system_failure)?;
        Self::require_applied(outcome)
    }

    fn prepare_logout(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        access_token: &CredentialSecret,
        audience: CredentialAudience,
    ) -> Result<SecurityMutation, CredentialFailure> {
        let (claims, user) = self.validate_access(token_service, access_token, audience)?;
        let now = self.clock.now();
        let event = CredentialEvent {
            kind: CredentialEventKind::SessionClosed,
            subject: user.principal_id.clone(),
            session_id: Some(claims.session_id.clone()),
            family_id: Some(claims.family_id),
            credential_id: None,
            key_version: Some(claims.key_version),
            correlation_id: self.random.correlation_id(),
            node_id: self.node_id.clone(),
            occurred_at: now,
        };
        Ok(SecurityMutation::CloseSession {
            id: claims.session_id,
            subject: user.principal_id,
            reason: SessionStatus::LoggedOut,
            occurred_at: now,
            event,
        })
    }

    pub fn list_sessions(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        access_token: &CredentialSecret,
        audience: CredentialAudience,
    ) -> Result<Vec<SessionDescriptor>, CredentialFailure> {
        let (claims, _) = self.validate_access(token_service, access_token, audience)?;
        let mut sessions = self
            .store
            .sessions_for_subject(&claims.subject)
            .map_err(Self::map_system_failure)?
            .into_iter()
            .map(|session| SessionDescriptor {
                id: session.id,
                subject: session.subject,
                credential_kind: session.credential_kind,
                created_at: session.created_at,
                last_refreshed_at: session.last_refreshed_at,
                absolute_expires_at: session.absolute_expires_at,
                idle_expires_at: session.idle_expires_at,
                security_generation: session.security_generation,
                key_version: session.key_version,
                family_id: session.family_id,
                status: session.status,
            })
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.created_at);
        Ok(sessions)
    }

    pub fn revoke_session(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        access_token: &CredentialSecret,
        audience: CredentialAudience,
        target_session_id: &SessionId,
    ) -> Result<(), CredentialFailure> {
        let mutation = self.prepare_owned_session_revocation(
            token_service,
            access_token,
            audience,
            target_session_id,
        )?;
        let outcome = self
            .store
            .apply_security_mutation(mutation)
            .map_err(Self::map_system_failure)?;
        Self::require_applied(outcome)
    }

    fn prepare_owned_session_revocation(
        &self,
        token_service: &crate::server::auth::CredentialTokenService,
        access_token: &CredentialSecret,
        audience: CredentialAudience,
        target_session_id: &SessionId,
    ) -> Result<SecurityMutation, CredentialFailure> {
        let (claims, _) = self.validate_access(token_service, access_token, audience)?;
        let target = self
            .store
            .session(target_session_id)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InactiveSession)?;
        if target.subject != claims.subject {
            return Err(CredentialFailure::InvalidCredential);
        }
        let now = self.clock.now();
        let event = CredentialEvent {
            kind: CredentialEventKind::SessionClosed,
            subject: target.subject,
            session_id: Some(target.id.clone()),
            family_id: Some(target.family_id),
            credential_id: None,
            key_version: Some(target.key_version),
            correlation_id: self.random.correlation_id(),
            node_id: self.node_id.clone(),
            occurred_at: now,
        };
        Ok(SecurityMutation::CloseSession {
            id: target_session_id.clone(),
            subject: claims.subject,
            reason: SessionStatus::LoggedOut,
            occurred_at: now,
            event,
        })
    }

    pub fn list_all_sessions(&self) -> Result<Vec<SessionDescriptor>, CredentialFailure> {
        let mut sessions = self
            .store
            .sessions()
            .map_err(Self::map_system_failure)?
            .into_iter()
            .map(|session| SessionDescriptor {
                id: session.id,
                subject: session.subject,
                credential_kind: session.credential_kind,
                created_at: session.created_at,
                last_refreshed_at: session.last_refreshed_at,
                absolute_expires_at: session.absolute_expires_at,
                idle_expires_at: session.idle_expires_at,
                security_generation: session.security_generation,
                key_version: session.key_version,
                family_id: session.family_id,
                status: session.status,
            })
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| session.created_at);
        Ok(sessions)
    }

    pub fn revoke_any_session(
        &self,
        target_session_id: &SessionId,
    ) -> Result<(), CredentialFailure> {
        let (_, reason, now, event) = self.prepare_session_revocation(target_session_id)?;
        self.store
            .close(target_session_id, reason, now, &event)
            .map_err(Self::map_system_failure)?;
        Ok(())
    }

    pub fn prepare_session_revocation(
        &self,
        target_session_id: &SessionId,
    ) -> Result<(CredentialSubjectId, SessionStatus, i64, CredentialEvent), CredentialFailure> {
        let target = self
            .store
            .session(target_session_id)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InactiveSession)?;
        let now = self.clock.now();
        let event = CredentialEvent {
            kind: CredentialEventKind::SessionClosed,
            subject: target.subject.clone(),
            session_id: Some(target.id),
            family_id: Some(target.family_id),
            credential_id: None,
            key_version: Some(target.key_version),
            correlation_id: self.random.correlation_id(),
            node_id: self.node_id.clone(),
            occurred_at: now,
        };
        Ok((target.subject, SessionStatus::LoggedOut, now, event))
    }

    pub fn create_api_key(
        &self,
        name: &str,
        audience: CredentialAudience,
        scope: BTreeSet<CredentialScope>,
        expires_at: i64,
    ) -> Result<IssuedApiKey, CredentialFailure> {
        let prepared = self.prepare_api_key(name, audience, scope, expires_at)?;
        let (issued, principal, record, event) = prepared.into_parts();
        self.store
            .install_api_key(&principal, &record, &event)
            .map_err(Self::map_system_failure)?;
        Ok(issued)
    }

    pub fn prepare_api_key(
        &self,
        name: &str,
        audience: CredentialAudience,
        scope: BTreeSet<CredentialScope>,
        expires_at: i64,
    ) -> Result<PreparedApiKeyIssue, CredentialFailure> {
        let now = self.clock.now();
        if name.trim().is_empty()
            || name.len() > 160
            || name.bytes().any(|byte| byte.is_ascii_control())
            || expires_at <= now
            || scope.is_empty()
            || scope.iter().any(|value| {
                !matches!(
                    value,
                    CredentialScope::Api | CredentialScope::Administration
                )
            })
        {
            return Err(CredentialFailure::Policy);
        }
        let id = self.random.credential_id();
        let subject = CredentialSubjectId::new();
        let entropy = self.random.secret(48).map_err(Self::map_system_failure)?;
        let secret = CredentialSecret::new(format!("z4g_{}.{}", id.as_str(), entropy.expose()));
        let verifier = PasswordPolicy::default().hash(&secret)?;
        let principal = ServicePrincipal {
            id: subject.clone(),
            name: name.to_string(),
            status: CredentialStatus::Active,
            security_generation: 1,
            created_at: now,
            updated_at: now,
        };
        let record = ApiKeyRecord {
            id: id.clone(),
            subject: subject.clone(),
            verifier: verifier.encoded().to_string(),
            issuer: self.policy.issuer.clone(),
            audience,
            scope: scope.clone(),
            created_at: now,
            not_before: now,
            expires_at,
            version: 1,
            status: CredentialStatus::Active,
            overlap_until: None,
            last_used_bucket: None,
        };
        let event = CredentialEvent {
            kind: CredentialEventKind::ApiKeyIssued,
            subject,
            session_id: None,
            family_id: None,
            credential_id: Some(id),
            key_version: None,
            correlation_id: self.random.correlation_id(),
            node_id: self.node_id.clone(),
            occurred_at: now,
        };
        let issued = IssuedApiKey {
            descriptor: Self::api_key_descriptor(&record, &principal),
            secret,
        };
        Ok(PreparedApiKeyIssue {
            issued,
            principal,
            record,
            event,
        })
    }

    pub fn list_api_keys(&self) -> Result<Vec<ApiKeyDescriptor>, CredentialFailure> {
        let records = self.store.api_keys().map_err(Self::map_system_failure)?;
        let mut descriptors = Vec::with_capacity(records.len());
        for record in records {
            let principal = self
                .store
                .service_principal(&record.subject)
                .map_err(Self::map_system_failure)?
                .ok_or(CredentialFailure::Internal)?;
            descriptors.push(Self::api_key_descriptor(&record, &principal));
        }
        descriptors.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(descriptors)
    }

    pub fn rotate_api_key(
        &self,
        predecessor_id: &CredentialId,
        overlap_until: i64,
        expires_at: i64,
    ) -> Result<IssuedApiKey, CredentialFailure> {
        let prepared = self.prepare_api_key_rotation(predecessor_id, overlap_until, expires_at)?;
        let (issued, predecessor_id, successor, overlap_until, occurred_at, event) =
            prepared.into_parts();
        self.store
            .rotate_api_key(
                &predecessor_id,
                &successor,
                overlap_until,
                occurred_at,
                &event,
            )
            .map_err(Self::map_system_failure)?;
        Ok(issued)
    }

    pub fn prepare_api_key_rotation(
        &self,
        predecessor_id: &CredentialId,
        overlap_until: i64,
        expires_at: i64,
    ) -> Result<PreparedApiKeyRotation, CredentialFailure> {
        let now = self.clock.now();
        let predecessor = self
            .store
            .api_key(predecessor_id)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InvalidCredential)?;
        if predecessor.status != CredentialStatus::Active
            || overlap_until <= now
            || overlap_until > predecessor.expires_at
            || expires_at <= overlap_until
        {
            return Err(CredentialFailure::Policy);
        }
        let principal = self
            .store
            .service_principal(&predecessor.subject)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::Internal)?;
        let id = self.random.credential_id();
        let entropy = self.random.secret(48).map_err(Self::map_system_failure)?;
        let secret = CredentialSecret::new(format!("z4g_{}.{}", id.as_str(), entropy.expose()));
        let verifier = PasswordPolicy::default().hash(&secret)?;
        let successor = ApiKeyRecord {
            id: id.clone(),
            subject: predecessor.subject.clone(),
            verifier: verifier.encoded().to_string(),
            issuer: predecessor.issuer.clone(),
            audience: predecessor.audience,
            scope: predecessor.scope.clone(),
            created_at: now,
            not_before: now,
            expires_at,
            version: predecessor.version.saturating_add(1),
            status: CredentialStatus::Active,
            overlap_until: None,
            last_used_bucket: None,
        };
        let event = CredentialEvent {
            kind: CredentialEventKind::ApiKeyRotated,
            subject: predecessor.subject,
            session_id: None,
            family_id: None,
            credential_id: Some(id),
            key_version: None,
            correlation_id: self.random.correlation_id(),
            node_id: self.node_id.clone(),
            occurred_at: now,
        };
        let issued = IssuedApiKey {
            descriptor: Self::api_key_descriptor(&successor, &principal),
            secret,
        };
        Ok(PreparedApiKeyRotation {
            issued,
            predecessor_id: predecessor_id.clone(),
            successor,
            overlap_until,
            occurred_at: now,
            event,
        })
    }

    pub fn revoke_api_key(&self, id: &CredentialId) -> Result<ApiKeyDescriptor, CredentialFailure> {
        let (occurred_at, event) = self.prepare_api_key_revocation(id)?;
        let record = self
            .store
            .api_key(id)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InvalidCredential)?;
        let principal = self
            .store
            .service_principal(&record.subject)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::Internal)?;
        let revoked = self
            .store
            .revoke_api_key(id, occurred_at, &event)
            .map_err(Self::map_system_failure)?;
        Ok(Self::api_key_descriptor(&revoked, &principal))
    }

    pub fn prepare_api_key_revocation(
        &self,
        id: &CredentialId,
    ) -> Result<(i64, CredentialEvent), CredentialFailure> {
        let now = self.clock.now();
        let record = self
            .store
            .api_key(id)
            .map_err(Self::map_system_failure)?
            .ok_or(CredentialFailure::InvalidCredential)?;
        let event = CredentialEvent {
            kind: CredentialEventKind::ApiKeyRevoked,
            subject: record.subject,
            session_id: None,
            family_id: None,
            credential_id: Some(id.clone()),
            key_version: None,
            correlation_id: self.random.correlation_id(),
            node_id: self.node_id.clone(),
            occurred_at: now,
        };
        Ok((now, event))
    }

    pub fn verify_api_key(
        &self,
        secret: &CredentialSecret,
        audience: CredentialAudience,
        required_scope: CredentialScope,
    ) -> Result<AuthenticatedApiKey, CredentialFailure> {
        let parsed_id = secret
            .expose()
            .strip_prefix("z4g_")
            .and_then(|value| value.split_once('.'))
            .and_then(|(id, material)| (!material.is_empty()).then_some(id))
            .ok_or(CredentialFailure::Malformed)
            .and_then(CredentialId::parse);
        let admission_key = parsed_id
            .as_ref()
            .map_or("malformed-api-key", CredentialId::as_str);
        let _admission = self.admission.admit_secret_verification(
            b"api-key-id",
            admission_key,
            self.clock.now(),
        )?;
        let record = match &parsed_id {
            Ok(id) => self.store.api_key(id).map_err(Self::map_system_failure)?,
            Err(_) => None,
        };
        let verifier = record
            .as_ref()
            .and_then(|record| PasswordHash::from_encoded(&record.verifier, 1).ok())
            .unwrap_or_else(|| self.api_key_dummy_hash.clone());
        let verification = PasswordPolicy::default().verify(secret, &verifier);
        parsed_id?;
        let Some(record) = record else {
            return Err(CredentialFailure::InvalidCredential);
        };
        if !matches!(
            verification,
            PasswordVerification::Valid | PasswordVerification::ValidNeedsUpgrade
        ) {
            return Err(CredentialFailure::InvalidCredential);
        }
        let now = self.clock.now();
        let active = match record.status {
            CredentialStatus::Active => true,
            CredentialStatus::Rotating => {
                record.overlap_until.is_some_and(|deadline| now < deadline)
            }
            CredentialStatus::Suspended | CredentialStatus::Revoked | CredentialStatus::Expired => {
                false
            }
        };
        if !active
            || record.issuer != self.policy.issuer
            || record.audience != audience
            || !record.scope.contains(&required_scope)
            || now < record.not_before
            || now >= record.expires_at
        {
            return Err(CredentialFailure::InvalidCredential);
        }
        let principal = self
            .store
            .service_principal(&record.subject)
            .map_err(Self::map_system_failure)?
            .filter(|principal| principal.status == CredentialStatus::Active)
            .ok_or(CredentialFailure::InvalidCredential)?;
        Ok(AuthenticatedApiKey {
            descriptor: Self::api_key_descriptor(&record, &principal),
            principal,
        })
    }

    fn api_key_descriptor(record: &ApiKeyRecord, principal: &ServicePrincipal) -> ApiKeyDescriptor {
        ApiKeyDescriptor {
            id: record.id.clone(),
            subject: record.subject.clone(),
            name: principal.name.clone(),
            issuer: record.issuer.clone(),
            audience: record.audience,
            scope: record.scope.clone(),
            created_at: record.created_at,
            not_before: record.not_before,
            expires_at: record.expires_at,
            version: record.version,
            status: record.status,
            overlap_until: record.overlap_until,
        }
    }

    /// The per-subject cap never refuses: storing the session evicts the
    /// subject's oldest one. Only the server-wide cap refuses, unless that
    /// eviction frees a slot.
    fn ensure_session_admission(&self, user: &User) -> Result<(), CredentialFailure> {
        let now = self.clock.now();
        let active = self
            .store
            .sessions_for_subject(&user.principal_id)
            .map_err(Self::map_system_failure)?
            .into_iter()
            .filter(|session| {
                session.status == SessionStatus::Active
                    && session.absolute_expires_at > now
                    && session.idle_expires_at > now
            })
            .count();
        let evicts = active >= self.policy.maximum_active_sessions_per_subject;
        let global_active = self
            .store
            .sessions()
            .map_err(Self::map_system_failure)?
            .into_iter()
            .filter(|session| {
                session.status == SessionStatus::Active
                    && session.absolute_expires_at > now
                    && session.idle_expires_at > now
            })
            .count();
        if global_active >= self.policy.maximum_active_sessions_global && !evicts {
            return Err(CredentialFailure::Throttled);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn access_claims(
        &self,
        user: &User,
        audience: CredentialAudience,
        session_id: &SessionId,
        family_id: &TokenFamilyId,
        token_id: TokenId,
        issued_at: i64,
        expires_at: i64,
    ) -> AccessClaims {
        AccessClaims {
            issuer: self.policy.issuer.clone(),
            audience,
            subject: user.principal_id.clone(),
            scope: BTreeSet::from([CredentialScope::Access]),
            issued_at,
            not_before: issued_at,
            expires_at,
            token_id,
            session_id: session_id.clone(),
            family_id: family_id.clone(),
            key_version: self.signer.key_version(),
            security_generation: user.security_generation,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn refresh_claims(
        &self,
        user: &User,
        audience: CredentialAudience,
        session_id: &SessionId,
        family_id: &TokenFamilyId,
        token_id: TokenId,
        issued_at: i64,
        expires_at: i64,
    ) -> RefreshClaims {
        RefreshClaims {
            issuer: self.policy.issuer.clone(),
            audience,
            subject: user.principal_id.clone(),
            scope: BTreeSet::from([CredentialScope::Refresh]),
            issued_at,
            not_before: issued_at,
            expires_at,
            token_id,
            session_id: session_id.clone(),
            family_id: family_id.clone(),
            key_version: self.signer.key_version(),
            security_generation: user.security_generation,
        }
    }

    fn map_system_failure(error: SystemError) -> CredentialFailure {
        match error {
            SystemError::Credential(failure) => failure,
            SystemError::AuthenticationFailed(_) => CredentialFailure::InvalidCredential,
            SystemError::InvalidConfiguration(_) => CredentialFailure::Policy,
            _ => CredentialFailure::Internal,
        }
    }

    fn map_password_auth_failure(error: SystemError) -> CredentialFailure {
        match error {
            SystemError::PasswordChangeRequired | SystemError::BootstrapExpired => {
                CredentialFailure::InactiveSession
            }
            other => Self::map_system_failure(other),
        }
    }
}

pub async fn maintain_local_signing_keys(
    security: Arc<LocalSecurityService>,
    token_service: Arc<crate::server::auth::CredentialTokenService>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    loop {
        interval.tick().await;
        if security
            .retire_expired_signing_keys(&token_service)
            .is_err()
        {
            continue;
        }
        let _ = security.synchronize_token_verifier(&token_service);
    }
}

pub fn initialize_local_credential_runtime(
    security: Arc<SecurityManager>,
    signer_reference: &crate::server::auth::SignerReference,
    policy: CredentialRuntimePolicy,
    node_id: impl Into<String>,
) -> Result<
    (
        Arc<LocalSecurityService>,
        Arc<crate::server::auth::CredentialTokenService>,
    ),
    CredentialFailure,
> {
    let store = Arc::new(LocalCredentialStore::new(security.storage()));
    let signer = Arc::new(crate::server::auth::Ed25519Signer::load(signer_reference)?);
    let now = Utc::now().timestamp();
    let mut keys = store
        .retire_expired_signing_keys(now)
        .map_err(|_| CredentialFailure::Internal)?;
    let local_public_key = signer
        .verification_key(SigningKeyStatus::Prepared, 0)
        .public_key;
    if keys.is_empty() {
        let prepared = signer.verification_key(SigningKeyStatus::Prepared, now);
        store
            .prepare_signing_key(&prepared)
            .map_err(|_| CredentialFailure::Internal)?;
        keys = store
            .activate_signing_key(signer_reference.version(), now, None)
            .map_err(|_| CredentialFailure::Internal)?;
    } else if let Some(local_key) = keys.iter().find(|key| {
        key.version == signer_reference.version()
            && key.key_id == signer_reference.key_id()
            && key.public_key == local_public_key
    }) {
        if local_key.status == SigningKeyStatus::Prepared {
            keys = store
                .activate_signing_key(signer_reference.version(), now, None)
                .map_err(|_| CredentialFailure::Internal)?;
        } else if matches!(
            local_key.status,
            SigningKeyStatus::Retired | SigningKeyStatus::Compromised
        ) {
            return Err(CredentialFailure::KeyVersion);
        }
    } else {
        let maximum_version = keys
            .iter()
            .map(|key| key.version)
            .max()
            .ok_or(CredentialFailure::Internal)?;
        let active_count = keys
            .iter()
            .filter(|key| key.status == SigningKeyStatus::Active)
            .count();
        let incident_recovery = active_count == 0
            && keys.iter().all(|key| {
                matches!(
                    key.status,
                    SigningKeyStatus::Compromised | SigningKeyStatus::Retired
                )
            });
        if signer_reference.version() <= maximum_version
            || (active_count != 1 && !incident_recovery)
        {
            return Err(CredentialFailure::KeyVersion);
        }
        let prepared = signer.verification_key(SigningKeyStatus::Prepared, now);
        store
            .prepare_signing_key(&prepared)
            .map_err(|_| CredentialFailure::Internal)?;
        let overlap_deadline = if active_count == 1 {
            Some(
                now.checked_add(policy.refresh_ttl_seconds)
                    .and_then(|deadline| deadline.checked_add(policy.clock_skew_seconds))
                    .ok_or(CredentialFailure::Policy)?,
            )
        } else {
            None
        };
        keys = store
            .activate_signing_key(signer_reference.version(), now, overlap_deadline)
            .map_err(|_| CredentialFailure::Internal)?;
    }
    let token_service = Arc::new(crate::server::auth::CredentialTokenService::new(
        signer.clone(),
        keys,
    )?);
    let lifecycle = Arc::new(LocalSecurityService::new(
        security,
        store,
        Arc::new(SystemCredentialClock),
        Arc::new(SystemCredentialRandom),
        signer,
        policy,
        node_id,
    )?);
    Ok((lifecycle, token_service))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::atomic::{AtomicI64, Ordering};

    use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
    use zeta4j_credential::{
        CredentialAudience, CredentialScope, KeyVersion, SecretDigest, SigningKeyStatus,
        digest_secret,
    };

    use super::*;
    use crate::server::auth::{CredentialTokenService, Ed25519Signer, SignerReference};

    fn fixture(
        now: i64,
    ) -> SystemResult<(AuthSession, RefreshFamily, CredentialEvent, SecretDigest)> {
        let session_id = SessionId::new();
        let family_id = TokenFamilyId::new();
        let subject = CredentialSubjectId::new();
        let refresh_digest = digest_secret(b"refresh", &CredentialSecret::new("refresh-canary"));
        let session = AuthSession {
            id: session_id.clone(),
            subject: subject.clone(),
            credential_kind: CredentialKind::Password,
            created_at: now,
            last_refreshed_at: now,
            absolute_expires_at: now + 1_000,
            idle_expires_at: now + 100,
            security_generation: 1,
            key_version: KeyVersion::new(1).map_err(LocalCredentialStore::map_failure)?,
            family_id: family_id.clone(),
            status: SessionStatus::Active,
            closed_at: None,
        };
        let family = RefreshFamily {
            id: family_id.clone(),
            session_id: session_id.clone(),
            current_token_id: TokenId::new(),
            current_digest: refresh_digest.clone(),
            consumed_digests: Vec::new(),
            status: CredentialStatus::Active,
            expires_at: now + 1_000,
            version: 1,
        };
        let event = CredentialEvent {
            kind: CredentialEventKind::SessionOpened,
            subject,
            session_id: Some(session_id),
            family_id: Some(family_id),
            credential_id: None,
            key_version: None,
            correlation_id: CorrelationId::new(),
            node_id: "base".to_string(),
            occurred_at: now,
        };
        Ok((session, family, event, refresh_digest))
    }

    #[test]
    fn session_family_and_event_survive_reopen_atomically() -> SystemResult<()> {
        let temp = crate::server::system::owner_only_tempdir().map_err(SystemError::Io)?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let store = LocalCredentialStore::new(storage.clone());
        let (session, family, event, _) = fixture(1_000)?;
        store.open(&session, &family, &event, 16, 10_000)?;
        drop(store);
        drop(storage);

        let reopened_storage = Arc::new(SystemStorage::open(temp.path())?);
        let reopened = LocalCredentialStore::new(reopened_storage.clone());
        assert_eq!(reopened.session(&session.id)?, Some(session));
        assert_eq!(reopened.family(&family.id)?, Some(family));
        Ok(())
    }

    #[test]
    fn replay_revocation_and_event_commit_together() -> SystemResult<()> {
        let temp = crate::server::system::owner_only_tempdir().map_err(SystemError::Io)?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let store = LocalCredentialStore::new(storage.clone());
        let (session, family, event, digest) = fixture(1_000)?;
        store.open(&session, &family, &event, 16, 10_000)?;
        let successor = digest_secret(b"refresh", &CredentialSecret::new("successor"));
        let first = store.consume(
            &family.id,
            &digest,
            TokenId::new(),
            successor,
            1_010,
            1_200,
            CorrelationId::new(),
            "base",
        )?;
        assert!(matches!(first, RefreshTransition::Rotated { .. }));
        let replay = store.consume(
            &family.id,
            &digest,
            TokenId::new(),
            digest_secret(b"refresh", &CredentialSecret::new("unused")),
            1_020,
            1_300,
            CorrelationId::new(),
            "base",
        )?;
        assert!(matches!(replay, RefreshTransition::ReplayRevoked { .. }));
        let persisted_session = store
            .session(&session.id)?
            .ok_or_else(|| SystemError::Internal("session missing after replay".to_string()))?;
        assert_eq!(persisted_session.status, SessionStatus::FamilyReplay);
        Ok(())
    }

    #[test]
    fn durable_admission_is_bounded_and_survives_reopen() -> SystemResult<()> {
        let temp = crate::server::system::owner_only_tempdir().map_err(SystemError::Io)?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let store = LocalCredentialStore::new(storage.clone());
        let bucket = digest_secret(
            b"password-account",
            &CredentialSecret::new("opaque-account"),
        );
        for (offset, expected) in [
            (0_i64, SecurityMutationOutcome::AdmissionGranted),
            (1_i64, SecurityMutationOutcome::AdmissionGranted),
            (2_i64, SecurityMutationOutcome::AdmissionThrottled),
        ] {
            let outcome = store.apply_security_mutation(SecurityMutation::ConsumeAdmission {
                bucket_key: bucket.clone(),
                occurred_at: 1_000 + offset,
                correlation_id: CorrelationId::new(),
                maximum_buckets: 1,
                attempts_per_minute: 2,
            })?;
            assert_eq!(outcome, expected);
        }
        drop(store);
        drop(storage);
        let reopened = LocalCredentialStore::new(Arc::new(SystemStorage::open(temp.path())?));
        assert_eq!(
            reopened
                .storage
                .scan_prefix::<CredentialThrottleState>(cf::CREDENTIAL_THROTTLES, b"throttle:")?
                .len(),
            1
        );
        let outcome = reopened.apply_security_mutation(SecurityMutation::ConsumeAdmission {
            bucket_key: bucket,
            occurred_at: 1_003,
            correlation_id: CorrelationId::new(),
            maximum_buckets: 1,
            attempts_per_minute: 2,
        })?;
        assert_eq!(outcome, SecurityMutationOutcome::AdmissionThrottled);
        Ok(())
    }

    #[test]
    fn signing_key_activation_is_monotonic_and_compromise_closes_sessions() -> SystemResult<()> {
        let temp = crate::server::system::owner_only_tempdir().map_err(SystemError::Io)?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let store = LocalCredentialStore::new(storage);
        let (session, family, event, _) = fixture(1_000)?;
        store.open(&session, &family, &event, 16, 10_000)?;
        let version = KeyVersion::new(1).map_err(LocalCredentialStore::map_failure)?;
        let prepared = SigningVerificationKey {
            version,
            key_id: "key-1".to_string(),
            public_key: vec![7_u8; 32],
            status: SigningKeyStatus::Prepared,
            prepared_at: 1_000,
            activated_at: None,
            verify_until: None,
        };
        store.prepare_signing_key(&prepared)?;
        store.prepare_signing_key(&SigningVerificationKey {
            prepared_at: 1_001,
            ..prepared.clone()
        })?;
        assert_eq!(
            store
                .signing_keys()?
                .into_iter()
                .find(|key| key.version == version)
                .map(|key| key.prepared_at),
            Some(1_000)
        );
        let keys = store.activate_signing_key(version, 1_001, None)?;
        assert_eq!(keys[0].status, SigningKeyStatus::Active);
        assert!(store.prepare_signing_key(&prepared).is_err());
        assert_eq!(
            store.compromise_signing_key(version, 1_002, CorrelationId::new(), "base",)?,
            1
        );
        assert_eq!(
            store.session(&session.id)?.map(|value| value.status),
            Some(SessionStatus::Aborted)
        );
        assert_eq!(
            store.family(&family.id)?.map(|value| value.status),
            Some(CredentialStatus::Revoked)
        );
        Ok(())
    }

    #[test]
    fn signer_seam_types_do_not_serialize_raw_tokens() -> SystemResult<()> {
        struct UnusedSigner {
            version: KeyVersion,
        }
        impl CredentialSigner for UnusedSigner {
            fn key_version(&self) -> KeyVersion {
                self.version
            }

            fn verification_key(
                &self,
                status: SigningKeyStatus,
                prepared_at: i64,
            ) -> SigningVerificationKey {
                SigningVerificationKey {
                    version: self.version,
                    key_id: "unused-test-signer".to_string(),
                    public_key: vec![0_u8; 32],
                    status,
                    prepared_at,
                    activated_at: None,
                    verify_until: None,
                }
            }

            fn sign_access(
                &self,
                _claims: &AccessClaims,
            ) -> Result<CredentialSecret, CredentialFailure> {
                Err(CredentialFailure::Unavailable)
            }

            fn sign_refresh(
                &self,
                _claims: &RefreshClaims,
            ) -> Result<CredentialSecret, CredentialFailure> {
                Err(CredentialFailure::Unavailable)
            }
        }

        let temp = crate::server::system::owner_only_tempdir().map_err(SystemError::Io)?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = Arc::new(SecurityManager::new(storage.clone()));
        let service = LocalSecurityService::new(
            security,
            Arc::new(LocalCredentialStore::new(storage)),
            Arc::new(SystemCredentialClock),
            Arc::new(SystemCredentialRandom),
            Arc::new(UnusedSigner {
                version: KeyVersion::new(1).map_err(LocalCredentialStore::map_failure)?,
            }),
            CredentialRuntimePolicy::default(),
            "base",
        )
        .map_err(LocalCredentialStore::map_failure)?;
        assert_eq!(service.node_id(), "base");
        let _closed_types = (
            CredentialAudience::Http,
            BTreeSet::from([CredentialScope::Access]),
            KeyVersion::new(1).map_err(LocalCredentialStore::map_failure)?,
        );
        Ok(())
    }

    struct TestClock(AtomicI64);

    impl TestClock {
        fn new(now: i64) -> Self {
            Self(AtomicI64::new(now))
        }

        fn set(&self, now: i64) {
            self.0.store(now, Ordering::SeqCst);
        }
    }

    impl CredentialClock for TestClock {
        fn now(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn token_service(
        temp: &tempfile::TempDir,
    ) -> Result<(Arc<Ed25519Signer>, CredentialTokenService), CredentialFailure> {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| CredentialFailure::Internal)?;
        let path = temp.path().join("credential-signer.pk8");
        std::fs::write(&path, document.as_ref()).map_err(|_| CredentialFailure::Internal)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| CredentialFailure::Internal)?;
        }
        let reference = SignerReference::new(path, "credential-key-1", KeyVersion::new(1)?)?;
        let signer = Arc::new(Ed25519Signer::load(&reference)?);
        let verification = signer.verification_key(SigningKeyStatus::Active, 1_000);
        let service = CredentialTokenService::new(signer.clone(), vec![verification])?;
        Ok((signer, service))
    }

    fn signer_reference(
        temp: &tempfile::TempDir,
        filename: &str,
        key_id: &str,
        version: u64,
    ) -> Result<SignerReference, CredentialFailure> {
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| CredentialFailure::Internal)?;
        let path = temp.path().join(filename);
        std::fs::write(&path, document.as_ref()).map_err(|_| CredentialFailure::Internal)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| CredentialFailure::Internal)?;
        }
        SignerReference::new(path, key_id, KeyVersion::new(version)?)
    }

    #[test]
    fn higher_signer_recovers_only_after_all_prior_keys_are_terminal()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path().join("system"))?);
        let security = Arc::new(SecurityManager::new(storage.clone()));
        let first_version = KeyVersion::new(1)?;
        let second_version = KeyVersion::new(2)?;
        let first = signer_reference(&temp, "signer-v1.pk8", "credential-key-v1", 1)?;
        let (first_runtime, first_tokens) = initialize_local_credential_runtime(
            security.clone(),
            &first,
            CredentialRuntimePolicy::default(),
            "base",
        )?;
        drop(first_runtime);
        drop(first_tokens);

        let store = LocalCredentialStore::new(storage.clone());
        store.compromise_signing_key(
            first_version,
            chrono::Utc::now().timestamp(),
            CorrelationId::new(),
            "base",
        )?;
        let second = signer_reference(&temp, "signer-v2.pk8", "credential-key-v2", 2)?;
        let (second_runtime, _) = initialize_local_credential_runtime(
            security,
            &second,
            CredentialRuntimePolicy::default(),
            "base",
        )?;
        let keys = second_runtime.signing_keys()?;
        assert!(keys.iter().any(|key| {
            key.version == first_version && key.status == SigningKeyStatus::Compromised
        }));
        assert!(keys.iter().any(|key| {
            key.version == second_version && key.status == SigningKeyStatus::Active
        }));
        Ok(())
    }

    fn activate_test_signer(
        store: &LocalCredentialStore,
        signer: &Ed25519Signer,
        now: i64,
    ) -> SystemResult<()> {
        let prepared = signer.verification_key(SigningKeyStatus::Prepared, now);
        store.prepare_signing_key(&prepared)?;
        store.activate_signing_key(signer.key_version(), now, None)?;
        Ok(())
    }

    #[test]
    fn admission_is_bounded_and_preserves_unrelated_account_progress()
    -> Result<(), CredentialFailure> {
        let policy = CredentialRuntimePolicy {
            maximum_admission_buckets: 2,
            authentication_attempts_per_minute: 2,
            maximum_concurrent_secret_verifications: 1,
            ..CredentialRuntimePolicy::default()
        };
        let admission = CredentialAdmissionController::new(&policy);
        let first = admission.admit_secret_verification(b"password-account", "attacked", 1_000)?;
        assert!(matches!(
            admission.admit_secret_verification(b"password-account", "other", 1_000),
            Err(CredentialFailure::Throttled)
        ));
        drop(first);
        let attacked_second =
            admission.admit_secret_verification(b"password-account", "attacked", 1_001)?;
        drop(attacked_second);
        assert!(matches!(
            admission.admit_secret_verification(b"password-account", "attacked", 1_002),
            Err(CredentialFailure::Throttled)
        ));
        let other = admission.admit_secret_verification(b"password-account", "other", 1_002)?;
        drop(other);
        for account in ["spray-a", "spray-b", "spray-c"] {
            let result = admission.admit_secret_verification(b"password-account", account, 1_003);
            if let Ok(permit) = result {
                drop(permit);
            }
        }
        assert_eq!(admission.retained_bucket_count()?, 2);
        Ok(())
    }

    #[test]
    fn login_refresh_replay_and_logout_use_durable_session_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = Arc::new(SecurityManager::new(storage.clone()));
        security.create_user("lifecycle-user", "Correct!Password234")?;
        let (signer, token_service) =
            token_service(&temp).map_err(LocalCredentialStore::map_failure)?;
        let credential_store = LocalCredentialStore::new(storage.clone());
        activate_test_signer(&credential_store, &signer, 1_000)?;
        let clock = Arc::new(TestClock::new(1_000));
        let lifecycle = LocalSecurityService::new(
            security,
            Arc::new(credential_store),
            clock.clone(),
            Arc::new(SystemCredentialRandom),
            signer,
            CredentialRuntimePolicy {
                access_ttl_seconds: 300,
                refresh_ttl_seconds: 1_000,
                session_absolute_ttl_seconds: 1_000,
                session_idle_ttl_seconds: 500,
                clock_skew_seconds: 5,
                ..CredentialRuntimePolicy::default()
            },
            "base",
        )
        .map_err(LocalCredentialStore::map_failure)?;

        let first = lifecycle
            .login(
                &token_service,
                "lifecycle-user",
                "Correct!Password234",
                CredentialAudience::Http,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        lifecycle
            .validate_access(
                &token_service,
                first.access_token(),
                CredentialAudience::Http,
            )
            .map_err(LocalCredentialStore::map_failure)?;

        clock.set(1_010);
        let successor = lifecycle
            .refresh(
                &token_service,
                first.refresh_token(),
                CredentialAudience::Http,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        assert!(matches!(
            lifecycle.refresh(
                &token_service,
                first.refresh_token(),
                CredentialAudience::Http,
            ),
            Err(CredentialFailure::InvalidCredential)
        ));
        assert!(matches!(
            lifecycle.validate_access(
                &token_service,
                successor.access_token(),
                CredentialAudience::Http,
            ),
            Err(CredentialFailure::InactiveSession)
        ));

        let second = lifecycle
            .login(
                &token_service,
                "lifecycle-user",
                "Correct!Password234",
                CredentialAudience::Http,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        lifecycle
            .logout(
                &token_service,
                second.access_token(),
                CredentialAudience::Http,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        let closed_session = LocalCredentialStore::new(storage.clone())
            .session(&second.session_id)?
            .ok_or_else(|| SystemError::Internal("logged-out session missing".to_string()))?;
        assert_eq!(
            LocalCredentialStore::new(storage.clone())
                .family(&closed_session.family_id)?
                .map(|family| family.status),
            Some(CredentialStatus::Revoked)
        );
        assert!(matches!(
            lifecycle.validate_access(
                &token_service,
                second.access_token(),
                CredentialAudience::Http,
            ),
            Err(CredentialFailure::InactiveSession)
        ));

        let bolt_session = lifecycle
            .open_password_session("lifecycle-user", "Correct!Password234")
            .map_err(LocalCredentialStore::map_failure)?;
        lifecycle
            .validate_password_session(
                &bolt_session.session_id,
                &bolt_session.user.principal_id,
                bolt_session.user.security_generation,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        lifecycle
            .close_password_session(&bolt_session.session_id)
            .map_err(LocalCredentialStore::map_failure)?;
        assert!(matches!(
            lifecycle.validate_password_session(
                &bolt_session.session_id,
                &bolt_session.user.principal_id,
                bolt_session.user.security_generation,
            ),
            Err(CredentialFailure::InactiveSession)
        ));

        let issued_api_key = lifecycle
            .create_api_key(
                "backup-service",
                CredentialAudience::Http,
                BTreeSet::from([CredentialScope::Api]),
                4_000,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        lifecycle
            .verify_api_key(
                issued_api_key.secret(),
                CredentialAudience::Http,
                CredentialScope::Api,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        let rotated_api_key = lifecycle
            .rotate_api_key(&issued_api_key.descriptor.id, 1_500, 5_000)
            .map_err(LocalCredentialStore::map_failure)?;
        lifecycle
            .verify_api_key(
                issued_api_key.secret(),
                CredentialAudience::Http,
                CredentialScope::Api,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        clock.set(1_500);
        assert!(matches!(
            lifecycle.verify_api_key(
                issued_api_key.secret(),
                CredentialAudience::Http,
                CredentialScope::Api,
            ),
            Err(CredentialFailure::InvalidCredential)
        ));
        lifecycle
            .verify_api_key(
                rotated_api_key.secret(),
                CredentialAudience::Http,
                CredentialScope::Api,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        lifecycle
            .revoke_api_key(&rotated_api_key.descriptor.id)
            .map_err(LocalCredentialStore::map_failure)?;
        assert!(matches!(
            lifecycle.verify_api_key(
                rotated_api_key.secret(),
                CredentialAudience::Http,
                CredentialScope::Api,
            ),
            Err(CredentialFailure::InvalidCredential)
        ));
        let persisted_api_keys = LocalCredentialStore::new(storage.clone()).api_keys()?;
        let persisted_debug = format!("{persisted_api_keys:?}");
        assert!(!persisted_debug.contains(issued_api_key.secret().expose()));
        assert!(!persisted_debug.contains(rotated_api_key.secret().expose()));
        assert_eq!(
            lifecycle
                .list_api_keys()
                .map_err(LocalCredentialStore::map_failure)?
                .len(),
            2
        );

        assert!(format!("{first:?}").contains("[REDACTED]"));
        assert!(!format!("{first:?}").contains(first.access_token().expose()));
        Ok(())
    }

    #[test]
    fn concurrent_refresh_has_exactly_one_winner_and_revokes_replayed_family()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = Arc::new(SecurityManager::new(storage.clone()));
        security.create_user("refresh-race-user", "Correct!Password234")?;
        let (signer, token_service) =
            token_service(&temp).map_err(LocalCredentialStore::map_failure)?;
        let credential_store = LocalCredentialStore::new(storage.clone());
        activate_test_signer(&credential_store, &signer, 1_000)?;
        let lifecycle = Arc::new(
            LocalSecurityService::new(
                security,
                Arc::new(credential_store),
                Arc::new(TestClock::new(1_000)),
                Arc::new(SystemCredentialRandom),
                signer,
                CredentialRuntimePolicy::default(),
                "base",
            )
            .map_err(LocalCredentialStore::map_failure)?,
        );
        let issued = lifecycle
            .login(
                &token_service,
                "refresh-race-user",
                "Correct!Password234",
                CredentialAudience::Http,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let token_service = &token_service;
            let refresh_token = issued.refresh_token();
            let first_lifecycle = lifecycle.clone();
            let first_barrier = barrier.clone();
            let first = scope.spawn(move || {
                first_barrier.wait();
                first_lifecycle.refresh(token_service, refresh_token, CredentialAudience::Http)
            });
            let second_lifecycle = lifecycle.clone();
            let second_barrier = barrier;
            let second = scope.spawn(move || {
                second_barrier.wait();
                second_lifecycle.refresh(token_service, refresh_token, CredentialAudience::Http)
            });
            [first.join(), second.join()]
        });
        let mut successes = 0_usize;
        for result in results {
            let refresh = result.map_err(|_| std::io::Error::other("refresh worker panicked"))?;
            match refresh {
                Ok(_) => successes = successes.saturating_add(1),
                Err(CredentialFailure::InvalidCredential) => {}
                Err(other) => {
                    return Err(std::io::Error::other(format!(
                        "unexpected refresh failure: {}",
                        other.code()
                    ))
                    .into());
                }
            }
        }
        assert_eq!(successes, 1);
        let session = LocalCredentialStore::new(storage)
            .session(&issued.session_id)?
            .ok_or_else(|| SystemError::Internal("refresh session missing".to_string()))?;
        assert_eq!(session.status, SessionStatus::FamilyReplay);
        Ok(())
    }

    #[test]
    fn login_beyond_subject_session_cap_evicts_oldest_session()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = crate::server::system::owner_only_tempdir()?;
        let storage = Arc::new(SystemStorage::open(temp.path())?);
        let security = Arc::new(SecurityManager::new(storage.clone()));
        security.create_user("busy-user", "Correct!Password234")?;
        let (signer, token_service) =
            token_service(&temp).map_err(LocalCredentialStore::map_failure)?;
        let credential_store = LocalCredentialStore::new(storage.clone());
        activate_test_signer(&credential_store, &signer, 1_000)?;
        let clock = Arc::new(TestClock::new(1_000));
        let policy = CredentialRuntimePolicy::default();
        let cap = policy.maximum_active_sessions_per_subject;
        let lifecycle = LocalSecurityService::new(
            security,
            Arc::new(credential_store),
            clock.clone(),
            Arc::new(SystemCredentialRandom),
            signer,
            policy,
            "base",
        )
        .map_err(LocalCredentialStore::map_failure)?;

        let mut issued = Vec::new();
        for offset in 0..=cap {
            clock.set(1_000 + i64::try_from(offset)?);
            issued.push(
                lifecycle
                    .login(
                        &token_service,
                        "busy-user",
                        "Correct!Password234",
                        CredentialAudience::Http,
                    )
                    .map_err(LocalCredentialStore::map_failure)?,
            );
        }
        let bolt = lifecycle
            .open_password_session("busy-user", "Correct!Password234")
            .map_err(LocalCredentialStore::map_failure)?;

        for evicted in &issued[..2] {
            assert!(matches!(
                lifecycle.validate_access(
                    &token_service,
                    evicted.access_token(),
                    CredentialAudience::Http,
                ),
                Err(CredentialFailure::InactiveSession)
            ));
        }
        for retained in &issued[2..] {
            lifecycle
                .validate_access(
                    &token_service,
                    retained.access_token(),
                    CredentialAudience::Http,
                )
                .map_err(LocalCredentialStore::map_failure)?;
        }
        lifecycle
            .validate_password_session(
                &bolt.session_id,
                &bolt.user.principal_id,
                bolt.user.security_generation,
            )
            .map_err(LocalCredentialStore::map_failure)?;
        let active = LocalCredentialStore::new(storage)
            .sessions_for_subject(&bolt.user.principal_id)?
            .into_iter()
            .filter(|session| session.status == SessionStatus::Active)
            .count();
        assert_eq!(active, cap);
        Ok(())
    }
}
