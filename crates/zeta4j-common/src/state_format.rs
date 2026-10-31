// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Runtime state format manifest and fail-closed startup validation.
//!
//! Package versions are deliberately not used for persistence compatibility.
//! Only the format generations in this module determine whether a data
//! directory can be opened by the current binary.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Manifest stored at the root of every initialized data directory.
pub const STATE_MANIFEST_FILE: &str = ".zeta4j-state.json";
/// Current canonical persistence format generation.
pub const CURRENT_STORAGE_FORMAT: u32 = 3;
/// Current generated configuration format generation.
pub const CURRENT_CONFIG_FORMAT: u32 = 1;
/// Configuration files generated and owned by `z4j server init`.
pub const GENERATED_CONFIG_FILES: &[&str] = &["core.toml", "server.toml"];

const PRODUCT: &str = "zeta4j";
const TEMP_PREFIX: &str = "..zeta4j-state.json.tmp-";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Exact format identity required by the current binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateManifest {
    product: String,
    storage_format: u32,
    config_format: u32,
}

impl StateManifest {
    /// Construct the only manifest accepted by this binary.
    pub fn current() -> Self {
        Self {
            product: PRODUCT.to_string(),
            storage_format: CURRENT_STORAGE_FORMAT,
            config_format: CURRENT_CONFIG_FORMAT,
        }
    }

    fn validate(&self) -> Result<(), StateFormatError> {
        let expected = Self::current();
        if self == &expected {
            return Ok(());
        }
        Err(StateFormatError::Incompatible {
            path: PathBuf::new(),
            found: self.clone(),
            expected,
        })
    }
}

/// Result of preparing a data directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateFormatStatus {
    /// A manifest was atomically installed into a fresh directory.
    Initialized,
    /// An existing manifest exactly matched the current format.
    Current,
}

/// Fail-closed state format errors.
#[derive(Debug)]
pub enum StateFormatError {
    /// Filesystem metadata or I/O could not be read safely.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The selected data path is not a real directory.
    InvalidDataDirectory { path: PathBuf, reason: String },
    /// Existing product state has no manifest and cannot be identified.
    UnidentifiedExistingState { path: PathBuf },
    /// Existing generated configuration has no manifest and cannot be identified.
    UnidentifiedGeneratedConfig { path: PathBuf },
    /// The manifest is malformed or contains an unknown/newer field.
    MalformedManifest { path: PathBuf, reason: String },
    /// The manifest identifies a different format generation.
    Incompatible {
        path: PathBuf,
        found: StateManifest,
        expected: StateManifest,
    },
}

impl StateFormatError {
    fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// Operator action attached to every incompatible-state diagnostic.
    pub fn remediation() -> &'static str {
        "Run `z4j server init --clean -y` to recreate the server state, then reload \
data from the original sources. Existing state is not upgraded or converted."
    }
}

impl fmt::Display for StateFormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(
                formatter,
                "cannot validate Zeta4j state at {}: {source}. {}",
                path.display(),
                Self::remediation()
            ),
            Self::InvalidDataDirectory { path, reason } => write!(
                formatter,
                "invalid Zeta4j data directory {}: {reason}. {}",
                path.display(),
                Self::remediation()
            ),
            Self::UnidentifiedExistingState { path } => write!(
                formatter,
                "refusing unidentified existing Zeta4j state at {} because {} is missing. {}",
                path.display(),
                STATE_MANIFEST_FILE,
                Self::remediation()
            ),
            Self::UnidentifiedGeneratedConfig { path } => write!(
                formatter,
                "refusing unidentified existing Zeta4j generated configuration at {} because \
{} is missing from the data directory. {}",
                path.display(),
                STATE_MANIFEST_FILE,
                Self::remediation()
            ),
            Self::MalformedManifest { path, reason } => write!(
                formatter,
                "refusing malformed Zeta4j state manifest {}: {reason}. {}",
                path.display(),
                Self::remediation()
            ),
            Self::Incompatible {
                path,
                found,
                expected,
            } => write!(
                formatter,
                "refusing incompatible Zeta4j state at {}: found product={}, storage_format={}, \
config_format={}; expected product={}, storage_format={}, config_format={}. {}",
                path.display(),
                found.product,
                found.storage_format,
                found.config_format,
                expected.product,
                expected.storage_format,
                expected.config_format,
                Self::remediation()
            ),
        }
    }
}

