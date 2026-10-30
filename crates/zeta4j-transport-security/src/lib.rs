//! Canonical TLS and trust-material primitives.
//!
//! This crate owns transport policy and cryptographic material loading. Protocol
//! adapters consume validated rustls configurations instead of implementing
//! their own certificate verification.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, CertificateRevocationListDer, PrivateKeyDer, ServerName};
use rustls::server::WebPkiClientVerifier;
use rustls::sign::{CertifiedKey, SingleCertAndKey};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pki_types::pem::PemObject;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;
use zeta4j_common::codes;

const MAX_CERTIFICATE_FILE_BYTES: u64 = 1024 * 1024;
const MAX_PRIVATE_KEY_FILE_BYTES: u64 = 128 * 1024;
const MAX_TRUST_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Runtime transport mode.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransportMode {
    /// Unencrypted listeners.
    #[default]
    Plaintext,
    /// TLS on every product endpoint.
    Tls,
}

impl FromStr for TransportMode {
    type Err = TransportError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "plaintext" => Ok(Self::Plaintext),
            "tls" => Ok(Self::Tls),
            _ => Err(TransportError::configuration(
                "transport mode must be plaintext or tls",
            )),
        }
    }
}

/// Reference to public certificate or trust material.
#[derive(Clone, PartialEq, Eq)]
pub enum MaterialRef {
    File(PathBuf),
    KubernetesProjected(PathBuf),
}

impl std::fmt::Debug for MaterialRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(_) => formatter.write_str("MaterialRef::File([REDACTED])"),
            Self::KubernetesProjected(_) => {
                formatter.write_str("MaterialRef::KubernetesProjected([REDACTED])")
            }
        }
    }
}

impl FromStr for MaterialRef {
    type Err = TransportError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(path) = value.strip_prefix("file:") {
            return non_empty_path(path).map(Self::File);
        }
        if let Some(path) = value.strip_prefix("k8s-projected:") {
            return non_empty_path(path).map(Self::KubernetesProjected);
        }
        Err(TransportError::configuration(
            "material reference must use file: or k8s-projected:",
        ))
    }
}

/// Reference to private signing material.
#[derive(Clone, PartialEq, Eq)]
pub enum KeyMaterialRef {
    File(PathBuf),
    KubernetesProjected(PathBuf),
}

impl std::fmt::Debug for KeyMaterialRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(_) => formatter.write_str("KeyMaterialRef::File([REDACTED])"),
            Self::KubernetesProjected(_) => {
                formatter.write_str("KeyMaterialRef::KubernetesProjected([REDACTED])")
            }
        }
    }
}

impl FromStr for KeyMaterialRef {
    type Err = TransportError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some(path) = value.strip_prefix("file:") {
            return non_empty_path(path).map(Self::File);
        }
        if let Some(path) = value.strip_prefix("k8s-projected:") {
            return non_empty_path(path).map(Self::KubernetesProjected);
        }
        Err(TransportError::configuration(
            "key reference must use file: or k8s-projected:",
        ))
    }
}

fn non_empty_path(value: &str) -> Result<PathBuf, TransportError> {
    if value.is_empty() {
        Err(TransportError::configuration(
            "transport material path cannot be empty",
        ))
    } else {
        Ok(PathBuf::from(value))
    }
}

/// A certificate chain and private-key reference.
#[derive(Clone, PartialEq, Eq)]
pub struct IdentityRef {
    pub certificate_chain: MaterialRef,
    pub private_key: KeyMaterialRef,
}

impl std::fmt::Debug for IdentityRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IdentityRef")
            .field("certificate_chain", &"[REDACTED REFERENCE]")
            .field("private_key", &"[REDACTED REFERENCE]")
            .finish()
    }
}

/// Root and CRL references.
#[derive(Clone, PartialEq, Eq)]
pub struct TrustRef {
    pub roots: MaterialRef,
    pub crls: MaterialRef,
}

