//! Canonical authenticated principal identity.
//!
//! Authentication adapters resolve credentials into a [`Principal`]. Every
//! active authenticated principal may use every protected surface; there is no
//! role or grant model.

use serde::{Deserialize, Deserializer, Serialize};

/// Stable lowercase identifier used for principals and tenants.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct StableId(String);

impl StableId {
    /// Creates a validated stable identifier.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidStableId> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 160
            && value.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
            })
            && value
                .bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_lowercase());
        if !valid {
            return Err(InvalidStableId(value));
        }
        Ok(Self(value))
    }

    /// Returns the serialized identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for StableId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for StableId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Rejected stable identifier.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid stable identifier: {0}")]
pub struct InvalidStableId(pub String);

/// Canonical opaque principal identifier.
pub type PrincipalId = StableId;
/// Canonical tenant identifier.
pub type TenantId = StableId;

/// Closed principal kinds. A credential cannot silently change kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalKind {
    User,
    ApiKeyService,
    LocalTestAnonymous,
}

/// Current canonical principal lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalStatus {
    Active,
    Suspended,
    BootstrapPending,
    Deleted,
}

/// Canonical principal loaded from current persisted security state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub status: PrincipalStatus,
    pub tenant_id: TenantId,
    pub security_generation: u64,
}

impl Principal {
    /// Only active principals may use protected surfaces.
    pub fn is_active(&self) -> bool {
        self.status == PrincipalStatus::Active
    }
}

/// Compares credential bytes without data-dependent early exit.
pub fn constant_time_equal(expected: &[u8], candidate: &[u8]) -> bool {
    let mut difference = expected.len() ^ candidate.len();
    let length = expected.len().max(candidate.len());
    for index in 0..length {
        let left = expected.get(index).copied().unwrap_or(0);
        let right = candidate.get(index).copied().unwrap_or(0);
        difference |= usize::from(left ^ right);
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principal(status: PrincipalStatus) -> Result<Principal, InvalidStableId> {
        Ok(Principal {
            id: StableId::new("user.alice")?,
            kind: PrincipalKind::User,
            status,
            tenant_id: StableId::new("default")?,
            security_generation: 1,
        })
    }

    #[test]
    fn only_active_principals_are_active() -> Result<(), InvalidStableId> {
        assert!(principal(PrincipalStatus::Active)?.is_active());
        assert!(!principal(PrincipalStatus::Suspended)?.is_active());
        assert!(!principal(PrincipalStatus::BootstrapPending)?.is_active());
        assert!(!principal(PrincipalStatus::Deleted)?.is_active());
        Ok(())
    }

    #[test]
    fn stable_ids_reject_uppercase_and_empty_values() {
        assert!(StableId::new("").is_err());
        assert!(StableId::new("User").is_err());
        assert!(StableId::new("user.alice").is_ok());
    }

    #[test]
    fn constant_time_equal_matches_byte_equality() {
        assert!(constant_time_equal(b"secret", b"secret"));
        assert!(!constant_time_equal(b"secret", b"secreT"));
        assert!(!constant_time_equal(b"secret", b"secret-longer"));
    }
}
