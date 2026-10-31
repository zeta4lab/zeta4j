// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Owner-only client credential envelope shared by `z4j` commands.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

const ENVELOPE_VERSION: u32 = 1;
const MAXIMUM_ENVELOPE_BYTES: u64 = 64 * 1024;
static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialEnvelope {
    pub version: u32,
    pub audience: String,
    pub auth_endpoint: String,
    access_token: String,
    refresh_token: String,
    pub access_expires_at: i64,
    pub refresh_expires_at: i64,
    pub session_id: String,
}

impl CredentialEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        audience: impl Into<String>,
        auth_endpoint: impl Into<String>,
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        access_expires_at: i64,
        refresh_expires_at: i64,
        session_id: impl Into<String>,
    ) -> Result<Self, CredentialEnvelopeError> {
        let envelope = Self {
            version: ENVELOPE_VERSION,
            audience: audience.into(),
            auth_endpoint: auth_endpoint.into(),
            access_token: access_token.into(),
            refresh_token: refresh_token.into(),
            access_expires_at,
            refresh_expires_at,
            session_id: session_id.into(),
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    pub fn refresh_token(&self) -> &str {
        &self.refresh_token
    }

    pub fn validate(&self) -> Result<(), CredentialEnvelopeError> {
        if self.version != ENVELOPE_VERSION
            || !matches!(self.audience.as_str(), "grpc" | "bolt")
            || !(self.auth_endpoint.starts_with("http://")
                || self.auth_endpoint.starts_with("https://"))
            || self.access_token.is_empty()
            || self.refresh_token.is_empty()
            || self.session_id.is_empty()
            || self.access_expires_at <= 0
            || self.refresh_expires_at <= self.access_expires_at
        {
            return Err(CredentialEnvelopeError::Invalid);
        }
        Ok(())
    }
}

impl std::fmt::Debug for CredentialEnvelope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialEnvelope")
            .field("version", &self.version)
            .field("audience", &self.audience)
            .field("auth_endpoint", &self.auth_endpoint)
            .field("access_token", &"[REDACTED]")
            .field("refresh_token", &"[REDACTED]")
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .field("session_id", &self.session_id)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialEnvelopeError {
    #[error("credential envelope is unavailable")]
    Unavailable,
    #[error("credential envelope is invalid")]
    Invalid,
    #[error("credential envelope permissions are insecure")]
    InsecurePermissions,
    #[error("credential envelope operation failed")]
    Io,
    #[error("credential refresh service is unavailable")]
    RefreshUnavailable,
}

pub fn default_credential_path() -> Result<PathBuf, CredentialEnvelopeError> {
    if let Some(path) = std::env::var_os("ZETA4J_CREDENTIAL_FILE") {
        return Ok(PathBuf::from(path));
    }
    if let Some(home) = std::env::var_os("ZETA4J_HOME") {
        return Ok(PathBuf::from(home).join("credentials").join("grpc.json"));
    }
    dirs::home_dir()
        .map(|home| home.join(".zeta4j").join("credentials").join("grpc.json"))
        .ok_or(CredentialEnvelopeError::Unavailable)
}

pub fn load_credential_envelope(
    path: &Path,
) -> Result<CredentialEnvelope, CredentialEnvelopeError> {
    let metadata = std::fs::metadata(path).map_err(|_| CredentialEnvelopeError::Unavailable)?;
    if !metadata.is_file() || metadata.len() > MAXIMUM_ENVELOPE_BYTES {
        return Err(CredentialEnvelopeError::Invalid);
    }
    ensure_owner_only(&metadata)?;
    let mut file = File::open(path).map_err(|_| CredentialEnvelopeError::Io)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|_| CredentialEnvelopeError::Io)?;
    let envelope: CredentialEnvelope =
        serde_json::from_slice(&bytes).map_err(|_| CredentialEnvelopeError::Invalid)?;
    envelope.validate()?;
    Ok(envelope)
}

pub fn store_credential_envelope(
    path: &Path,
    envelope: &CredentialEnvelope,
) -> Result<(), CredentialEnvelopeError> {
    envelope.validate()?;
    let parent = path.parent().ok_or(CredentialEnvelopeError::Invalid)?;
    std::fs::create_dir_all(parent).map_err(|_| CredentialEnvelopeError::Io)?;
    set_owner_only_directory(parent)?;
    let lock_path = parent.join(".credential.lock");
    let lock = owner_only_file(&lock_path)?;
    lock.lock().map_err(|_| CredentialEnvelopeError::Io)?;
    let result = store_locked(path, envelope);
    let unlock_result = lock.unlock().map_err(|_| CredentialEnvelopeError::Io);
    result.and(unlock_result)
}

pub fn load_or_refresh_credential_envelope(
    path: &Path,
    now: i64,
) -> Result<CredentialEnvelope, CredentialEnvelopeError> {
    let envelope = load_credential_envelope(path)?;
    if envelope.access_expires_at > now.saturating_add(30) {
        return Ok(envelope);
    }
    let parent = path.parent().ok_or(CredentialEnvelopeError::Invalid)?;
    let lock = owner_only_file(&parent.join(".credential.lock"))?;
    lock.lock().map_err(|_| CredentialEnvelopeError::Io)?;
    let result = refresh_locked(path, now, false);
    let unlock_result = lock.unlock().map_err(|_| CredentialEnvelopeError::Io);
    finish_unlock(result, unlock_result)
}

