// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Fail-closed startup security validation.

use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use rustls::ServerConfig;
use zeta4j_common::error::{ErrorCode, codes};
use zeta4j_common::{RuntimeProfile, is_numeric_loopback};
use zeta4j_credential::KeyVersion;
use zeta4j_server::server::auth::{Ed25519Signer, SignerReference};

use super::{BootstrapCredential, EffectiveConfig, load_bootstrap_credential};
use zeta4j_transport_security::{
    ClientAuth, IdentityRef, KeyMaterialRef, MaterialRef, TransportError, TransportMode,
    build_server_config, load_identity,
};

/// Startup validation failure with a stable public code.
#[derive(Clone, PartialEq, Eq)]
pub struct StartupSecurityError {
    code: ErrorCode,
    message: String,
}

impl StartupSecurityError {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &ErrorCode {
        &self.code
    }
}

impl std::fmt::Display for StartupSecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::fmt::Debug for StartupSecurityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for StartupSecurityError {}

/// Effective configuration that passed the common security policy.
pub struct ValidatedStartupConfig {
    config: EffectiveConfig,
    bootstrap_credential: Option<BootstrapCredential>,
    transport: ValidatedTransportConfig,
}

/// Transport material validated before any listener is bound.
pub struct ValidatedTransportConfig {
    mode: TransportMode,
    http_server: Option<Arc<ServerConfig>>,
    grpc_server: Option<Arc<ServerConfig>>,
    bolt_server: Option<Arc<ServerConfig>>,
}

impl ValidatedTransportConfig {
    pub const fn mode(&self) -> TransportMode {
        self.mode
    }

    pub fn http_server(&self) -> Option<Arc<ServerConfig>> {
        self.http_server.clone()
    }

    pub fn bolt_server(&self) -> Option<Arc<ServerConfig>> {
        self.bolt_server.clone()
    }

    pub fn grpc_server(&self) -> Option<Arc<ServerConfig>> {
        self.grpc_server.clone()
    }
}

impl Deref for ValidatedStartupConfig {
    type Target = EffectiveConfig;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl AsRef<EffectiveConfig> for ValidatedStartupConfig {
    fn as_ref(&self) -> &EffectiveConfig {
        &self.config
    }
}

impl ValidatedStartupConfig {
    /// Credential read and validated by the startup preflight, if fresh state
    /// needs one. The file is never reopened after startup side effects begin.
    pub fn bootstrap_credential(&self) -> Option<&BootstrapCredential> {
        self.bootstrap_credential.as_ref()
    }

