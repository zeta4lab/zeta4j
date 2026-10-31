// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Server directory initialization utilities.

use log::info;
use std::path::Path;
use zeta4j_common::{DEFAULT_CORE_TOML, DEFAULT_SERVER_TOML, prepare_runtime_state};
use zeta4j_graph::config::Edition;

// Re-export for external use
pub use zeta4j_graph::config::Edition as DirectoryEdition;

use super::ValidatedStartupConfig;
use zeta4j_common::private_fs::{ensure_private_dir, write_private_file};

/// Makes every subsequent runtime-created file owner-only by default.
#[allow(unsafe_code)]
pub fn enforce_owner_only_process_umask() {
    #[cfg(unix)]
    // SAFETY: umask changes only this process's file-creation mask and accepts
    // every mode_t value. Startup calls this before worker threads are created.
    unsafe {
        libc::umask(0o077);
    }
}

/// Directory initialization helper
pub struct DirectoryInit<'a> {
    config: &'a ValidatedStartupConfig,
}

impl<'a> DirectoryInit<'a> {
    /// Create a new DirectoryInit
    pub fn new(config: &'a ValidatedStartupConfig) -> Self {
        Self { config }
    }

    /// Create all required directories
    pub fn create_all(&self) -> std::io::Result<()> {
        self.prepare_state()?;
        self.create_log_dir()?;
        self.create_config_dir()?;
        self.create_namespace_dirs()?;
        Ok(())
    }

    /// Validate or initialize the state format before any other startup side effect.
    pub fn prepare_state(&self) -> std::io::Result<()> {
        prepare_runtime_state(&self.config.data_dir, &self.config.config_dir)
            .map(|_| ())
            .map_err(|error| std::io::Error::other(error.to_string()))
    }

    /// Create the logical database directory root.
    fn create_namespace_dirs(&self) -> std::io::Result<()> {
        let data_dir = &self.config.data_dir;

        // Each logical database owns graph, corpus, Fulltext, and Vector state
        // through one DatabaseContext under graphs/{database}/database.redb.
        create_dir_if_not_exists(&data_dir.join("graphs"), "graphs")?;

        Ok(())
    }

    /// Ensure the logical database namespace exists (`graphs/{name}`).
    ///
    /// Called when `--index {name}` is used to create all three directories at once.
    pub fn ensure_index_namespace(
        data_dir: &Path,
        index_name: &str,
        _edition: Edition,
    ) -> std::io::Result<()> {
        // graphs/{index_name}/
        create_dir_if_not_exists(
            &data_dir.join("graphs").join(index_name),
            &format!("graphs/{}", index_name),
        )?;

        Ok(())
    }

    /// Create log directory if it doesn't exist
    fn create_log_dir(&self) -> std::io::Result<()> {
        create_dir_if_not_exists(&self.config.log_dir, "log")
    }

    /// Create config directory and default config files
    fn create_config_dir(&self) -> std::io::Result<()> {
        create_dir_if_not_exists(&self.config.config_dir, "config")?;
        self.create_default_configs(&self.config.config_dir)?;
        Ok(())
    }

    /// Create default configuration files
    fn create_default_configs(&self, config_dir: &Path) -> std::io::Result<()> {
        create_config_if_not_exists(config_dir, "core.toml", DEFAULT_CORE_TOML)?;
        create_config_if_not_exists(config_dir, "server.toml", DEFAULT_SERVER_TOML)?;

        Ok(())
    }
}

/// Create a directory if it doesn't exist
fn create_dir_if_not_exists(path: &Path, name: &str) -> std::io::Result<()> {
    let created = !path.exists();
    ensure_private_dir(path)?;
    if created {
        info!("Created {} directory: {}", name, path.display());
    }
    Ok(())
}

/// Create a config file if it doesn't exist
fn create_config_if_not_exists(
    config_dir: &Path,
    filename: &str,
    content: &str,
) -> std::io::Result<()> {
    let path = config_dir.join(filename);
    if !path.exists() {
        write_private_file(&path, content.as_bytes())?;
        info!("Created default config: {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use zeta4j_common::STATE_MANIFEST_FILE;

    use super::*;
    use crate::{EffectiveConfig, RuntimeProfile, validate_startup_security};

    fn validated(
        config: EffectiveConfig,
    ) -> Result<ValidatedStartupConfig, Box<dyn std::error::Error>> {
        Ok(validate_startup_security(EffectiveConfig {
            runtime_profile: RuntimeProfile::LocalTest,
            transport_mode: "plaintext".to_string(),
            auth_enabled: false,
            ..config
        })?)
    }

    #[test]
    fn uses_the_effective_config_directory_for_state_ownership_and_generation()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let config = validated(EffectiveConfig {
            data_dir: root.path().join("data"),
            config_dir: root.path().join("selected-config"),
            log_dir: root.path().join("logs"),
            ..EffectiveConfig::default()
        })?;
        std::fs::create_dir_all(&config.config_dir)?;
        std::fs::write(
            config.config_dir.join("core.toml"),
            b"unidentified = true\n",
        )?;

        assert!(DirectoryInit::new(&config).prepare_state().is_err());
        assert!(!config.data_dir.join(STATE_MANIFEST_FILE).exists());
        assert!(!config.log_dir.exists());

        std::fs::remove_file(config.config_dir.join("core.toml"))?;
        DirectoryInit::new(&config).create_all()?;
        assert!(config.data_dir.join(STATE_MANIFEST_FILE).is_file());
        assert!(config.config_dir.join("core.toml").is_file());
        assert!(config.config_dir.join("server.toml").is_file());

        let other_config = root.path().join("other-config");
        let config = validated(EffectiveConfig {
            data_dir: config.data_dir.clone(),
            config_dir: other_config.clone(),
            log_dir: config.log_dir.clone(),
            ..EffectiveConfig::default()
        })?;
        assert!(DirectoryInit::new(&config).create_all().is_ok());
        assert!(other_config.join("core.toml").is_file());
        Ok(())
    }
}