impl std::fmt::Debug for TrustRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrustRef")
            .field("roots", &"[REDACTED REFERENCE]")
            .field("crls", &"[REDACTED REFERENCE]")
            .finish()
    }
}

/// Loaded identity. Debug output deliberately contains no material.
pub struct IdentityMaterial {
    certificates: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
}

impl std::fmt::Debug for IdentityMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("IdentityMaterial")
            .field("certificates", &"[REDACTED]")
            .field("private_key", &"[REDACTED]")
            .finish()
    }
}

impl IdentityMaterial {
    fn into_certified_key(
        self,
        provider: &rustls::crypto::CryptoProvider,
    ) -> Result<CertifiedKey, TransportError> {
        CertifiedKey::from_der(self.certificates, self.private_key, provider)
            .map_err(|_| TransportError::material("certificate and private key do not match"))
    }
}

/// Loaded roots and CRLs.
pub struct TrustMaterial {
    roots: Arc<RootCertStore>,
    crls: Vec<CertificateRevocationListDer<'static>>,
}

impl std::fmt::Debug for TrustMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrustMaterial")
            .field("roots", &"[REDACTED]")
            .field("crls", &"[REDACTED]")
            .finish()
    }
}

impl TrustMaterial {
    pub fn roots(&self) -> Arc<RootCertStore> {
        Arc::clone(&self.roots)
    }

    pub fn crls(&self) -> &[CertificateRevocationListDer<'static>] {
        &self.crls
    }
}

/// Client-certificate policy for a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuth {
    Disabled,
    Optional,
    Required,
}

/// Loads a PEM certificate chain and private key.
pub fn load_identity(reference: &IdentityRef) -> Result<IdentityMaterial, TransportError> {
    let (certificate_bytes, key_bytes) =
        match (&reference.certificate_chain, &reference.private_key) {
            (
                MaterialRef::KubernetesProjected(certificate_path),
                KeyMaterialRef::KubernetesProjected(key_path),
            ) => {
                let mut materials = load_projected_generation(&[
                    (certificate_path, MAX_CERTIFICATE_FILE_BYTES),
                    (key_path, MAX_PRIVATE_KEY_FILE_BYTES),
                ])?
                .into_iter();
                let certificate_bytes = materials.next().ok_or_else(|| {
                    TransportError::material("projected identity has no certificate")
                })?;
                let key_bytes = materials.next().ok_or_else(|| {
                    TransportError::material("projected identity has no private key")
                })?;
                (certificate_bytes, key_bytes)
            }
            (certificate, KeyMaterialRef::File(path)) => (
                load_material(certificate, MAX_CERTIFICATE_FILE_BYTES)?,
                load_strict_file(path, MAX_PRIVATE_KEY_FILE_BYTES, true)?,
            ),
            (certificate, KeyMaterialRef::KubernetesProjected(path)) => (
                load_material(certificate, MAX_CERTIFICATE_FILE_BYTES)?,
                load_projected_file(path, MAX_PRIVATE_KEY_FILE_BYTES)?,
            ),
        };

    parse_identity(certificate_bytes, parse_private_key(key_bytes)?)
}

fn parse_identity(
    certificate_bytes: Vec<u8>,
    private_key: PrivateKeyDer<'static>,
) -> Result<IdentityMaterial, TransportError> {
    let certificates = CertificateDer::pem_slice_iter(&certificate_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TransportError::material("certificate chain is not valid PEM"))?;
    if certificates.is_empty() {
        return Err(TransportError::material(
            "certificate chain contains no certificate",
        ));
    }

    Ok(IdentityMaterial {
        certificates,
        private_key,
    })
}

fn parse_private_key(key_bytes: Vec<u8>) -> Result<PrivateKeyDer<'static>, TransportError> {
    let key_bytes = Zeroizing::new(key_bytes);
    PrivateKeyDer::from_pem_slice(&key_bytes)
        .map_err(|_| TransportError::material("private key is not valid PEM"))
}

