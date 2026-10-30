//! External initial-administrator credential loading.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use secrecy::SecretString;
use serde::Deserialize;
use zeta4j_common::BOOTSTRAP_ENVELOPE_VERSION;
use zeta4j_common::error::codes;

const DEFAULT_BOOTSTRAP_TTL_MINUTES: i64 = 15;

/// One-time initial administrator credential.
pub struct BootstrapCredential {
    pub username: String,
    pub password: SecretString,
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for BootstrapCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BootstrapCredential")
            .field("username", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialEnvelope {
    version: u8,
    username: String,
    password: String,
    expires_at_unix: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum BootstrapCredentialError {
    #[error("[{code}] bootstrap credential source is invalid: {message}")]
    Invalid {
        code: zeta4j_common::ErrorCode,
        message: String,
    },
    #[error(
        "[{code}] authenticated fresh state requires ZETA4J_BOOTSTRAP_CREDENTIAL_FILE or ZETA4J_BOOTSTRAP_USERNAME/ZETA4J_BOOTSTRAP_PASSWORD"
    )]
    Missing { code: zeta4j_common::ErrorCode },
}

impl BootstrapCredentialError {
    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid {
            code: codes::server::BOOTSTRAP_INVALID_SECRET,
            message: message.into(),
        }
    }

    pub fn missing() -> Self {
        Self::Missing {
            code: codes::server::BOOTSTRAP_CREDENTIAL_REQUIRED,
        }
    }
}

/// Load a credential without ever accepting the secret in process arguments.
pub fn load_bootstrap_credential() -> Result<Option<BootstrapCredential>, BootstrapCredentialError>
{
    let direct_expiry = std::env::var("ZETA4J_BOOTSTRAP_EXPIRES_AT_UNIX").ok();
    if let Some(path) = std::env::var_os("ZETA4J_BOOTSTRAP_CREDENTIAL_FILE") {
        if std::env::var_os("ZETA4J_BOOTSTRAP_USERNAME").is_some()
            || std::env::var_os("ZETA4J_BOOTSTRAP_PASSWORD").is_some()
            || direct_expiry.is_some()
        {
            return Err(BootstrapCredentialError::invalid(
                "bootstrap file and direct environment sources cannot be combined",
            ));
        }
        return read_credential_file(Path::new(&path)).map(Some);
    }

    let username = std::env::var("ZETA4J_BOOTSTRAP_USERNAME").ok();
    let password = std::env::var("ZETA4J_BOOTSTRAP_PASSWORD").ok();
    match (username, password, direct_expiry) {
        (None, None, None) => Ok(None),
        (Some(username), Some(password), expiry) => {
            let expires_at = direct_credential_expiry(expiry)?;
            validate_fields(&username, &password, expires_at)?;
            Ok(Some(BootstrapCredential {
                username,
                password: password.into(),
                expires_at,
            }))
        }
        _ => Err(BootstrapCredentialError::invalid(
            "bootstrap username and password must be provided together",
        )),
    }
}

fn direct_credential_expiry(
    value: Option<String>,
) -> Result<DateTime<Utc>, BootstrapCredentialError> {
    let Some(value) = value else {
        return Ok(Utc::now() + Duration::minutes(DEFAULT_BOOTSTRAP_TTL_MINUTES));
    };
    let timestamp = value
        .parse::<i64>()
        .map_err(|_| BootstrapCredentialError::invalid("invalid expiry"))?;
    DateTime::from_timestamp(timestamp, 0)
        .ok_or_else(|| BootstrapCredentialError::invalid("invalid expiry"))
}

fn read_credential_file(path: &Path) -> Result<BootstrapCredential, BootstrapCredentialError> {
    let content = zeta4j_common::load_secret_file(path, "ZETA4J_BOOTSTRAP_CREDENTIAL_FILE")
        .map_err(|error| BootstrapCredentialError::invalid(error.to_string()))?;
    let envelope: CredentialEnvelope = toml::from_str(&content)
        .map_err(|_| BootstrapCredentialError::invalid("credential envelope is not valid TOML"))?;
    if envelope.version != BOOTSTRAP_ENVELOPE_VERSION {
        return Err(BootstrapCredentialError::invalid(
            "unsupported credential envelope version",
        ));
    }
    let expires_at = DateTime::from_timestamp(envelope.expires_at_unix, 0)
        .ok_or_else(|| BootstrapCredentialError::invalid("invalid expiry"))?;
    validate_fields(&envelope.username, &envelope.password, expires_at)?;
    Ok(BootstrapCredential {
        username: envelope.username,
        password: envelope.password.into(),
        expires_at,
    })
}

fn validate_fields(
    username: &str,
    password: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), BootstrapCredentialError> {
    if username.trim().is_empty()
        || zeta4j_credential::PasswordPolicy::default()
            .validate(&zeta4j_credential::CredentialSecret::new(password))
            .is_err()
    {
        return Err(BootstrapCredentialError::invalid(
            "credential fields do not meet the bootstrap policy",
        ));
    }
    if expires_at <= Utc::now() {
        return Err(BootstrapCredentialError::invalid(
            "bootstrap credential has expired",
        ));
    }
    Ok(())
}

/// Resolve a path for diagnostics without exposing credential contents.
pub fn configured_credential_path() -> Option<PathBuf> {
    std::env::var_os("ZETA4J_BOOTSTRAP_CREDENTIAL_FILE").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use secrecy::ExposeSecret;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn credential_file_requires_owner_only_permissions() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let path = temp.path().join("bootstrap.toml");
        fs::write(
            &path,
            zeta4j_common::render_bootstrap_envelope(
                "operator-91ac",
                "TemporaryCredential!234",
                u64::try_from((Utc::now() + Duration::minutes(5)).timestamp())?,
            ),
        )?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        assert!(read_credential_file(&path).is_err());

        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let credential = read_credential_file(&path)?;
        assert_eq!(credential.username, "operator-91ac");
        assert_eq!(
            credential.password.expose_secret(),
            "TemporaryCredential!234"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn credential_file_rejects_symbolic_links() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temp = tempfile::tempdir()?;
        let target = temp.path().join("bootstrap.toml");
        let link = temp.path().join("bootstrap-link.toml");
        fs::write(
            &target,
            zeta4j_common::render_bootstrap_envelope(
                "operator-91ac",
                "TemporaryCredential!234",
                u64::try_from((Utc::now() + Duration::minutes(5)).timestamp())?,
            ),
        )?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
        symlink(&target, &link)?;

        assert!(read_credential_file(&link).is_err());
        Ok(())
    }

    #[test]
    fn direct_source_expiry_defaults_to_the_bootstrap_ttl() {
        assert!(direct_credential_expiry(None).is_ok());
    }
}
