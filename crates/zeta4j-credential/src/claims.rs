use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    CredentialFailure, CredentialSubjectId, KeyVersion, SessionId, TokenFamilyId, TokenId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialAudience {
    Http,
    Bolt,
    Grpc,
    Cli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialScope {
    Access,
    Refresh,
    Api,
    Administration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    Access,
    Refresh,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessClaims {
    pub issuer: String,
    pub audience: CredentialAudience,
    pub subject: CredentialSubjectId,
    pub scope: BTreeSet<CredentialScope>,
    pub issued_at: i64,
    pub not_before: i64,
    pub expires_at: i64,
    pub token_id: TokenId,
    pub session_id: SessionId,
    pub family_id: TokenFamilyId,
    pub key_version: KeyVersion,
    pub security_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefreshClaims {
    pub issuer: String,
    pub audience: CredentialAudience,
    pub subject: CredentialSubjectId,
    pub scope: BTreeSet<CredentialScope>,
    pub issued_at: i64,
    pub not_before: i64,
    pub expires_at: i64,
    pub token_id: TokenId,
    pub session_id: SessionId,
    pub family_id: TokenFamilyId,
    pub key_version: KeyVersion,
    pub security_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "token_type", rename_all = "snake_case")]
pub enum TokenClaims {
    Access(AccessClaims),
    Refresh(RefreshClaims),
}

impl TokenClaims {
    fn common(&self) -> CommonClaims<'_> {
        match self {
            Self::Access(claims) => CommonClaims {
                token_type: TokenType::Access,
                issuer: &claims.issuer,
                audience: claims.audience,
                subject: &claims.subject,
                scope: &claims.scope,
                issued_at: claims.issued_at,
                not_before: claims.not_before,
                expires_at: claims.expires_at,
                token_id: &claims.token_id,
                session_id: &claims.session_id,
                family_id: &claims.family_id,
                key_version: claims.key_version,
                security_generation: claims.security_generation,
            },
            Self::Refresh(claims) => CommonClaims {
                token_type: TokenType::Refresh,
                issuer: &claims.issuer,
                audience: claims.audience,
                subject: &claims.subject,
                scope: &claims.scope,
                issued_at: claims.issued_at,
                not_before: claims.not_before,
                expires_at: claims.expires_at,
                token_id: &claims.token_id,
                session_id: &claims.session_id,
                family_id: &claims.family_id,
                key_version: claims.key_version,
                security_generation: claims.security_generation,
            },
        }
    }
}

struct CommonClaims<'a> {
    token_type: TokenType,
    issuer: &'a str,
    audience: CredentialAudience,
    subject: &'a CredentialSubjectId,
    scope: &'a BTreeSet<CredentialScope>,
    issued_at: i64,
    not_before: i64,
    expires_at: i64,
    token_id: &'a TokenId,
    session_id: &'a SessionId,
    family_id: &'a TokenFamilyId,
    key_version: KeyVersion,
    security_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimValidation {
    issuer: String,
    audience: CredentialAudience,
    token_type: TokenType,
    required_scope: CredentialScope,
    maximum_ttl_seconds: i64,
    clock_skew_seconds: i64,
}

impl ClaimValidation {
    pub fn new(
        issuer: impl Into<String>,
        audience: CredentialAudience,
        token_type: TokenType,
        required_scope: CredentialScope,
        maximum_ttl_seconds: i64,
        clock_skew_seconds: i64,
    ) -> Result<Self, CredentialFailure> {
        let issuer = issuer.into();
        if issuer.is_empty()
            || maximum_ttl_seconds <= 0
            || clock_skew_seconds < 0
            || clock_skew_seconds > maximum_ttl_seconds
        {
            return Err(CredentialFailure::Policy);
        }
        let valid_scope = matches!(
            (token_type, required_scope),
            (TokenType::Access, CredentialScope::Access)
                | (TokenType::Refresh, CredentialScope::Refresh)
        );
        if !valid_scope {
            return Err(CredentialFailure::Policy);
        }
        Ok(Self {
            issuer,
            audience,
            token_type,
            required_scope,
            maximum_ttl_seconds,
            clock_skew_seconds,
        })
    }

    pub fn validate(
        &self,
        claims: &TokenClaims,
        now: i64,
    ) -> Result<ValidatedClaims, CredentialFailure> {
        let common = claims.common();
        if common.issuer != self.issuer {
            return Err(CredentialFailure::Issuer);
        }
        if common.audience != self.audience {
            return Err(CredentialFailure::Audience);
        }
        if common.token_type != self.token_type {
            return Err(CredentialFailure::TokenType);
        }
        if common.scope.len() != 1 || !common.scope.contains(&self.required_scope) {
            return Err(CredentialFailure::Scope);
        }
        if common.security_generation == 0 {
            return Err(CredentialFailure::Malformed);
        }
        if common.not_before < common.issued_at
            || common.expires_at <= common.not_before
            || common.expires_at.saturating_sub(common.issued_at) > self.maximum_ttl_seconds
        {
            return Err(CredentialFailure::Ttl);
        }
        if now.saturating_add(self.clock_skew_seconds) < common.not_before {
            return Err(CredentialFailure::NotYetValid);
        }
        if now.saturating_sub(self.clock_skew_seconds) >= common.expires_at {
            return Err(CredentialFailure::Expired);
        }
        Ok(ValidatedClaims {
            token_type: common.token_type,
            subject: common.subject.clone(),
            token_id: common.token_id.clone(),
            session_id: common.session_id.clone(),
            family_id: common.family_id.clone(),
            key_version: common.key_version,
            security_generation: common.security_generation,
            expires_at: common.expires_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedClaims {
    pub token_type: TokenType,
    pub subject: CredentialSubjectId,
    pub token_id: TokenId,
    pub session_id: SessionId,
    pub family_id: TokenFamilyId,
    pub key_version: KeyVersion,
    pub security_generation: u64,
    pub expires_at: i64,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn access_claims() -> Result<TokenClaims, CredentialFailure> {
        Ok(TokenClaims::Access(AccessClaims {
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
        }))
    }

    fn validator() -> Result<ClaimValidation, CredentialFailure> {
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
    fn exact_access_claims_validate() -> Result<(), CredentialFailure> {
        let validated = validator()?.validate(&access_claims()?, 1_100)?;
        assert_eq!(validated.token_type, TokenType::Access);
        Ok(())
    }

    #[test]
    fn claim_confusion_matrix_is_rejected() -> Result<(), CredentialFailure> {
        let validator = validator()?;
        let mut cases = Vec::new();

        let mut wrong_issuer = access_claims()?;
        if let TokenClaims::Access(claims) = &mut wrong_issuer {
            claims.issuer = "attacker".to_string();
        }
        cases.push((wrong_issuer, CredentialFailure::Issuer));

        let mut wrong_audience = access_claims()?;
        if let TokenClaims::Access(claims) = &mut wrong_audience {
            claims.audience = CredentialAudience::Grpc;
        }
        cases.push((wrong_audience, CredentialFailure::Audience));

        let mut wrong_scope = access_claims()?;
        if let TokenClaims::Access(claims) = &mut wrong_scope {
            claims.scope = BTreeSet::from([CredentialScope::Administration]);
        }
        cases.push((wrong_scope, CredentialFailure::Scope));

        let mut excessive_ttl = access_claims()?;
        if let TokenClaims::Access(claims) = &mut excessive_ttl {
            claims.expires_at = 1_301;
        }
        cases.push((excessive_ttl, CredentialFailure::Ttl));

        for (claims, expected) in cases {
            assert_eq!(validator.validate(&claims, 1_100), Err(expected));
        }
        assert_eq!(
            validator.validate(&access_claims()?, 1_306),
            Err(CredentialFailure::Expired)
        );
        assert_eq!(
            validator.validate(&access_claims()?, 994),
            Err(CredentialFailure::NotYetValid)
        );
        Ok(())
    }
}