/// Loads a root bundle and an optional CRL bundle.
pub fn load_trust(reference: &TrustRef) -> Result<TrustMaterial, TransportError> {
    let (root_bytes, crl_bytes) = match (&reference.roots, &reference.crls) {
        (
            MaterialRef::KubernetesProjected(root_path),
            MaterialRef::KubernetesProjected(crl_path),
        ) => {
            let mut materials = load_projected_generation(&[
                (root_path, MAX_TRUST_FILE_BYTES),
                (crl_path, MAX_TRUST_FILE_BYTES),
            ])?
            .into_iter();
            let roots = materials
                .next()
                .ok_or_else(|| TransportError::material("projected trust has no root bundle"))?;
            let crls = materials
                .next()
                .ok_or_else(|| TransportError::material("projected trust has no CRL bundle"))?;
            (roots, crls)
        }
        _ => (
            load_material(&reference.roots, MAX_TRUST_FILE_BYTES)?,
            load_material(&reference.crls, MAX_TRUST_FILE_BYTES)?,
        ),
    };
    parse_trust(root_bytes, crl_bytes)
}

fn parse_trust(root_bytes: Vec<u8>, crl_bytes: Vec<u8>) -> Result<TrustMaterial, TransportError> {
    let root_certificates = CertificateDer::pem_slice_iter(&root_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TransportError::material("trust bundle is not valid PEM"))?;
    if root_certificates.is_empty() {
        return Err(TransportError::material(
            "trust bundle contains no root certificate",
        ));
    }
    let mut roots = RootCertStore::empty();
    for certificate in root_certificates {
        roots
            .add(certificate)
            .map_err(|_| TransportError::material("trust bundle contains an invalid root"))?;
    }

    let crls = CertificateRevocationListDer::pem_slice_iter(&crl_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| TransportError::material("revocation bundle is not valid PEM"))?;
    if crls.is_empty() && !crl_bytes.is_empty() {
        return Err(TransportError::material(
            "revocation bundle contains no CRL",
        ));
    }
    Ok(TrustMaterial {
        roots: Arc::new(roots),
        crls,
    })
}

fn load_material(reference: &MaterialRef, limit: u64) -> Result<Vec<u8>, TransportError> {
    match reference {
        MaterialRef::File(path) => load_strict_file(path, limit, false),
        MaterialRef::KubernetesProjected(path) => load_projected_file(path, limit),
    }
}

fn load_strict_file(
    path: &Path,
    limit: u64,
    require_owner_only: bool,
) -> Result<Vec<u8>, TransportError> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    {
        if std::fs::symlink_metadata(path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            return Err(TransportError::material(
                "transport material cannot be a symbolic link",
            ));
        }
    }
    let file = options
        .open(path)
        .map_err(|_| TransportError::material("transport material cannot be opened"))?;
    read_validated_file(file, limit, require_owner_only)
}

fn load_projected_file(path: &Path, limit: u64) -> Result<Vec<u8>, TransportError> {
    load_projected_generation(&[(path, limit)]).and_then(|mut values| {
        values.pop().ok_or_else(|| {
            TransportError::material("projected material generation contains no value")
        })
    })
}

fn load_projected_generation(inputs: &[(&Path, u64)]) -> Result<Vec<Vec<u8>>, TransportError> {
    let mut generation_directory: Option<PathBuf> = None;
    let mut values = Vec::with_capacity(inputs.len());
    for (path, limit) in inputs {
        let root = projected_volume_root(path)?;
        let canonical_root = root
            .canonicalize()
            .map_err(|_| TransportError::material("projected volume root cannot be resolved"))?;
        let canonical_path = path
            .canonicalize()
            .map_err(|_| TransportError::material("projected material cannot be resolved"))?;
        if !canonical_path.starts_with(&canonical_root) {
            return Err(TransportError::material(
                "projected material resolves outside its volume root",
            ));
        }
        let parent = canonical_path.parent().ok_or_else(|| {
            TransportError::material("projected material has no generation directory")
        })?;
        if generation_directory
            .as_deref()
            .is_some_and(|generation| generation != parent)
        {
            return Err(TransportError::generation(
                "projected material resolves to a mixed generation",
            ));
        }
        generation_directory = Some(parent.to_path_buf());
        let file = File::open(&canonical_path)
            .map_err(|_| TransportError::material("projected material cannot be opened"))?;
        values.push(read_validated_file(file, *limit, false)?);
    }
    Ok(values)
}

