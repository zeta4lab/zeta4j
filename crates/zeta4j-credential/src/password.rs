use std::fmt;

use argon2::{
    Algorithm, Argon2, Params, PasswordHash as ParsedPasswordHash, PasswordHasher,
    PasswordVerifier, Version, password_hash::SaltString,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::{CredentialFailure, CredentialSecret};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordPolicyConfig {
    pub minimum_bytes: usize,
    pub maximum_bytes: usize,
    pub memory_cost_kib: u32,
    pub time_cost: u32,
    pub parallelism: u32,
    pub hash_version: u32,
}

impl Default for PasswordPolicyConfig {
    fn default() -> Self {
        Self {
            minimum_bytes: 12,
            maximum_bytes: 1024,
            memory_cost_kib: 19_456,
            time_cost: 2,
            parallelism: 1,
            hash_version: 1,
        }
    }
}

#[derive(Clone)]
pub struct PasswordPolicy {
    config: PasswordPolicyConfig,
    params: Params,
}

impl fmt::Debug for PasswordPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PasswordPolicy")
            .field("config", &self.config)
            .finish()
    }
}

impl PasswordPolicy {
    pub fn new(config: PasswordPolicyConfig) -> Result<Self, CredentialFailure> {
        if config.minimum_bytes == 0
            || config.minimum_bytes > config.maximum_bytes
            || config.maximum_bytes > 1_048_576
            || config.hash_version == 0
        {
            return Err(CredentialFailure::Policy);
        }
        let params = Params::new(
            config.memory_cost_kib,
            config.time_cost,
            config.parallelism,
            None,
        )
        .map_err(|_| CredentialFailure::Policy)?;
        Ok(Self { config, params })
    }

    pub fn config(&self) -> PasswordPolicyConfig {
        self.config
    }

    pub fn validate(&self, password: &CredentialSecret) -> Result<(), CredentialFailure> {
        let bytes = password.expose().as_bytes();
        if bytes.len() < self.config.minimum_bytes || bytes.len() > self.config.maximum_bytes {
            return Err(CredentialFailure::PasswordPolicy);
        }
        if bytes
            .iter()
            .any(|byte| *byte == 0 || byte.is_ascii_control())
        {
            return Err(CredentialFailure::PasswordPolicy);
        }
        Ok(())
    }

    pub fn hash(&self, password: &CredentialSecret) -> Result<PasswordHash, CredentialFailure> {
        self.validate(password)?;
        let mut salt_bytes = [0_u8; 16];
        rand::rng().fill(&mut salt_bytes);
        let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| CredentialFailure::Internal)?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, self.params.clone());
        let encoded = argon2
            .hash_password(password.expose().as_bytes(), &salt)
            .map_err(|_| CredentialFailure::Internal)?
            .to_string();
        Ok(PasswordHash {
            encoded,
            policy_version: self.config.hash_version,
        })
    }

    pub fn verify(
        &self,
        password: &CredentialSecret,
        password_hash: &PasswordHash,
    ) -> PasswordVerification {
        let Ok(parsed) = ParsedPasswordHash::new(&password_hash.encoded) else {
            return PasswordVerification::Invalid;
        };
        if parsed.algorithm.as_str() != "argon2id" || parsed.version != Some(Version::V0x13.into())
        {
            return PasswordVerification::Invalid;
        }
        let params = match Params::try_from(&parsed) {
            Ok(params) => params,
            Err(_) => return PasswordVerification::Invalid,
        };
        if params.m_cost() > self.config.memory_cost_kib.saturating_mul(4)
            || params.t_cost() > self.config.time_cost.saturating_mul(4)
            || params.p_cost() > self.config.parallelism.saturating_mul(4)
        {
            return PasswordVerification::Invalid;
        }
        let verifier = Argon2::default();
        if verifier
            .verify_password(password.expose().as_bytes(), &parsed)
            .is_err()
        {
            return PasswordVerification::Invalid;
        }
        if password_hash.policy_version < self.config.hash_version
            || params.m_cost() < self.config.memory_cost_kib
            || params.t_cost() < self.config.time_cost
            || params.p_cost() < self.config.parallelism
        {
            PasswordVerification::ValidNeedsUpgrade
        } else {
            PasswordVerification::Valid
        }
    }
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        let config = PasswordPolicyConfig::default();
        let params = Params::new(
            config.memory_cost_kib,
            config.time_cost,
            config.parallelism,
            None,
        )
        .unwrap_or_else(|_| Params::default());
        Self { config, params }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasswordHash {
    encoded: String,
    policy_version: u32,
}

impl PasswordHash {
    pub fn from_encoded(
        encoded: impl Into<String>,
        policy_version: u32,
    ) -> Result<Self, CredentialFailure> {
        let encoded = encoded.into();
        ParsedPasswordHash::new(&encoded).map_err(|_| CredentialFailure::Malformed)?;
        if policy_version == 0 {
            return Err(CredentialFailure::Malformed);
        }
        Ok(Self {
            encoded,
            policy_version,
        })
    }

    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    pub const fn policy_version(&self) -> u32 {
        self.policy_version
    }
}

impl fmt::Debug for PasswordHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PasswordHash")
            .field("encoded", &"[REDACTED]")
            .field("policy_version", &self.policy_version)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordVerification {
    Invalid,
    Valid,
    ValidNeedsUpgrade,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_rejects_short_long_and_control_passwords() -> Result<(), CredentialFailure> {
        let policy = PasswordPolicy::default();
        assert_eq!(
            policy.validate(&CredentialSecret::new("short")),
            Err(CredentialFailure::PasswordPolicy)
        );
        assert_eq!(
            policy.validate(&CredentialSecret::new("LongEnough!\nPassword")),
            Err(CredentialFailure::PasswordPolicy)
        );
        let long = "x".repeat(policy.config().maximum_bytes + 1);
        assert_eq!(
            policy.validate(&CredentialSecret::new(long)),
            Err(CredentialFailure::PasswordPolicy)
        );
        Ok(())
    }

    #[test]
    fn argon2id_hash_round_trip_and_redaction() -> Result<(), CredentialFailure> {
        let policy = PasswordPolicy::default();
        let secret = CredentialSecret::new("CorrectHorse!234");
        let password_hash = policy.hash(&secret)?;
        assert_eq!(
            policy.verify(&secret, &password_hash),
            PasswordVerification::Valid
        );
        assert_eq!(
            policy.verify(&CredentialSecret::new("WrongPassword!234"), &password_hash),
            PasswordVerification::Invalid
        );
        assert!(!format!("{password_hash:?}").contains("argon2"));
        Ok(())
    }
}
