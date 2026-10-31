// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Owner-only filesystem helpers for client-managed configuration and logs.

use std::io::Write;
use std::path::Path;

/// Creates `path` (and parents) and restricts it to the owner.
pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Creates `path` if missing and restricts the file to the owner.
pub fn prepare_private_file(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Atomically replaces `path` with `content` readable only by the owner.
pub fn write_private_file(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "private file path requires a parent directory",
        )
    })?;
    ensure_private_dir(parent)?;
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "private file path requires a file name",
        )
    })?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = options.open(&temporary).and_then(|mut file| {
        file.write_all(content)?;
        file.sync_all()
    });
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, path)
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn private_file_and_directory_are_owner_only() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("nested").join("config.toml");
        write_private_file(&path, b"value")?;
        assert_eq!(std::fs::read(&path)?, b"value");
        assert_eq!(
            std::fs::metadata(&path)?.permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(root.path().join("nested"))?
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        Ok(())
    }
}
