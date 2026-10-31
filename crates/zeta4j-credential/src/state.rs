// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CorrelationId, CredentialAudience, CredentialId, CredentialScope, CredentialSubjectId,
    KeyVersion, SecretDigest, SessionId, TokenFamilyId, TokenId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Password,
    AccessToken,
    RefreshToken,
    ApiKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    Active,
    Suspended,
    Rotating,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    LoggedOut,
    FamilyReplay,
    PrincipalChanged,
    Expired,
    Aborted,
    /// Closed to admit a newer session once the subject reached its cap.
    Evicted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SigningKeyStatus {
    Prepared,
    Active,
    VerifyOnly,
    Retired,
    Compromised,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
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
    pub closed_at: Option<i64>,
}

impl AuthSession {
    pub fn validate_active(
        &self,
        subject: &CredentialSubjectId,
        security_generation: u64,
        now: i64,
    ) -> Result<(), CredentialFailure> {
        if &self.subject != subject {
            return Err(CredentialFailure::InvalidCredential);
        }
        if self.status != SessionStatus::Active {
            return Err(CredentialFailure::InactiveSession);
        }
        if self.security_generation != security_generation {
            return Err(CredentialFailure::PrincipalChanged);
        }
        if now >= self.absolute_expires_at || now >= self.idle_expires_at {
            return Err(CredentialFailure::Expired);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshFamily {
    pub id: TokenFamilyId,
    pub session_id: SessionId,
    pub current_token_id: TokenId,
    pub current_digest: SecretDigest,
    pub consumed_digests: Vec<SecretDigest>,
    pub status: CredentialStatus,
    pub expires_at: i64,
    pub version: u64,
}

pub fn revoke_refresh_family(family: &RefreshFamily) -> Result<RefreshFamily, CredentialFailure> {
    let mut revoked = family.clone();
    if revoked.status == CredentialStatus::Active {
        revoked.status = CredentialStatus::Revoked;
        revoked.version = revoked
            .version
            .checked_add(1)
            .ok_or(CredentialFailure::Internal)?;
    }
    Ok(revoked)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshTransition {
    Rotated {
        family: RefreshFamily,
        session: AuthSession,
        event: CredentialEvent,
    },
    ReplayRevoked {
        family: RefreshFamily,
        session: AuthSession,
        event: CredentialEvent,
    },
}

#[allow(clippy::too_many_arguments)]
pub fn consume_refresh(
    family: &RefreshFamily,
    session: &AuthSession,
    supplied_digest: &SecretDigest,
    successor_token_id: TokenId,
    successor_digest: SecretDigest,
    now: i64,
    idle_expires_at: i64,
    correlation_id: CorrelationId,
    node_id: impl Into<String>,
) -> Result<RefreshTransition, CredentialFailure> {
    if family.session_id != session.id || family.id != session.family_id {
        return Err(CredentialFailure::Malformed);
    }
    session.validate_active(&session.subject, session.security_generation, now)?;
    if family.status != CredentialStatus::Active || now >= family.expires_at {
        return Err(CredentialFailure::InactiveSession);
    }
    let replayed = family
        .consumed_digests
        .iter()
        .any(|digest| crate::verify_digest(digest, supplied_digest));
    if replayed {
        let revoked_family = revoke_refresh_family(family)?;
        let revoked_session = close_session(session, SessionStatus::FamilyReplay, now)?;
        let event = CredentialEvent {
            kind: CredentialEventKind::RefreshReplayDetected,
            subject: session.subject.clone(),
            session_id: Some(session.id.clone()),
            family_id: Some(family.id.clone()),
            credential_id: None,
            key_version: None,
            correlation_id,
            node_id: node_id.into(),
            occurred_at: now,
        };
        return Ok(RefreshTransition::ReplayRevoked {
            family: revoked_family,
            session: revoked_session,
            event,
        });
    }
    if !crate::verify_digest(&family.current_digest, supplied_digest) {
        return Err(CredentialFailure::InvalidCredential);
    }
    if idle_expires_at <= now || idle_expires_at > session.absolute_expires_at {
        return Err(CredentialFailure::Policy);
    }

    let mut next_family = family.clone();
    next_family
        .consumed_digests
        .push(next_family.current_digest.clone());
    next_family.current_token_id = successor_token_id;
    next_family.current_digest = successor_digest;
    next_family.version = next_family
        .version
        .checked_add(1)
        .ok_or(CredentialFailure::Internal)?;

    let mut next_session = session.clone();
    next_session.last_refreshed_at = now;
    next_session.idle_expires_at = idle_expires_at;

    let event = CredentialEvent {
        kind: CredentialEventKind::RefreshRotated,
        subject: session.subject.clone(),
        session_id: Some(session.id.clone()),
        family_id: Some(family.id.clone()),
        credential_id: None,
        key_version: None,
        correlation_id,
        node_id: node_id.into(),
        occurred_at: now,
    };
    Ok(RefreshTransition::Rotated {
        family: next_family,
        session: next_session,
        event,
    })
}

pub fn close_session(
    session: &AuthSession,
    reason: SessionStatus,
    now: i64,
) -> Result<AuthSession, CredentialFailure> {
    if reason == SessionStatus::Active {
        return Err(CredentialFailure::Policy);
    }
    let mut next = session.clone();
    if next.status == SessionStatus::Active {
        next.status = reason;
        next.closed_at = Some(now);
    }
    Ok(next)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningVerificationKey {
    pub version: KeyVersion,
    pub key_id: String,
    pub public_key: Vec<u8>,
    pub status: SigningKeyStatus,
    pub prepared_at: i64,
    pub activated_at: Option<i64>,
    pub verify_until: Option<i64>,
}

pub fn transition_signing_key(
    key: &SigningVerificationKey,
    target: SigningKeyStatus,
    now: i64,
    verify_until: Option<i64>,
) -> Result<SigningVerificationKey, CredentialFailure> {
    let allowed = matches!(
        (key.status, target),
        (SigningKeyStatus::Prepared, SigningKeyStatus::Active)
            | (SigningKeyStatus::Prepared, SigningKeyStatus::Compromised)
            | (SigningKeyStatus::Active, SigningKeyStatus::VerifyOnly)
            | (SigningKeyStatus::Active, SigningKeyStatus::Compromised)
            | (SigningKeyStatus::VerifyOnly, SigningKeyStatus::Retired)
            | (SigningKeyStatus::VerifyOnly, SigningKeyStatus::Compromised)
    );
    if !allowed {
        return Err(CredentialFailure::KeyTransition);
    }
    if target == SigningKeyStatus::VerifyOnly && verify_until.is_none_or(|deadline| deadline <= now)
    {
        return Err(CredentialFailure::Policy);
    }
    if target != SigningKeyStatus::VerifyOnly && verify_until.is_some() {
        return Err(CredentialFailure::Policy);
    }
    let mut next = key.clone();
    next.status = target;
    next.verify_until = verify_until;
    if target == SigningKeyStatus::Active {
        next.activated_at = Some(now);
    }
    Ok(next)
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub id: CredentialId,
    pub subject: CredentialSubjectId,
    pub verifier: String,
    pub issuer: String,
    pub audience: CredentialAudience,
    pub scope: BTreeSet<CredentialScope>,
    pub created_at: i64,
    pub not_before: i64,
    pub expires_at: i64,
    pub version: u64,
    pub status: CredentialStatus,
    pub overlap_until: Option<i64>,
    pub last_used_bucket: Option<i64>,
}

impl std::fmt::Debug for ApiKeyRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiKeyRecord")
            .field("id", &self.id)
            .field("subject", &self.subject)
            .field("verifier", &"[REDACTED]")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("scope", &self.scope)
            .field("created_at", &self.created_at)
            .field("not_before", &self.not_before)
            .field("expires_at", &self.expires_at)
            .field("version", &self.version)
            .field("status", &self.status)
            .field("overlap_until", &self.overlap_until)
            .field("last_used_bucket", &self.last_used_bucket)
            .finish()
    }
}

pub fn transition_api_key(
    record: &ApiKeyRecord,
    target: CredentialStatus,
    now: i64,
    overlap_until: Option<i64>,
) -> Result<ApiKeyRecord, CredentialFailure> {
    let allowed = matches!(
        (record.status, target),
        (CredentialStatus::Active, CredentialStatus::Rotating)
            | (CredentialStatus::Active, CredentialStatus::Revoked)
            | (CredentialStatus::Active, CredentialStatus::Expired)
            | (CredentialStatus::Rotating, CredentialStatus::Revoked)
            | (CredentialStatus::Rotating, CredentialStatus::Expired)
    );
    if !allowed {
        return Err(CredentialFailure::KeyTransition);
    }
    if target == CredentialStatus::Rotating
        && overlap_until.is_none_or(|deadline| deadline <= now || deadline > record.expires_at)
    {
        return Err(CredentialFailure::Policy);
    }
    if target != CredentialStatus::Rotating && overlap_until.is_some() {
        return Err(CredentialFailure::Policy);
    }
    let mut next = record.clone();
    next.status = target;
    next.overlap_until = overlap_until;
    next.version = next
        .version
        .checked_add(1)
        .ok_or(CredentialFailure::Internal)?;
    Ok(next)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialEventKind {
    SessionOpened,
    SessionClosed,
    RefreshRotated,
    RefreshReplayDetected,
    PrincipalChanged,
    ApiKeyIssued,
    ApiKeyRotated,
    ApiKeyRevoked,
    SigningKeyActivated,
    SigningKeyCompromised,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialEvent {
    pub kind: CredentialEventKind,
    pub subject: CredentialSubjectId,
    pub session_id: Option<SessionId>,
    pub family_id: Option<TokenFamilyId>,
    pub credential_id: Option<CredentialId>,
    pub key_version: Option<KeyVersion>,
    pub correlation_id: CorrelationId,
    pub node_id: String,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CredentialFailure {
    #[error("[SV-AUTH-001] credential rejected")]
    InvalidCredential,
    #[error("[SV-AUTH-002] credential malformed")]
    Malformed,
    #[error("[SV-AUTH-003] credential expired")]
    Expired,
    #[error("[SV-AUTH-004] credential not active")]
    InactiveSession,
    #[error("[SV-AUTH-005] credential issuer rejected")]
    Issuer,
    #[error("[SV-AUTH-006] credential audience rejected")]
    Audience,
    #[error("[SV-AUTH-007] credential scope rejected")]
    Scope,
    #[error("[SV-AUTH-008] credential type rejected")]
    TokenType,
    #[error("[SV-AUTH-009] credential TTL rejected")]
    Ttl,
    #[error("[SV-AUTH-010] credential is not yet valid")]
    NotYetValid,
    #[error("[SV-AUTH-011] principal state changed")]
    PrincipalChanged,
    #[error("[SV-AUTH-012] credential key version rejected")]
    KeyVersion,
    #[error("[SV-AUTH-013] credential key transition rejected")]
    KeyTransition,
    #[error("[SV-AUTH-014] password policy rejected")]
    PasswordPolicy,
    #[error("[SV-AUTH-015] credential policy rejected")]
    Policy,
    #[error("[SV-AUTH-016] credential request throttled")]
    Throttled,
    #[error("[SV-AUTH-017] credential service unavailable")]
    Unavailable,
    #[error("[SV-AUTH-018] credential operation failed")]
    Internal,
}

impl CredentialFailure {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidCredential => "SV-AUTH-001",
            Self::Malformed => "SV-AUTH-002",
            Self::Expired => "SV-AUTH-003",
            Self::InactiveSession => "SV-AUTH-004",
            Self::Issuer => "SV-AUTH-005",
            Self::Audience => "SV-AUTH-006",
            Self::Scope => "SV-AUTH-007",
            Self::TokenType => "SV-AUTH-008",
            Self::Ttl => "SV-AUTH-009",
            Self::NotYetValid => "SV-AUTH-010",
            Self::PrincipalChanged => "SV-AUTH-011",
            Self::KeyVersion => "SV-AUTH-012",
            Self::KeyTransition => "SV-AUTH-013",
            Self::PasswordPolicy => "SV-AUTH-014",
            Self::Policy => "SV-AUTH-015",
            Self::Throttled => "SV-AUTH-016",
            Self::Unavailable => "SV-AUTH-017",
            Self::Internal => "SV-AUTH-018",
        }
    }

    pub const fn public_message(self) -> &'static str {
        match self {
            Self::Expired => "Credential expired",
            Self::NotYetValid => "Credential not active",
            Self::Throttled => "Credential request throttled",
            Self::Unavailable => "Credential service unavailable",
            Self::Internal => "Credential operation failed",
            _ => "Credential rejected",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CredentialSecret, digest_secret};

    fn session(now: i64) -> Result<AuthSession, CredentialFailure> {
        Ok(AuthSession {
            id: SessionId::new(),
            subject: CredentialSubjectId::new(),
            credential_kind: CredentialKind::Password,
            created_at: now,
            last_refreshed_at: now,
            absolute_expires_at: now + 1_000,
            idle_expires_at: now + 100,
            security_generation: 1,
            key_version: KeyVersion::new(1)?,
            family_id: TokenFamilyId::new(),
            status: SessionStatus::Active,
            closed_at: None,
        })
    }

    #[test]
    fn refresh_is_one_time_and_replay_revokes_family_and_session() -> Result<(), CredentialFailure>
    {
        let now = 1_000;
        let session = session(now)?;
        let first_secret = CredentialSecret::new("first-refresh");
        let first_digest = digest_secret(b"refresh", &first_secret);
        let family = RefreshFamily {
            id: session.family_id.clone(),
            session_id: session.id.clone(),
            current_token_id: TokenId::new(),
            current_digest: first_digest.clone(),
            consumed_digests: Vec::new(),
            status: CredentialStatus::Active,
            expires_at: now + 1_000,
            version: 1,
        };
        let next_digest = digest_secret(b"refresh", &CredentialSecret::new("second-refresh"));
        let RefreshTransition::Rotated {
            family: rotated,
            session: refreshed,
            event: _,
        } = consume_refresh(
            &family,
            &session,
            &first_digest,
            TokenId::new(),
            next_digest,
            now + 10,
            now + 200,
            CorrelationId::new(),
            "node-1",
        )?
        else {
            return Err(CredentialFailure::Internal);
        };
        let RefreshTransition::ReplayRevoked {
            family: revoked_family,
            session: revoked_session,
            event,
        } = consume_refresh(
            &rotated,
            &refreshed,
            &first_digest,
            TokenId::new(),
            digest_secret(b"refresh", &CredentialSecret::new("third-refresh")),
            now + 20,
            now + 300,
            CorrelationId::new(),
            "node-1",
        )?
        else {
            return Err(CredentialFailure::Internal);
        };
        assert_eq!(revoked_family.status, CredentialStatus::Revoked);
        assert_eq!(revoked_session.status, SessionStatus::FamilyReplay);
        assert_eq!(event.kind, CredentialEventKind::RefreshReplayDetected);
        Ok(())
    }

    #[test]
    fn invalid_signing_key_transitions_are_closed() -> Result<(), CredentialFailure> {
        let key = SigningVerificationKey {
            version: KeyVersion::new(1)?,
            key_id: "kid-1".to_string(),
            public_key: vec![1, 2, 3],
            status: SigningKeyStatus::Prepared,
            prepared_at: 100,
            activated_at: None,
            verify_until: None,
        };
        assert_eq!(
            transition_signing_key(&key, SigningKeyStatus::Retired, 200, None),
            Err(CredentialFailure::KeyTransition)
        );
        let active = transition_signing_key(&key, SigningKeyStatus::Active, 200, None)?;
        let verify_only =
            transition_signing_key(&active, SigningKeyStatus::VerifyOnly, 300, Some(400))?;
        let retired = transition_signing_key(&verify_only, SigningKeyStatus::Retired, 400, None)?;
        assert_eq!(retired.status, SigningKeyStatus::Retired);
        Ok(())
    }
}