fn projected_volume_root(path: &Path) -> Result<PathBuf, TransportError> {
    path.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| TransportError::material("projected material has no volume root"))
}

fn read_validated_file(
    mut file: File,
    limit: u64,
    require_owner_only: bool,
) -> Result<Vec<u8>, TransportError> {
    let metadata = file
        .metadata()
        .map_err(|_| TransportError::material("transport material metadata cannot be read"))?;
    if !metadata.is_file() {
        return Err(TransportError::material(
            "transport material must be a regular file",
        ));
    }
    if metadata.len() == 0 || metadata.len() > limit {
        return Err(TransportError::material(
            "transport material has an invalid size",
        ));
    }
    #[cfg(unix)]
    if require_owner_only {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(TransportError::material(
                "transport material permissions must be 0600 or stricter",
            ));
        }
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| TransportError::material("transport material is too large"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.read_to_end(&mut bytes)
        .map_err(|_| TransportError::material("transport material cannot be read"))?;
    Ok(bytes)
}

/// Builds a TLS 1.3 server config with the requested client-auth policy.
pub fn build_server_config(
    identity: IdentityMaterial,
    client_trust: Option<&TrustMaterial>,
    client_auth: ClientAuth,
    alpn_protocols: &[&[u8]],
) -> Result<ServerConfig, TransportError> {
    install_default_crypto_provider()?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let certified_key = identity.into_certified_key(&provider)?;
    let builder = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| TransportError::configuration("TLS 1.3 provider is unavailable"))?;
    let resolver = Arc::new(SingleCertAndKey::from(certified_key));

    let mut config = match client_auth {
        ClientAuth::Disabled => builder.with_no_client_auth().with_cert_resolver(resolver),
        ClientAuth::Optional | ClientAuth::Required => {
            let trust = client_trust.ok_or_else(|| {
                TransportError::configuration("client authentication requires a trust bundle")
            })?;
            let verifier = WebPkiClientVerifier::builder(trust.roots())
                .with_crls(trust.crls().iter().cloned());
            let verifier: Arc<dyn rustls::server::danger::ClientCertVerifier> =
                if client_auth == ClientAuth::Optional {
                    verifier
                        .allow_unauthenticated()
                        .build()
                        .map_err(map_verifier_error)?
                } else {
                    verifier.build().map_err(map_verifier_error)?
                };
            builder
                .with_client_cert_verifier(verifier)
                .with_cert_resolver(resolver)
        }
    };
    config.alpn_protocols = alpn_protocols.iter().map(|value| value.to_vec()).collect();
    Ok(config)
}

/// Builds a TLS 1.3 client config with an optional client identity.
pub fn build_client_config(
    trust: &TrustMaterial,
    identity: Option<IdentityMaterial>,
    alpn_protocols: &[&[u8]],
) -> Result<ClientConfig, TransportError> {
    install_default_crypto_provider()?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = WebPkiServerVerifier::builder_with_provider(trust.roots(), provider.clone())
        .with_crls(trust.crls().iter().cloned())
        .build()
        .map_err(map_verifier_error)?;
    let builder = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| TransportError::configuration("TLS 1.3 provider is unavailable"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier);
    let mut config = if let Some(identity) = identity {
        let certified_key = identity.into_certified_key(builder.crypto_provider())?;
        builder.with_client_cert_resolver(Arc::new(SingleCertAndKey::from(certified_key)))
    } else {
        builder.with_no_client_auth()
    };
    config.alpn_protocols = alpn_protocols.iter().map(|value| value.to_vec()).collect();
    Ok(config)
}