    pub fn transport(&self) -> &ValidatedTransportConfig {
        &self.transport
    }
}

/// Validate security before any process, filesystem, logging, database, or
/// listener side effect.
pub fn validate_startup_security(
    mut config: EffectiveConfig,
) -> Result<ValidatedStartupConfig, StartupSecurityError> {
    if !config.auth_enabled {
        if config.runtime_profile != RuntimeProfile::LocalTest {
            return Err(StartupSecurityError::new(
                codes::server::BOOTSTRAP_INSECURE_PROFILE,
                "authentication can only be disabled in the explicit local-test profile",
            ));
        }
        if [&config.http_host, &config.bolt_host, &config.grpc_host]
            .into_iter()
            .any(|host| !is_numeric_loopback(host))
        {
            return Err(StartupSecurityError::new(
                codes::server::BOOTSTRAP_NON_LOOPBACK,
                "authentication-disabled listeners require numeric loopback addresses",
            ));
        }
        let transport = validate_transport(&config)?;
        return Ok(ValidatedStartupConfig {
            config,
            bootstrap_credential: None,
            transport,
        });
    }

    if let Some(path) = std::env::var_os("ZETA4J_JWT_SIGNING_KEY_FILE") {
        config.jwt_signing_key_file = PathBuf::from(path);
    }
    if let Ok(key_id) = std::env::var("ZETA4J_JWT_KEY_ID") {
        config.jwt_key_id = key_id;
    }
    if let Ok(version) = std::env::var("ZETA4J_JWT_KEY_VERSION") {
        config.jwt_key_version = version.parse().map_err(|_| {
            StartupSecurityError::new(
                codes::server::BOOTSTRAP_INVALID_SECRET,
                "ZETA4J_JWT_KEY_VERSION must be a positive integer",
            )
        })?;
    }
    validate_signer_reference(&config)?;
    let system_database = config.data_dir.join("system").join("system.redb");
    let bootstrap_credential = if !system_database.is_file() {
        match load_bootstrap_credential() {
            Ok(Some(credential)) => Some(credential),
            Ok(None) => {
                return Err(StartupSecurityError::new(
                    codes::server::BOOTSTRAP_CREDENTIAL_REQUIRED,
                    "authenticated fresh state requires an external bootstrap credential",
                ));
            }
            Err(error) => {
                return Err(StartupSecurityError::new(
                    codes::server::BOOTSTRAP_INVALID_SECRET,
                    error.to_string(),
                ));
            }
        }
    } else {
        None
    };

    let transport = validate_transport(&config)?;
    Ok(ValidatedStartupConfig {
        config,
        bootstrap_credential,
        transport,
    })
}

fn validate_transport(
    config: &EffectiveConfig,
) -> Result<ValidatedTransportConfig, StartupSecurityError> {
    let mode = TransportMode::from_str(&config.transport_mode).map_err(map_transport_error)?;
    match mode {
        TransportMode::Plaintext => Ok(ValidatedTransportConfig {
            mode,
            http_server: None,
            grpc_server: None,
            bolt_server: None,
        }),
        TransportMode::Tls => {
            if config.server_cert_ref.is_empty() || config.server_key_ref.is_empty() {
                return Err(StartupSecurityError::new(
                    codes::server::TLS_CONFIGURATION_INVALID,
                    "tls transport requires server certificate and key references",
                ));
            }
            let identity_ref = IdentityRef {
                certificate_chain: MaterialRef::from_str(&config.server_cert_ref)
                    .map_err(map_transport_error)?,
                private_key: KeyMaterialRef::from_str(&config.server_key_ref)
                    .map_err(map_transport_error)?,
            };
            let server = |alpn: &[&[u8]]| {
                build_server_config(
                    load_identity(&identity_ref).map_err(map_transport_error)?,
                    None,
                    ClientAuth::Disabled,
                    alpn,
                )
                .map(Arc::new)
                .map_err(map_transport_error)
            };
            Ok(ValidatedTransportConfig {
                mode,
                http_server: Some(server(&[b"h2", b"http/1.1"])?),
                grpc_server: Some(server(&[b"h2"])?),
                bolt_server: Some(server(&[])?),
            })
        }
    }
}

fn map_transport_error(error: TransportError) -> StartupSecurityError {
    let code = match error.code() {
        "SV-TLS-001" => codes::server::TLS_CONFIGURATION_INVALID,
        "SV-TLS-002" => codes::server::TLS_MATERIAL_INVALID,
        "SV-TLS-004" => codes::server::TLS_IDENTITY_MISMATCH,
        "SV-TLS-006" => codes::server::TLS_IDENTITY_REVOKED,
        "SV-TLS-009" => codes::server::TLS_GENERATION_REJECTED,
        _ => codes::server::TLS_CONFIGURATION_INVALID,
    };
    StartupSecurityError::new(code, error.to_string())
}

fn validate_signer_reference(config: &EffectiveConfig) -> Result<(), StartupSecurityError> {
    let version = KeyVersion::new(config.jwt_key_version).map_err(|_| {
        StartupSecurityError::new(
            codes::server::BOOTSTRAP_INVALID_SECRET,
            "JWT signing key version must be positive",
        )
    })?;
    let reference = SignerReference::new(&config.jwt_signing_key_file, &config.jwt_key_id, version)
        .map_err(|_| {
            StartupSecurityError::new(
                codes::server::BOOTSTRAP_MISSING_SECRET,
                "authentication requires a valid Ed25519 signer file reference",
            )
        })?;
    Ed25519Signer::load(&reference).map_err(|_| {
        StartupSecurityError::new(
            codes::server::BOOTSTRAP_INVALID_SECRET,
            "Ed25519 signer material is missing, insecurely permissioned, or invalid",
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ring::rand::SystemRandom;
    use ring::signature::Ed25519KeyPair;

    use super::*;

    fn secure_config() -> Result<(tempfile::TempDir, EffectiveConfig), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let signer_path = directory.path().join("signer.pk8");
        let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| std::io::Error::other("failed to generate test signing key"))?;
        std::fs::write(&signer_path, document.as_ref())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&signer_path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok((
            directory,
            EffectiveConfig {
                jwt_signing_key_file: signer_path,
                jwt_key_id: "test-key".to_string(),
                jwt_key_version: 1,
                ..EffectiveConfig::default()
            },
        ))
    }

    #[test]
    fn production_rejects_auth_disable_even_on_loopback() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_signer, base) = secure_config()?;
        let config = EffectiveConfig {
            auth_enabled: false,
            ..base
        };
        let error = validate_startup_security(config)
            .err()
            .ok_or("production no-auth must fail")?;
        assert_eq!(error.code(), &codes::server::BOOTSTRAP_INSECURE_PROFILE);
        assert!(format!("{error:?}").contains("[SV-BOOT-001]"));
        Ok(())
    }