impl std::error::Error for StateFormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Validate generated configuration ownership, then prepare the data directory.
///
/// A pre-manifest configuration directory containing files generated by an
/// older `z4j` cannot be labeled as current merely because its data directory
/// is empty. Files unrelated to Zeta4j's generated configuration are ignored.
pub fn prepare_runtime_state(
    data_directory: impl AsRef<Path>,
    config_directory: impl AsRef<Path>,
) -> Result<StateFormatStatus, StateFormatError> {
    let data_directory = data_directory.as_ref();
    let manifest_path = data_directory.join(STATE_MANIFEST_FILE);
    if !manifest_path.exists() {
        let config_directory = config_directory.as_ref();
        if has_generated_config(config_directory)? {
            return Err(StateFormatError::UnidentifiedGeneratedConfig {
                path: config_directory.to_path_buf(),
            });
        }
    }
    prepare_data_directory(data_directory)
}

fn has_generated_config(config_directory: &Path) -> Result<bool, StateFormatError> {
    for filename in GENERATED_CONFIG_FILES {
        let path = config_directory.join(filename);
        match fs::symlink_metadata(&path) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(StateFormatError::io(path, error)),
        }
    }
    Ok(false)
}

/// Validate an existing initialized data directory without modifying it.
pub fn validate_data_directory(
    data_directory: impl AsRef<Path>,
) -> Result<StateManifest, StateFormatError> {
    let data_directory = data_directory.as_ref();
    validate_directory_path(data_directory)?;
    let manifest_path = data_directory.join(STATE_MANIFEST_FILE);
    read_manifest(&manifest_path)
}

/// Validate current state or atomically initialize a completely fresh directory.
///
/// A non-empty directory without a manifest is never adopted. This is the
/// boundary that prevents an old database, WAL, or snapshot from
/// being silently opened as current state.
pub fn prepare_data_directory(
    data_directory: impl AsRef<Path>,
) -> Result<StateFormatStatus, StateFormatError> {
    let data_directory = data_directory.as_ref();
    if data_directory.exists() {
        validate_directory_path(data_directory)?;
    } else {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(data_directory)
            .map_err(|error| StateFormatError::io(data_directory, error))?;
    }

    let manifest_path = data_directory.join(STATE_MANIFEST_FILE);
    if manifest_path.exists() {
        read_manifest(&manifest_path)?;
        return Ok(StateFormatStatus::Current);
    }

    if !directory_is_fresh(data_directory, &manifest_path)? {
        if manifest_path.exists() {
            read_manifest(&manifest_path)?;
            return Ok(StateFormatStatus::Current);
        }
        return Err(StateFormatError::UnidentifiedExistingState {
            path: data_directory.to_path_buf(),
        });
    }

    install_current_manifest(data_directory, &manifest_path)?;
    read_manifest(&manifest_path)?;
    Ok(StateFormatStatus::Initialized)
}