pub fn refresh_credential_envelope(
    path: &Path,
    now: i64,
) -> Result<CredentialEnvelope, CredentialEnvelopeError> {
    let parent = path.parent().ok_or(CredentialEnvelopeError::Invalid)?;
    let lock = owner_only_file(&parent.join(".credential.lock"))?;
    lock.lock().map_err(|_| CredentialEnvelopeError::Io)?;
    let result = refresh_locked(path, now, true);
    let unlock_result = lock.unlock().map_err(|_| CredentialEnvelopeError::Io);
    finish_unlock(result, unlock_result)
}

fn finish_unlock<T>(
    result: Result<T, CredentialEnvelopeError>,
    unlock: Result<(), CredentialEnvelopeError>,
) -> Result<T, CredentialEnvelopeError> {
    match (result, unlock) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

fn refresh_locked(
    path: &Path,
    now: i64,
    force: bool,
) -> Result<CredentialEnvelope, CredentialEnvelopeError> {
    let envelope = load_credential_envelope(path)?;
    if !force && envelope.access_expires_at > now.saturating_add(30) {
        return Ok(envelope);
    }
    if envelope.refresh_expires_at <= now {
        return Err(CredentialEnvelopeError::Invalid);
    }
    let response = reqwest::blocking::Client::new()
        .post(format!(
            "{}/auth/refresh",
            envelope.auth_endpoint.trim_end_matches('/')
        ))
        .json(&serde_json::json!({
            "refresh_token": envelope.refresh_token(),
            "audience": &envelope.audience
        }))
        .send()
        .map_err(|_| CredentialEnvelopeError::RefreshUnavailable)?;
    if !response.status().is_success() {
        return Err(CredentialEnvelopeError::Invalid);
    }
    #[derive(Deserialize)]
    struct RefreshResponse {
        access_token: String,
        refresh_token: String,
        access_expires_at: i64,
        refresh_expires_at: i64,
        session_id: String,
    }
    let response = response
        .json::<RefreshResponse>()
        .map_err(|_| CredentialEnvelopeError::Invalid)?;
    let refreshed = CredentialEnvelope::new(
        &envelope.audience,
        &envelope.auth_endpoint,
        response.access_token,
        response.refresh_token,
        response.access_expires_at,
        response.refresh_expires_at,
        response.session_id,
    )?;
    store_locked(path, &refreshed)?;
    Ok(refreshed)
}

fn store_locked(path: &Path, envelope: &CredentialEnvelope) -> Result<(), CredentialEnvelopeError> {
    let parent = path.parent().ok_or(CredentialEnvelopeError::Invalid)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(CredentialEnvelopeError::Invalid)?;
    let bytes = serde_json::to_vec(envelope).map_err(|_| CredentialEnvelopeError::Invalid)?;
    if bytes.len() as u64 > MAXIMUM_ENVELOPE_BYTES {
        return Err(CredentialEnvelopeError::Invalid);
    }
    let (temporary, mut file) = owner_only_temporary_file(parent, file_name)?;
    let result = file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| std::fs::rename(&temporary, path))
        .map_err(|_| CredentialEnvelopeError::Io);
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| CredentialEnvelopeError::Io)
}

fn owner_only_temporary_file(
    parent: &Path,
    file_name: &str,
) -> Result<(PathBuf, File), CredentialEnvelopeError> {
    for _ in 0..32 {
        let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let mut options = OpenOptions::new();
        options.create_new(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(CredentialEnvelopeError::Io),
        }
    }
    Err(CredentialEnvelopeError::Io)
}

fn owner_only_file(path: &Path) -> Result<File, CredentialEnvelopeError> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|_| CredentialEnvelopeError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|_| CredentialEnvelopeError::Io)?;
    }
    Ok(file)
}

fn ensure_owner_only(metadata: &std::fs::Metadata) -> Result<(), CredentialEnvelopeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(CredentialEnvelopeError::InsecurePermissions);
        }
    }
    Ok(())
}

fn set_owner_only_directory(path: &Path) -> Result<(), CredentialEnvelopeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| CredentialEnvelopeError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trip_is_owner_only_and_redacted() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("credentials").join("grpc.json");
        let envelope = CredentialEnvelope::new(
            "grpc",
            "http://127.0.0.1:9044",
            "access-canary",
            "refresh-canary",
            2_000,
            3_000,
            "sess_00000000000000000000000000000000",
        )?;
        store_credential_envelope(&path, &envelope)?;
        let loaded = load_credential_envelope(&path)?;
        assert_eq!(loaded.access_token(), "access-canary");
        assert!(!format!("{loaded:?}").contains("access-canary"));
        assert!(!format!("{loaded:?}").contains("refresh-canary"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path)?.permissions().mode() & 0o777,
                0o600
            );
        }
        Ok(())
    }

    #[test]
    fn temporary_envelope_never_reuses_a_stale_file() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let sequence = TEMPORARY_FILE_SEQUENCE.load(Ordering::Relaxed);
        let stale = directory.path().join(format!(
            ".grpc.json.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        std::fs::write(&stale, b"stale-secret")?;

        let (temporary, _) = owner_only_temporary_file(directory.path(), "grpc.json")?;
        assert_ne!(temporary, stale);
        assert_eq!(std::fs::read(&stale)?, b"stale-secret");
        std::fs::remove_file(temporary)?;
        Ok(())
    }
}