    #[test]
    fn local_test_no_auth_requires_numeric_loopback() -> Result<(), Box<dyn std::error::Error>> {
        let (_allowed_signer, allowed_base) = secure_config()?;
        let allowed = EffectiveConfig {
            runtime_profile: RuntimeProfile::LocalTest,
            transport_mode: "plaintext".to_string(),
            auth_enabled: false,
            ..allowed_base
        };
        assert!(validate_startup_security(allowed).is_ok());

        let (_denied_signer, denied_base) = secure_config()?;
        let denied = EffectiveConfig {
            runtime_profile: RuntimeProfile::LocalTest,
            transport_mode: "plaintext".to_string(),
            auth_enabled: false,
            http_host: "localhost".to_string(),
            ..denied_base
        };
        assert_eq!(
            validate_startup_security(denied)
                .err()
                .map(|error| error.code().clone()),
            Some(codes::server::BOOTSTRAP_NON_LOOPBACK)
        );
        Ok(())
    }

    #[test]
    fn authenticated_startup_rejects_invalid_signer_references()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let invalid_path = directory.path().join("invalid.pk8");
        std::fs::write(&invalid_path, b"not-a-private-key")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&invalid_path, std::fs::Permissions::from_mode(0o600))?;
        }
        for (path, version) in [
            (PathBuf::new(), 1),
            (directory.path().join("missing.pk8"), 1),
            (invalid_path, 1),
            (directory.path().join("missing-version.pk8"), 0),
        ] {
            let config = EffectiveConfig {
                jwt_signing_key_file: path,
                jwt_key_version: version,
                runtime_profile: RuntimeProfile::LocalTest,
                transport_mode: "plaintext".to_string(),
                ..EffectiveConfig::default()
            };
            assert!(validate_startup_security(config).is_err());
        }
        Ok(())
    }

    #[test]
    fn effective_config_debug_never_contains_secrets() -> Result<(), Box<dyn std::error::Error>> {
        let (_signer, mut config) = secure_config()?;
        let signer_canary = config.jwt_signing_key_file.display().to_string();
        let reference_canary = "/private/transport/reference-canary";
        config.server_cert_ref = reference_canary.to_string();
        config.server_key_ref = reference_canary.to_string();
        let debug = format!("{config:?}");
        assert!(!debug.contains(&signer_canary));
        assert!(!debug.contains(reference_canary));
        Ok(())
    }

    #[test]
    fn existing_system_database_does_not_require_bootstrap_source()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let system_dir = temp.path().join("system");
        std::fs::create_dir_all(&system_dir)?;
        std::fs::write(system_dir.join("system.redb"), b"existing")?;
        let (_signer, base) = secure_config()?;
        let config = EffectiveConfig {
            data_dir: temp.path().to_path_buf(),
            runtime_profile: RuntimeProfile::LocalTest,
            transport_mode: "plaintext".to_string(),
            ..base
        };

        assert!(validate_startup_security(config).is_ok());
        Ok(())
    }

    #[test]
    fn production_rejects_empty_signer_reference() {
        let config = EffectiveConfig {
            jwt_signing_key_file: PathBuf::new(),
            ..EffectiveConfig::default()
        };
        assert_eq!(
            validate_startup_security(config)
                .err()
                .map(|error| error.code().clone()),
            Some(codes::server::BOOTSTRAP_MISSING_SECRET)
        );
    }

    fn existing_state(
        base: EffectiveConfig,
    ) -> Result<(tempfile::TempDir, EffectiveConfig), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let system_dir = temp.path().join("system");
        std::fs::create_dir_all(&system_dir)?;
        std::fs::write(system_dir.join("system.redb"), b"existing")?;
        let config = EffectiveConfig {
            data_dir: temp.path().to_path_buf(),
            ..base
        };
        Ok((temp, config))
    }

    #[test]
    fn default_transport_is_plaintext() {
        assert_eq!(EffectiveConfig::default().transport_mode, "plaintext");
        assert!(!EffectiveConfig::default().tls_enabled());
    }

    #[test]
    fn plaintext_transport_accepts_non_loopback_authenticated_listeners()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_signer, base) = secure_config()?;
        let (_state, config) = existing_state(EffectiveConfig {
            transport_mode: "plaintext".to_string(),
            http_host: "0.0.0.0".to_string(),
            bolt_host: "0.0.0.0".to_string(),
            grpc_host: "0.0.0.0".to_string(),
            ..base
        })?;
        let validated = validate_startup_security(config)?;
        assert_eq!(validated.transport().mode(), TransportMode::Plaintext);
        assert!(validated.transport().http_server().is_none());
        Ok(())
    }

    #[test]
    fn tls_transport_needs_only_server_certificate_and_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])?;
        let directory = tempfile::tempdir()?;
        let cert_path = directory.path().join("server.crt");
        let key_path = directory.path().join("server.key");
        std::fs::write(&cert_path, certified.cert.pem())?;
        std::fs::write(&key_path, certified.signing_key.serialize_pem())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }
        let (_signer, base) = secure_config()?;
        let (_state, config) = existing_state(EffectiveConfig {
            transport_mode: "tls".to_string(),
            server_cert_ref: format!("file:{}", cert_path.display()),
            server_key_ref: format!("file:{}", key_path.display()),
            ..base
        })?;
        let validated = validate_startup_security(config)?;
        assert_eq!(validated.transport().mode(), TransportMode::Tls);
        assert!(validated.transport().http_server().is_some());
        assert!(validated.transport().bolt_server().is_some());
        assert!(validated.transport().grpc_server().is_some());
        Ok(())
    }

    #[test]
    fn tls_rejects_missing_server_certificate_before_listener_start()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_signer, base) = secure_config()?;
        let (_state, config) = existing_state(EffectiveConfig {
            transport_mode: "tls".to_string(),
            ..base
        })?;
        let error = validate_transport(&config)
            .err()
            .ok_or("tls accepted missing server certificate")?;
        assert_eq!(error.code(), &codes::server::TLS_CONFIGURATION_INVALID);
        Ok(())
    }
}
