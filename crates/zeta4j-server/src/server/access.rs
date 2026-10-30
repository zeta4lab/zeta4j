//! Authentication guard at server execution boundaries.
//!
//! Every active authenticated principal may use every protected surface. The
//! guard only re-resolves the installed identity against current persisted
//! state so a suspended, deleted, or rotated principal cannot keep executing.

use serde::{Deserialize, Serialize};
use zeta4j_common::error::codes;
use zeta4j_credential::Principal;

use crate::server::system::SecurityManager;
use crate::server::tenant::TenantDatabaseManager;

/// Canonical public access failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PublicError {
    AuthenticationFailed,
    TokenExpired,
    PrincipalInactive,
    DatabaseNotFound,
    PolicyInvariant,
}

impl PublicError {
    /// Returns the repository-wide stable error code.
    pub fn code(self) -> String {
        match self {
            Self::AuthenticationFailed => codes::server::AUTH_FAILED,
            Self::TokenExpired => codes::server::TOKEN_EXPIRED,
            Self::PrincipalInactive => codes::server::PRINCIPAL_INACTIVE,
            Self::DatabaseNotFound => codes::server::DATABASE_NOT_FOUND,
            Self::PolicyInvariant => codes::server::ACCESS_INVARIANT,
        }
        .full_code()
    }
}

/// Authenticated identity installed by the originating protocol session.
#[derive(Debug, Clone, Copy)]
pub struct InstalledExecutionIdentity<'a> {
    pub username: &'a str,
    pub principal_id: &'a str,
    pub security_generation: u64,
}

/// Re-resolves the installed identity and admits one Cypher execution.
pub async fn authorize_cypher_execution(
    security_manager: &SecurityManager,
    database_manager: &TenantDatabaseManager,
    installed: InstalledExecutionIdentity<'_>,
    database_name: &str,
) -> Result<(), PublicError> {
    let principal = security_manager
        .resolve_user_principal(installed.username)
        .map_err(|_| PublicError::PrincipalInactive)?;
    if principal.id.as_str() != installed.principal_id
        || principal.security_generation != installed.security_generation
    {
        return Err(PublicError::PrincipalInactive);
    }
    authorize_cypher_principal(database_manager, &principal, database_name).await
}

/// Admits a request-scoped principal against one existing database.
pub async fn authorize_cypher_principal(
    database_manager: &TenantDatabaseManager,
    principal: &Principal,
    database_name: &str,
) -> Result<(), PublicError> {
    if !principal.is_active() {
        return Err(PublicError::PrincipalInactive);
    }
    database_manager
        .get(database_name)
        .await
        .map_err(|_| PublicError::DatabaseNotFound)?;
    Ok(())
}
