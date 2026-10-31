// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Ed25519 credential token signing and exact verification.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
use zeta4j_credential::{
    AccessClaims, ClaimValidation, CredentialFailure, CredentialSecret, KeyVersion, RefreshClaims,
    SigningKeyStatus, SigningVerificationKey, TokenClaims, ValidatedClaims,
};

use crate::server::system::credential::CredentialSigner;

const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_SEGMENT_BYTES: usize = 12 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerReference {
    path: PathBuf,
    key_id: String,
    version: KeyVersion,
}

impl SignerReference {
    pub fn new(
        path: impl Into<PathBuf>,
        key_id: impl Into<String>,
        version: KeyVersion,
    ) -> Result<Self, CredentialFailure> {
        let path = path.into();
        let key_id = key_id.into();
        if path.as_os_str().is_empty()
            || key_id.is_empty()
            || key_id.len() > 128
            || !key_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(CredentialFailure::Policy);
        }
        Ok(Self {
            path,
            key_id,
            version,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub const fn version(&self) -> KeyVersion {
        self.version
    }
}

pub struct Ed25519Signer {
    key_pair: Arc<Ed25519KeyPair>,
    key_id: String,
    version: KeyVersion,
}

impl std::fmt::Debug for Ed25519Signer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Ed25519Signer")
            .field("key_pair", &"[REDACTED]")
            .field("key_id", &self.key_id)
            .field("version", &self.version)
            .finish()
    }
}

impl Ed25519Signer {
    pub fn load(reference: &SignerReference) -> Result<Self, CredentialFailure> {
        let metadata =
            std::fs::metadata(reference.path()).map_err(|_| CredentialFailure::Unavailable)?;
        if !metadata.is_file() {
            return Err(CredentialFailure::Policy);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(CredentialFailure::Policy);
            }
        }
        let bytes = Zeroizing::new(
            std::fs::read(reference.path()).map_err(|_| CredentialFailure::Unavailable)?,
        );
        let key_pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(bytes.as_slice())
            .map_err(|_| CredentialFailure::Policy)?;
        Ok(Self {
            key_pair: Arc::new(key_pair),
            key_id: reference.key_id().to_string(),
            version: reference.version(),
        })
    }

    pub fn verification_key(
        &self,
        status: SigningKeyStatus,
        prepared_at: i64,
    ) -> SigningVerificationKey {
        SigningVerificationKey {
            version: self.version,
            key_id: self.key_id.clone(),
            public_key: self.key_pair.public_key().as_ref().to_vec(),
            status,
            prepared_at,
            activated_at: (status == SigningKeyStatus::Active).then_some(prepared_at),
            verify_until: None,
        }
    }

    fn sign(&self, claims: &TokenClaims) -> Result<CredentialSecret, CredentialFailure> {
        let header = TokenHeader {
            algorithm: "EdDSA".to_string(),
            token_type: "JWT".to_string(),
            key_id: self.key_id.clone(),
            key_version: self.version,
        };
        let header = serde_json::to_vec(&header).map_err(|_| CredentialFailure::Internal)?;
        let claims = serde_json::to_vec(claims).map_err(|_| CredentialFailure::Internal)?;
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header),
            URL_SAFE_NO_PAD.encode(claims)
        );
        let signature = self.key_pair.sign(signing_input.as_bytes());
        Ok(CredentialSecret::new(format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        )))
    }
}

impl CredentialSigner for Ed25519Signer {
    fn key_version(&self) -> KeyVersion {
        self.version
    }

    fn verification_key(
        &self,
        status: SigningKeyStatus,
        prepared_at: i64,
    ) -> SigningVerificationKey {
        Ed25519Signer::verification_key(self, status, prepared_at)
    }

    fn sign_access(&self, claims: &AccessClaims) -> Result<CredentialSecret, CredentialFailure> {
        self.sign(&TokenClaims::Access(claims.clone()))
    }