/// Builds one client TLS config from environment references.
///
/// `ZETA4J_CLIENT_SERVER_ROOTS_REF` is required; `ZETA4J_CLIENT_SERVER_CRLS_REF` is
/// optional and `ZETA4J_CLIENT_SERVER_NAME` defaults to `default_server_name`.
///
/// # Errors
///
/// Fails when the root reference is missing, malformed or cannot be loaded.
pub fn client_tls_config_from_environment(
    alpn_protocols: &[&[u8]],
    default_server_name: &str,
) -> Result<(ClientConfig, String), TransportError> {
    let roots = std::env::var("ZETA4J_CLIENT_SERVER_ROOTS_REF").map_err(|_| {
        TransportError::configuration("ZETA4J_CLIENT_SERVER_ROOTS_REF is required for TLS")
    })?;
    let root_bytes = load_material(&MaterialRef::from_str(&roots)?, MAX_TRUST_FILE_BYTES)?;
    let crl_bytes = match std::env::var("ZETA4J_CLIENT_SERVER_CRLS_REF") {
        Ok(crls) if !crls.is_empty() => {
            load_material(&MaterialRef::from_str(&crls)?, MAX_TRUST_FILE_BYTES)?
        }
        _ => Vec::new(),
    };
    let server_name = std::env::var("ZETA4J_CLIENT_SERVER_NAME")
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| default_server_name.to_owned());
    let _ = validated_server_name(&server_name)?;
    let config = build_client_config(&parse_trust(root_bytes, crl_bytes)?, None, alpn_protocols)?;
    Ok((config, server_name))
}

/// Installs the canonical process-wide rustls provider before any library can
/// ask rustls to infer one from the dependency feature graph.
pub fn install_default_crypto_provider() -> Result<(), TransportError> {
    if rustls::crypto::CryptoProvider::get_default().is_some() {
        return Ok(());
    }
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
        && rustls::crypto::CryptoProvider::get_default().is_none()
    {
        return Err(TransportError::configuration(
            "TLS crypto provider could not be installed",
        ));
    }
    Ok(())
}

fn map_verifier_error(error: rustls::server::VerifierBuilderError) -> TransportError {
    let message = match error {
        rustls::server::VerifierBuilderError::NoRootAnchors => {
            "trust bundle contains no usable root"
        }
        rustls::server::VerifierBuilderError::InvalidCrl(_) => {
            return TransportError::revoked("revocation bundle contains an invalid CRL");
        }
        _ => "certificate verifier configuration is invalid",
    };
    TransportError::material(message)
}

/// Ensures that a server name can be represented without DNS/IP fallback.
pub fn validated_server_name(name: &str) -> Result<ServerName<'static>, TransportError> {
    ServerName::try_from(name.to_string())
        .map_err(|_| TransportError::identity("TLS server name is invalid"))
}

/// Opens a Tonic channel through the canonical rustls client verifier.
///
/// The caller supplies an explicit server name; address-to-SNI inference and
/// plaintext fallback are deliberately unavailable.
pub async fn connect_tonic(
    address: &str,
    server_name: ServerName<'static>,
    config: Arc<ClientConfig>,
    connect_timeout: Duration,
    request_timeout: Duration,
) -> Result<tonic::transport::Channel, tonic::transport::Error> {
    // The custom connector below returns an already-authenticated TLS stream.
    // Keep the Endpoint URI plaintext so Tonic does not try to add a second
    // TLS layer around that stream.
    let endpoint = tonic::transport::Endpoint::from_shared(format!("http://{address}"))?
        .connect_timeout(connect_timeout)
        .timeout(request_timeout);
    let connector = tower::service_fn(move |uri: http::Uri| {
        let tls = tokio_rustls::TlsConnector::from(config.clone());
        let server_name = server_name.clone();
        async move {
            let host = uri.host().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "TLS endpoint has no host")
            })?;
            let port = uri.port_u16().unwrap_or(443);
            let tcp = tokio::net::TcpStream::connect((host, port)).await?;
            tcp.set_nodelay(true)?;
            tls.connect(server_name, tcp)
                .await
                .map(hyper_util::rt::TokioIo::new)
                .map_err(std::io::Error::other)
        }
    });
    endpoint.connect_with_connector(connector).await
}