fn directory_is_fresh(
    data_directory: &Path,
    manifest_path: &Path,
) -> Result<bool, StateFormatError> {
    for _ in 0..100 {
        let entries = fs::read_dir(data_directory)
            .map_err(|error| StateFormatError::io(data_directory, error))?
            .map(|entry| {
                entry
                    .map(|entry| entry.file_name())
                    .map_err(|error| StateFormatError::io(data_directory, error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if entries.is_empty() {
            return Ok(true);
        }
        if entries
            .iter()
            .all(|name| name.to_string_lossy().starts_with(TEMP_PREFIX))
        {
            if manifest_path.exists() {
                read_manifest(manifest_path)?;
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        return Ok(false);
    }
    Ok(false)
}

fn validate_directory_path(path: &Path) -> Result<(), StateFormatError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| StateFormatError::io(path, error))?;
    if metadata.file_type().is_symlink() {
        return Err(StateFormatError::InvalidDataDirectory {
            path: path.to_path_buf(),
            reason: "symbolic links are not accepted as the state root".to_string(),
        });
    }
    if !metadata.is_dir() {
        return Err(StateFormatError::InvalidDataDirectory {
            path: path.to_path_buf(),
            reason: "path is not a directory".to_string(),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.mode() & 0o777 != 0o700 {
            return Err(StateFormatError::InvalidDataDirectory {
                path: path.to_path_buf(),
                reason: "state root must have owner-only mode 0700".to_string(),
            });
        }
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<StateManifest, StateFormatError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| StateFormatError::io(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(StateFormatError::MalformedManifest {
            path: path.to_path_buf(),
            reason: "manifest must be a regular file".to_string(),
        });
    }
    let bytes = fs::read(path).map_err(|error| StateFormatError::io(path, error))?;
    let manifest: StateManifest =
        serde_json::from_slice(&bytes).map_err(|error| StateFormatError::MalformedManifest {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    manifest.validate().map_err(|error| match error {
        StateFormatError::Incompatible {
            found, expected, ..
        } => StateFormatError::Incompatible {
            path: path.to_path_buf(),
            found,
            expected,
        },
        other => other,
    })?;
    Ok(manifest)
}

fn install_current_manifest(
    data_directory: &Path,
    manifest_path: &Path,
) -> Result<(), StateFormatError> {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary_path =
        data_directory.join(format!("{TEMP_PREFIX}{}-{sequence}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(&StateManifest::current()).map_err(|error| {
        StateFormatError::MalformedManifest {
            path: manifest_path.to_path_buf(),
            reason: error.to_string(),
        }
    })?;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temporary = options
        .open(&temporary_path)
        .map_err(|error| StateFormatError::io(&temporary_path, error))?;
    if let Err(error) = (|| {
        temporary.write_all(&bytes)?;
        temporary.write_all(b"\n")?;
        temporary.sync_all()
    })() {
        let _ = fs::remove_file(&temporary_path);
        return Err(StateFormatError::io(&temporary_path, error));
    }
    drop(temporary);

    if manifest_path.exists() {
        fs::remove_file(&temporary_path)
            .map_err(|error| StateFormatError::io(&temporary_path, error))?;
        read_manifest(manifest_path)?;
        return Ok(());
    }

    if let Err(error) = fs::rename(&temporary_path, manifest_path) {
        let _ = fs::remove_file(&temporary_path);
        if manifest_path.exists() {
            read_manifest(manifest_path)?;
            return Ok(());
        }
        return Err(StateFormatError::io(manifest_path, error));
    }

    File::open(data_directory)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| StateFormatError::io(data_directory, error))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    /// State roots must be 0700 regardless of the ambient umask.
    fn owner_only(path: &Path) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(not(unix))]
        let _ = path;
        Ok(())
    }

    fn owner_only_tempdir() -> std::io::Result<tempfile::TempDir> {
        let directory = tempdir()?;
        owner_only(directory.path())?;
        Ok(directory)
    }

    #[test]
    fn initializes_and_revalidates_a_fresh_directory() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let data = root.path().join("databases");

        assert_eq!(
            prepare_data_directory(&data)?,
            StateFormatStatus::Initialized
        );
        assert_eq!(prepare_data_directory(&data)?, StateFormatStatus::Current);
        assert_eq!(validate_data_directory(&data)?, StateManifest::current());
        Ok(())
    }

    #[test]
    fn rejects_unidentified_existing_state_without_modifying_it()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let data = root.path().join("databases");
        fs::create_dir_all(data.join("system"))?;
        owner_only(&data)?;
        fs::write(data.join("system").join("system.redb"), b"legacy")?;
        let before = fs::read(data.join("system").join("system.redb"))?;

        let error = prepare_data_directory(&data)
            .err()
            .ok_or("legacy state was unexpectedly accepted")?;

        assert!(matches!(
            error,
            StateFormatError::UnidentifiedExistingState { .. }
        ));
        assert_eq!(fs::read(data.join("system").join("system.redb"))?, before);
        assert!(!data.join(STATE_MANIFEST_FILE).exists());
        Ok(())
    }

    #[test]
    fn rejects_malformed_unknown_and_incompatible_manifests()
    -> Result<(), Box<dyn std::error::Error>> {
        let malformed_root = owner_only_tempdir()?;
        fs::write(malformed_root.path().join(STATE_MANIFEST_FILE), b"not json")?;
        assert!(matches!(
            prepare_data_directory(malformed_root.path()),
            Err(StateFormatError::MalformedManifest { .. })
        ));

        let unknown_root = owner_only_tempdir()?;
        fs::write(
            unknown_root.path().join(STATE_MANIFEST_FILE),
            br#"{"product":"zeta4j","storage_format":1,"config_format":1,"future":true}"#,
        )?;
        assert!(matches!(
            prepare_data_directory(unknown_root.path()),
            Err(StateFormatError::MalformedManifest { .. })
        ));

        let incompatible_root = owner_only_tempdir()?;
        fs::write(
            incompatible_root.path().join(STATE_MANIFEST_FILE),
            br#"{"product":"zeta4j","storage_format":99,"config_format":1}"#,
        )?;
        assert!(matches!(
            prepare_data_directory(incompatible_root.path()),
            Err(StateFormatError::Incompatible { .. })
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_state_roots() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let root = tempdir()?;
        let actual = root.path().join("actual");
        fs::create_dir(&actual)?;
        let link = root.path().join("link");
        symlink(&actual, &link)?;

        assert!(matches!(
            prepare_data_directory(&link),
            Err(StateFormatError::InvalidDataDirectory { .. })
        ));
        Ok(())
    }

    #[test]
    fn rejects_file_roots_and_manifest_type_collisions() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let file_root = root.path().join("not-a-directory");
        fs::write(&file_root, b"file")?;
        assert!(matches!(
            prepare_data_directory(&file_root),
            Err(StateFormatError::InvalidDataDirectory { .. })
        ));

        let manifest_directory_root = owner_only_tempdir()?;
        fs::create_dir(manifest_directory_root.path().join(STATE_MANIFEST_FILE))?;
        assert!(matches!(
            prepare_data_directory(manifest_directory_root.path()),
            Err(StateFormatError::MalformedManifest { .. })
        ));
        Ok(())
    }

    #[test]
    fn concurrent_fresh_initialization_converges_on_one_current_manifest()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::{Arc, Barrier};

        let root = tempdir()?;
        let data = root.path().join("databases");
        fs::create_dir(&data)?;
        owner_only(&data)?;
        let workers = 8;
        let barrier = Arc::new(Barrier::new(workers));
        let handles = (0..workers)
            .map(|_| {
                let data = data.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    prepare_data_directory(data)
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().map_err(|_| "initializer thread panicked")??;
        }
        assert_eq!(validate_data_directory(&data)?, StateManifest::current());
        let temporary_files = fs::read_dir(&data)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX))
            .count();
        assert_eq!(temporary_files, 0);
        Ok(())
    }

    #[test]
    fn refuses_unidentified_generated_config_but_ignores_unowned_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let data = root.path().join("databases");
        let config = root.path().join("config");
        fs::create_dir(&config)?;
        fs::write(config.join("core.toml"), b"legacy = true\n")?;

        assert!(matches!(
            prepare_runtime_state(&data, &config),
            Err(StateFormatError::UnidentifiedGeneratedConfig { .. })
        ));
        assert!(!data.exists());

        fs::remove_file(config.join("core.toml"))?;
        fs::write(config.join("operator-notes.txt"), b"preserve")?;
        assert_eq!(
            prepare_runtime_state(&data, &config)?,
            StateFormatStatus::Initialized
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn refuses_broken_generated_config_symlinks() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let root = tempdir()?;
        let data = root.path().join("databases");
        let config = root.path().join("config");
        fs::create_dir(&config)?;
        symlink(config.join("missing.toml"), config.join("core.toml"))?;

        assert!(matches!(
            prepare_runtime_state(&data, &config),
            Err(StateFormatError::UnidentifiedGeneratedConfig { .. })
        ));
        assert!(!data.exists());
        Ok(())
    }
}