    fn sign_refresh(&self, claims: &RefreshClaims) -> Result<CredentialSecret, CredentialFailure> {
        self.sign(&TokenClaims::Refresh(claims.clone()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenHeader {
    #[serde(rename = "alg")]
    algorithm: String,
    #[serde(rename = "typ")]
    token_type: String,
    #[serde(rename = "kid")]
    key_id: String,
    key_version: KeyVersion,
}

pub struct CredentialTokenService {
    signer: Arc<Ed25519Signer>,
    verification_keys: std::sync::RwLock<BTreeMap<KeyVersion, SigningVerificationKey>>,
}

impl std::fmt::Debug for CredentialTokenService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialTokenService")
            .field("signer", &self.signer)
            .field(
                "verification_key_versions",
                &self
                    .verification_keys
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .keys()
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl CredentialTokenService {
    pub fn new(
        signer: Arc<Ed25519Signer>,
        verification_keys: Vec<SigningVerificationKey>,
    ) -> Result<Self, CredentialFailure> {
        let mut key_map = BTreeMap::new();
        for key in verification_keys {
            if key.public_key.len() != 32
                || key.key_id.is_empty()
                || key_map.insert(key.version, key).is_some()
            {
                return Err(CredentialFailure::Policy);
            }
        }
        let Some(active) = key_map.get(&signer.version) else {
            return Err(CredentialFailure::Unavailable);
        };
        if !matches!(
            active.status,
            SigningKeyStatus::Prepared | SigningKeyStatus::Active | SigningKeyStatus::VerifyOnly
        ) || active.key_id != signer.key_id
            || active.public_key != signer.key_pair.public_key().as_ref()
        {
            return Err(CredentialFailure::Unavailable);
        }
        Ok(Self {
            signer,
            verification_keys: std::sync::RwLock::new(key_map),
        })
    }

    pub fn signer(&self) -> Arc<Ed25519Signer> {
        self.signer.clone()
    }

    pub fn sign_access(
        &self,
        claims: &AccessClaims,
    ) -> Result<CredentialSecret, CredentialFailure> {
        self.ensure_active_signer()?;
        self.signer.sign_access(claims)
    }

    pub fn sign_refresh(
        &self,
        claims: &RefreshClaims,
    ) -> Result<CredentialSecret, CredentialFailure> {
        self.ensure_active_signer()?;
        self.signer.sign_refresh(claims)
    }

    pub fn replace_verification_keys(
        &self,
        verification_keys: Vec<SigningVerificationKey>,
    ) -> Result<(), CredentialFailure> {
        let replacement = Self::validated_key_map(&self.signer, verification_keys)?;
        *self
            .verification_keys
            .write()
            .map_err(|_| CredentialFailure::Internal)? = replacement;
        Ok(())
    }

    pub(crate) fn ensure_active_signer(&self) -> Result<(), CredentialFailure> {
        let keys = self
            .verification_keys
            .read()
            .map_err(|_| CredentialFailure::Internal)?;
        let signer = keys
            .get(&self.signer.version)
            .ok_or(CredentialFailure::Unavailable)?;
        if signer.status == SigningKeyStatus::Active {
            Ok(())
        } else {
            Err(CredentialFailure::Unavailable)
        }
    }

    fn validated_key_map(
        signer: &Ed25519Signer,
        verification_keys: Vec<SigningVerificationKey>,
    ) -> Result<BTreeMap<KeyVersion, SigningVerificationKey>, CredentialFailure> {
        let mut key_map = BTreeMap::new();
        for key in verification_keys {
            if key.public_key.len() != 32
                || key.key_id.is_empty()
                || key_map.insert(key.version, key).is_some()
            {
                return Err(CredentialFailure::Policy);
            }
        }
        let local = key_map
            .get(&signer.version)
            .ok_or(CredentialFailure::Unavailable)?;
        if local.key_id != signer.key_id
            || local.public_key != signer.key_pair.public_key().as_ref()
        {
            return Err(CredentialFailure::Unavailable);
        }
        Ok(key_map)
    }

    pub fn validate(
        &self,
        token: &CredentialSecret,
        validation: &ClaimValidation,
        now: i64,
    ) -> Result<ValidatedClaims, CredentialFailure> {
        let raw = token.expose();
        if raw.len() > MAX_TOKEN_BYTES {
            return Err(CredentialFailure::Malformed);
        }
        let mut segments = raw.split('.');
        let Some(header_segment) = segments.next() else {
            return Err(CredentialFailure::Malformed);
        };
        let Some(claim_segment) = segments.next() else {
            return Err(CredentialFailure::Malformed);
        };
        let Some(signature_segment) = segments.next() else {
            return Err(CredentialFailure::Malformed);
        };
        if segments.next().is_some()
            || header_segment.len() > MAX_SEGMENT_BYTES
            || claim_segment.len() > MAX_SEGMENT_BYTES
            || signature_segment.len() > MAX_SEGMENT_BYTES
        {
            return Err(CredentialFailure::Malformed);
        }
        let header_bytes = URL_SAFE_NO_PAD
            .decode(header_segment)
            .map_err(|_| CredentialFailure::Malformed)?;
        let header: TokenHeader =
            serde_json::from_slice(&header_bytes).map_err(|_| CredentialFailure::Malformed)?;
        if header.algorithm != "EdDSA" || header.token_type != "JWT" {
            tracing::warn!("credential token header selected an unsupported signing contract");
            return Err(CredentialFailure::KeyVersion);
        }
        let verification_keys = self
            .verification_keys
            .read()
            .map_err(|_| CredentialFailure::Internal)?;
        let Some(key) = verification_keys.get(&header.key_version) else {
            tracing::warn!(
                key_version = header.key_version.get(),
                "credential token selected an unknown signing key version"
            );
            return Err(CredentialFailure::KeyVersion);
        };
        if key.key_id != header.key_id {
            tracing::warn!(
                key_version = header.key_version.get(),
                "credential token key ID does not match its signing key version"
            );
            return Err(CredentialFailure::KeyVersion);
        }
        let key_usable = match key.status {
            SigningKeyStatus::Active => true,
            SigningKeyStatus::VerifyOnly => key.verify_until.is_some_and(|deadline| now < deadline),
            SigningKeyStatus::Prepared
            | SigningKeyStatus::Retired
            | SigningKeyStatus::Compromised => false,
        };
        if !key_usable {
            tracing::warn!(
                key_version = header.key_version.get(),
                key_status = ?key.status,
                "credential token selected a signing key outside its verification window"
            );
            return Err(CredentialFailure::KeyVersion);
        }
        let signature = URL_SAFE_NO_PAD
            .decode(signature_segment)
            .map_err(|_| CredentialFailure::Malformed)?;
        let signing_input = format!("{header_segment}.{claim_segment}");
        UnparsedPublicKey::new(&ED25519, &key.public_key)
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| CredentialFailure::InvalidCredential)?;
        let claim_bytes = URL_SAFE_NO_PAD
            .decode(claim_segment)
            .map_err(|_| CredentialFailure::Malformed)?;
        let claims: TokenClaims =
            serde_json::from_slice(&claim_bytes).map_err(|_| CredentialFailure::Malformed)?;
        let validated = validation.validate(&claims, now)?;
        if validated.key_version != header.key_version {
            tracing::warn!("credential token header and claim key versions differ");
            return Err(CredentialFailure::KeyVersion);
        }
        Ok(validated)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ring::rand::SystemRandom;
    use zeta4j_credential::{
        CredentialAudience, CredentialScope, CredentialSubjectId, SessionId, TokenFamilyId,
        TokenId, TokenType,
    };

    use super::*;

    fn service() -> Result<(tempfile::TempDir, CredentialTokenService), CredentialFailure> {
        let random = SystemRandom::new();
        let document =
            Ed25519KeyPair::generate_pkcs8(&random).map_err(|_| CredentialFailure::Internal)?;
        let temp = tempfile::tempdir().map_err(|_| CredentialFailure::Internal)?;
        let path = temp.path().join("signer.pk8");
        std::fs::write(&path, document.as_ref()).map_err(|_| CredentialFailure::Internal)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|_| CredentialFailure::Internal)?;
        }
        let reference = SignerReference::new(&path, "key-1", KeyVersion::new(1)?)?;
        let signer = Arc::new(Ed25519Signer::load(&reference)?);
        let verification_key = signer.verification_key(SigningKeyStatus::Active, 1_000);
        let service = CredentialTokenService::new(signer, vec![verification_key])?;
        Ok((temp, service))
    }

    fn claims() -> Result<AccessClaims, CredentialFailure> {
        Ok(AccessClaims {
            issuer: "zeta4j".to_string(),
            audience: CredentialAudience::Http,
            subject: CredentialSubjectId::new(),
            scope: BTreeSet::from([CredentialScope::Access]),
            issued_at: 1_000,
            not_before: 1_000,
            expires_at: 1_300,
            token_id: TokenId::new(),
            session_id: SessionId::new(),
            family_id: TokenFamilyId::new(),
            key_version: KeyVersion::new(1)?,
            security_generation: 1,
        })
    }

    fn validation() -> Result<ClaimValidation, CredentialFailure> {
        ClaimValidation::new(
            "zeta4j",
            CredentialAudience::Http,
            TokenType::Access,
            CredentialScope::Access,
            300,
            5,
        )
    }

    #[test]
    fn ed25519_round_trip_validates_exact_header_and_claims() -> Result<(), CredentialFailure> {
        let (_temp, service) = service()?;
        let claims = claims()?;
        let token = service.sign_access(&claims)?;
        let validated = service.validate(&token, &validation()?, 1_100)?;
        assert_eq!(validated.subject, claims.subject);
        assert!(!format!("{service:?}").contains(token.expose()));
        Ok(())
    }

    #[test]
    fn signature_header_and_claim_substitution_fail_closed() -> Result<(), CredentialFailure> {
        let (_temp, service) = service()?;
        let token = service.sign_access(&claims()?)?;
        let mut raw = token.expose().to_string();
        raw.push('x');
        assert!(matches!(
            service.validate(&CredentialSecret::new(raw), &validation()?, 1_100),
            Err(CredentialFailure::InvalidCredential) | Err(CredentialFailure::Malformed)
        ));

        let segments: Vec<_> = token.expose().split('.').collect();
        if segments.len() != 3 {
            return Err(CredentialFailure::Internal);
        }
        let header = TokenHeader {
            algorithm: "HS256".to_string(),
            token_type: "JWT".to_string(),
            key_id: "key-1".to_string(),
            key_version: KeyVersion::new(1)?,
        };
        let encoded_header = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&header).map_err(|_| CredentialFailure::Internal)?);
        let substituted =
            CredentialSecret::new(format!("{encoded_header}.{}.{}", segments[1], segments[2]));
        assert_eq!(
            service.validate(&substituted, &validation()?, 1_100),
            Err(CredentialFailure::KeyVersion)
        );
        Ok(())
    }
}
