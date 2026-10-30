//! Protocol-independent credential lifecycle domain.
//!
//! This crate deliberately has no HTTP, gRPC, storage, or runtime
//! dependencies. Protocol adapters validate external syntax and pass closed
//! domain values through this boundary.

mod claims;
mod ids;
mod password;
mod principal;
mod secret;
mod state;

pub use claims::{
    AccessClaims, ClaimValidation, CredentialAudience, CredentialScope, RefreshClaims, TokenClaims,
    TokenType, ValidatedClaims,
};
pub use ids::{
    CorrelationId, CredentialId, CredentialSubjectId, KeyVersion, SessionId, TokenFamilyId, TokenId,
};
pub use password::{PasswordHash, PasswordPolicy, PasswordPolicyConfig, PasswordVerification};
pub use principal::{
    InvalidStableId, Principal, PrincipalId, PrincipalKind, PrincipalStatus, StableId, TenantId,
    constant_time_equal,
};
pub use secret::{CredentialSecret, SecretDigest, digest_secret, verify_digest};
pub use state::{
    ApiKeyRecord, AuthSession, CredentialEvent, CredentialEventKind, CredentialFailure,
    CredentialKind, CredentialStatus, RefreshFamily, RefreshTransition, SessionStatus,
    SigningKeyStatus, SigningVerificationKey, close_session, consume_refresh,
    revoke_refresh_family, transition_api_key, transition_signing_key,
};
