// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Canonical password policy adapter.

use zeta4j_credential::{CredentialSecret, PasswordHash, PasswordPolicy, PasswordVerification};

use super::error::{AuthError, AuthResult};

/// Hash a password with the one canonical runtime policy.
pub fn hash_password(password: &str) -> AuthResult<String> {
    PasswordPolicy::default()
        .hash(&CredentialSecret::new(password))
        .map(|password_hash| password_hash.encoded().to_string())
        .map_err(|failure| match failure {
            zeta4j_credential::CredentialFailure::PasswordPolicy => AuthError::InvalidCredentials,
            _ => AuthError::HashingFailed("canonical password operation failed".to_string()),
        })
}

/// Verify a password under the canonical algorithm and parameter bounds.
pub fn verify_password(password: &str, hash: &str) -> AuthResult<bool> {
    Ok(matches!(
        verify_password_outcome(password, hash)?,
        PasswordVerification::Valid | PasswordVerification::ValidNeedsUpgrade
    ))
}

/// Verify and report whether a successful hash needs an upgrade.
pub fn verify_password_outcome(password: &str, hash: &str) -> AuthResult<PasswordVerification> {
    let password_hash = PasswordHash::from_encoded(hash, 1).map_err(|_| {
        AuthError::VerificationFailed("canonical password verification failed".to_string())
    })?;
    Ok(PasswordPolicy::default().verify(&CredentialSecret::new(password), &password_hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_password_adapter_enforces_policy_and_redacts_failures() -> AuthResult<()> {
        assert!(matches!(
            hash_password("short"),
            Err(AuthError::InvalidCredentials)
        ));
        assert!(matches!(
            hash_password("LongEnough!\nPassword"),
            Err(AuthError::InvalidCredentials)
        ));
        let hash = hash_password("Correct!Password234")?;
        assert!(verify_password("Correct!Password234", &hash)?);
        assert!(!verify_password("Wrong!Password234", &hash)?);
        Ok(())
    }
}