/// Tonic channel factory bound to one transport mode.
///
/// TLS connectors always verify the server through the canonical rustls
/// configuration; plaintext connectors reach any host the caller names.
#[derive(Clone)]
pub struct TonicConnector {
    tls: Option<Arc<ClientConfig>>,
    channels: Arc<tokio::sync::RwLock<HashMap<(String, String), tonic::transport::Channel>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum TonicConnectorError {
    #[error(transparent)]
    Transport(#[from] tonic::transport::Error),
    #[error(transparent)]
    Policy(#[from] TransportError),
}

impl std::fmt::Debug for TonicConnector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TonicConnector")
            .field("mode", &self.mode())
            .finish()
    }
}

impl TonicConnector {
    pub fn tls(config: Arc<ClientConfig>) -> Self {
        Self {
            tls: Some(config),
            channels: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn plaintext() -> Self {
        Self {
            tls: None,
            channels: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub const fn mode(&self) -> TransportMode {
        if self.tls.is_some() {
            TransportMode::Tls
        } else {
            TransportMode::Plaintext
        }
    }

    pub async fn connect(
        &self,
        address: &str,
        server_name: &str,
        connect_timeout: Duration,
        request_timeout: Duration,
    ) -> Result<tonic::transport::Channel, TonicConnectorError> {
        let Some(config) = &self.tls else {
            return tonic::transport::Endpoint::from_shared(format!("http://{address}"))?
                .connect_timeout(connect_timeout)
                .timeout(request_timeout)
                .connect()
                .await
                .map_err(Into::into);
        };
        let key = (address.to_string(), server_name.to_string());
        if let Some(channel) = self.channels.read().await.get(&key) {
            return Ok(channel.clone());
        }
        let channel = connect_tonic(
            address,
            validated_server_name(server_name)?,
            Arc::clone(config),
            connect_timeout,
            request_timeout,
        )
        .await?;
        self.channels.write().await.insert(key, channel.clone());
        Ok(channel)
    }
}

/// Stable, redacted transport failure.
#[derive(Debug, thiserror::Error)]
#[error("[{code}] {message}")]
pub struct TransportError {
    code: String,
    message: &'static str,
}

impl TransportError {
    fn new(code: zeta4j_common::ErrorCode, message: &'static str) -> Self {
        Self {
            code: code.full_code(),
            message,
        }
    }

    pub fn configuration(message: &'static str) -> Self {
        Self::new(codes::server::TLS_CONFIGURATION_INVALID, message)
    }

    pub fn material(message: &'static str) -> Self {
        Self::new(codes::server::TLS_MATERIAL_INVALID, message)
    }

    pub fn identity(message: &'static str) -> Self {
        Self::new(codes::server::TLS_IDENTITY_MISMATCH, message)
    }

    pub fn revoked(message: &'static str) -> Self {
        Self::new(codes::server::TLS_IDENTITY_REVOKED, message)
    }

    pub fn generation(message: &'static str) -> Self {
        Self::new(codes::server::TLS_GENERATION_REJECTED, message)
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub const fn public_message(&self) -> &'static str {
        self.message
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use rcgen::{
        BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa,
        KeyPair, KeyUsagePurpose,
    };
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn material_debug_is_redacted() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture_identity()?;
        let identity = load_identity(&fixture.reference)?;
        let debug = format!("{identity:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("PRIVATE KEY"));
        let reference_canary = "/private/transport/reference-canary";
        let identity_ref = IdentityRef {
            certificate_chain: MaterialRef::File(reference_canary.into()),
            private_key: KeyMaterialRef::File(reference_canary.into()),
        };
        let trust_ref = TrustRef {
            roots: MaterialRef::File(reference_canary.into()),
            crls: MaterialRef::KubernetesProjected(reference_canary.into()),
        };
        assert!(!format!("{identity_ref:?}").contains(reference_canary));
        assert!(!format!("{trust_ref:?}").contains(reference_canary));
        Ok(())
    }

    #[test]
    fn strict_file_loader_rejects_group_readable_key() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture_identity()?;
        fs::set_permissions(&fixture.key_path, fs::Permissions::from_mode(0o640))?;
        let error = load_identity(&fixture.reference).err();
        assert!(error.is_some_and(|value| value.code() == "SV-TLS-002"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn projected_loader_rejects_mixed_atomic_writer_generations()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let first = directory.path().join("..generation-1");
        let second = directory.path().join("..generation-2");
        fs::create_dir(&first)?;
        fs::create_dir(&second)?;
        fs::write(first.join("cert"), b"first")?;
        fs::write(second.join("key"), b"second")?;
        symlink(first.join("cert"), directory.path().join("cert"))?;
        symlink(second.join("key"), directory.path().join("key"))?;

        let error = load_projected_generation(&[
            (&directory.path().join("cert"), 32),
            (&directory.path().join("key"), 32),
        ])
        .err()
        .ok_or("mixed projected generations were accepted")?;
        assert_eq!(
            error.code(),
            codes::server::TLS_GENERATION_REJECTED.full_code()
        );
        Ok(())
    }

    #[test]
    fn server_config_accepts_matching_identity() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture_identity()?;
        let identity = load_identity(&fixture.reference)?;
        assert!(build_server_config(identity, None, ClientAuth::Disabled, &[b"h2"]).is_ok());
        Ok(())
    }

    #[test]
    fn server_config_rejects_mismatched_key() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture_identity()?;
        let other = fixture_identity()?;
        fs::copy(other.key_path, &fixture.key_path)?;
        let identity = load_identity(&fixture.reference)?;
        let error = build_server_config(identity, None, ClientAuth::Disabled, &[b"h2"]).err();
        assert!(error.is_some_and(|value| value.code() == "SV-TLS-002"));
        Ok(())
    }

    #[tokio::test]
    async fn plaintext_connector_reaches_any_host() -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let mut server = tonic::transport::Server::builder();
        let server = tokio::spawn(
            server
                .add_routes(tonic::service::Routes::default())
                .serve_with_incoming(tonic::transport::server::TcpIncoming::from(listener)),
        );
        let connector = TonicConnector::plaintext();
        let reached = connector
            .connect(
                &format!("localhost:{port}"),
                "localhost",
                Duration::from_secs(5),
                Duration::from_secs(5),
            )
            .await;
        server.abort();
        reached?;

        // A routable address is dialled rather than refused by policy.
        let error = connector
            .connect(
                "192.0.2.10:9046",
                "192.0.2.10",
                Duration::from_millis(50),
                Duration::from_millis(50),
            )
            .await
            .err();
        assert!(matches!(error, Some(TonicConnectorError::Transport(_))));
        Ok(())
    }

    struct IdentityFixture {
        _directory: TempDir,
        key_path: PathBuf,
        reference: IdentityRef,
    }

    fn fixture_identity() -> Result<IdentityFixture, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let key_pair = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec!["server.example".to_string()])?;
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let issuer_key = KeyPair::generate()?;
        let mut issuer_params = CertificateParams::new(Vec::<String>::new())?;
        issuer_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        issuer_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let issuer = CertifiedIssuer::self_signed(issuer_params, issuer_key)?;
        let certificate = params.signed_by(&key_pair, &issuer)?;
        let cert_path = directory.path().join("tls.crt");
        let key_path = directory.path().join("tls.key");
        fs::write(&cert_path, certificate.pem())?;
        fs::write(&key_path, key_pair.serialize_pem())?;
        fs::set_permissions(&cert_path, fs::Permissions::from_mode(0o600))?;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;
        Ok(IdentityFixture {
            reference: IdentityRef {
                certificate_chain: MaterialRef::File(cert_path),
                private_key: KeyMaterialRef::File(key_path.clone()),
            },
            _directory: directory,
            key_path,
        })
    }
}
