use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// A short-lived raw credential whose diagnostics are always redacted.
pub struct CredentialSecret(SecretString);

impl CredentialSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for CredentialSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CredentialSecret([REDACTED])")
    }
}

/// Domain-separated digest suitable for persisted token lookup.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretDigest([u8; 32]);

impl SecretDigest {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SecretDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretDigest([REDACTED])")
    }
}

pub fn digest_secret(domain: &[u8], secret: &CredentialSecret) -> SecretDigest {
    let mut hasher = Sha256::new();
    hasher.update(b"zeta4j-credential-v1\0");
    hasher.update((domain.len() as u64).to_be_bytes());
    hasher.update(domain);
    hasher.update(secret.expose().as_bytes());
    SecretDigest(hasher.finalize().into())
}

pub fn verify_digest(expected: &SecretDigest, actual: &SecretDigest) -> bool {
    bool::from(expected.as_bytes().ct_eq(actual.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_secret_is_redacted_and_digest_is_domain_separated() {
        let secret = CredentialSecret::new("canary-secret");
        assert_eq!(format!("{secret:?}"), "CredentialSecret([REDACTED])");
        let refresh = digest_secret(b"refresh", &secret);
        let api_key = digest_secret(b"api-key", &secret);
        assert_ne!(refresh, api_key);
        assert!(verify_digest(&refresh, &refresh));
        assert!(!verify_digest(&refresh, &api_key));
        assert!(!format!("{refresh:?}").contains("canary-secret"));
    }
}
