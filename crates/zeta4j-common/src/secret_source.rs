// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Strict environment-or-file secret loading shared by deployment surfaces.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;

const MAX_SECRET_FILE_BYTES: u64 = 16 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum SecretSourceError {
    #[error("{value_env} and {file_env} cannot be configured together")]
    Conflicting {
        value_env: &'static str,
        file_env: &'static str,
    },
    #[error("{file_env} secret file is invalid: {message}")]
    InvalidFile {
        file_env: &'static str,
        message: String,
    },
}

/// Resolve one secret without including its value in errors or debug output.
pub fn load_secret_source(
    value_env: &'static str,
    file_env: &'static str,
) -> Result<Option<String>, SecretSourceError> {
    let direct = std::env::var(value_env)
        .ok()
        .filter(|value| !value.is_empty());
    let file = std::env::var_os(file_env);
    match (direct, file) {
        (Some(_), Some(_)) => Err(SecretSourceError::Conflicting {
            value_env,
            file_env,
        }),
        (Some(value), None) => Ok(Some(value)),
        (None, Some(path)) => load_secret_file(Path::new(&path), file_env).map(Some),
        (None, None) => Ok(None),
    }
}

pub fn load_secret_file(path: &Path, file_env: &'static str) -> Result<String, SecretSourceError> {
    let invalid = |message: String| SecretSourceError::InvalidFile { file_env, message };
    let mut file = open_secret_file(path).map_err(|error| invalid(error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| invalid(error.to_string()))?;
    if !metadata.is_file() {
        return Err(invalid(
            "source must be a regular non-symlink file".to_string(),
        ));
    }
    if metadata.len() == 0 || metadata.len() > MAX_SECRET_FILE_BYTES {
        return Err(invalid("source has an invalid size".to_string()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(invalid("permissions must be 0600 or stricter".to_string()));
        }
    }
    let mut value = String::new();
    file.read_to_string(&mut value)
        .map_err(|error| invalid(error.to_string()))?;
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    if value.is_empty() {
        return Err(invalid("source must not be empty".to_string()));
    }
    Ok(value)
}

fn open_secret_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    {
        if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "symbolic links are not allowed",
            ));
        }
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn rejects_group_or_world_readable_files() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let path = temp.path().join("secret");
        fs::write(&path, "canary-secret\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
        assert!(load_secret_file(&path, "TEST_SECRET_FILE").is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        assert_eq!(
            load_secret_file(&path, "TEST_SECRET_FILE")?,
            "canary-secret"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_links() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temp = tempfile::tempdir()?;
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::write(&target, "canary-secret")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
        symlink(&target, &link)?;

        assert!(load_secret_file(&link, "TEST_SECRET_FILE").is_err());
        Ok(())
    }
}
