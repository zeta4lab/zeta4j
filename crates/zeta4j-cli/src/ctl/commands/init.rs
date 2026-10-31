// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 저장소 초기화 명령
//!
//! `z4j server init` storage initialization.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeta4j_common::private_fs::{ensure_private_dir, write_private_file};
use zeta4j_common::{
    DEFAULT_BOOTSTRAP_TTL_SECS, DEFAULT_CORE_TOML, DEFAULT_SERVER_TOML, GENERATED_CONFIG_FILES,
    prepare_runtime_state, render_bootstrap_envelope,
};

/// 저장소 초기화
///
/// # 작업 내용
/// 1. 홈 디렉토리 구조 생성 (databases, logs, config)
/// 2. 기본 설정 파일 생성
/// 3. --clean 옵션 시 데이터와 생성 설정 초기화
pub fn cmd_init(
    home: PathBuf,
    force: bool,
    clean: bool,
    yes: bool,
    generate_bootstrap_credential: bool,
    bootstrap_output: Option<&Path>,
) -> ExitCode {
    let bootstrap_output_to_create = if generate_bootstrap_credential {
        let Some(output) = bootstrap_output else {
            eprintln!("Error: --bootstrap-output is required when generating a credential");
            return ExitCode::FAILURE;
        };
        Some(output)
    } else {
        if clean && let Err(error) = validate_external_bootstrap_source() {
            eprintln!("Error: [SV-BOOT-006] {error}");
            return ExitCode::FAILURE;
        }
        None
    };
    if clean
        && let Some(output) = bootstrap_output_to_create
        && let Err(error) = validate_bootstrap_output_location(&home, output)
    {
        eprintln!("Error: bootstrap output is not durable across clean init: {error}");
        return ExitCode::FAILURE;
    }

    println!("Initializing Zeta4j storage...");
    println!("  Home: {}", home.display());

    // 0. --clean 옵션: 데이터와 도구 소유 설정 초기화
    if clean {
        let db_dir = home.join("databases");
        let config_dir = home.join("config");
        if !yes {
            println!("\n⚠️  WARNING: clean reinitialization will:");
            println!("   - delete all data in {}", db_dir.display());
            println!(
                "   - replace generated Zeta4j configuration files in {}",
                config_dir.display()
            );
            println!("   - delete logs in {}", home.join("logs").display());
            print!("   Continue? [y/N] ");
            if io::stdout().flush().is_err() {
                eprintln!("Error: Failed to write confirmation prompt");
                return ExitCode::FAILURE;
            }

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_err() {
                eprintln!("Error: Failed to read input");
                return ExitCode::FAILURE;
            }

            let input = input.trim().to_lowercase();
            if input != "y" && input != "yes" {
                println!("Aborted.");
                return ExitCode::SUCCESS;
            }
        }

        if let Some(output) = bootstrap_output_to_create {
            if let Err(error) = create_bootstrap_envelope(output) {
                eprintln!(
                    "Error: Failed to create bootstrap credential file '{}': {error}",
                    output.display()
                );
                return ExitCode::FAILURE;
            }
            println!(
                "Created one-time bootstrap credential: {}",
                output.display()
            );
        }

        if db_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&db_dir) {
                eprintln!("Error: Failed to remove databases directory: {}", e);
                return ExitCode::FAILURE;
            }
            println!("  Cleaned: {}", db_dir.display());
        }

        for filename in GENERATED_CONFIG_FILES {
            let path = config_dir.join(filename);
            if path.exists() {
                if let Err(error) = std::fs::remove_file(&path) {
                    eprintln!(
                        "Error: Failed to remove generated configuration '{}': {}",
                        path.display(),
                        error
                    );
                    return ExitCode::FAILURE;
                }
                println!("  Cleaned: {}", path.display());
            }
        }

        let logs_dir = home.join("logs");
        if logs_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&logs_dir) {
                eprintln!("Warning: Failed to remove logs directory: {}", e);
            } else {
                println!("  Cleaned: {}", logs_dir.display());
            }
        }
    }

    if !clean && let Some(output) = bootstrap_output_to_create {
        if let Err(error) = create_bootstrap_envelope(output) {
            eprintln!(
                "Error: Failed to create bootstrap credential file '{}': {error}",
                output.display()
            );
            return ExitCode::FAILURE;
        }
        println!(
            "Created one-time bootstrap credential: {}",
            output.display()
        );
    }

    let data_dir = home.join("databases");
    let config_dir = home.join("config");
    if let Err(error) = prepare_runtime_state(&data_dir, &config_dir) {
        eprintln!("Error: {error}");
        return ExitCode::FAILURE;
    }

    // 1. 형식 검증을 통과한 뒤 나머지 디렉토리 생성
    if let Err(e) = create_directories(&home) {
        eprintln!("Error: Failed to create directories: {}", e);
        return ExitCode::FAILURE;
    }

    // 2. 설정 파일 생성
    if let Err(e) = create_config_files(&config_dir, force || clean) {
        eprintln!("Error: Failed to create config files: {}", e);
        return ExitCode::FAILURE;
    }

    println!("\nInitialization complete!");
    println!("\nDirectory structure:");
    println!("  {}/", home.display());
    println!("  ├── databases/    # Graph data storage");
    println!("  ├── logs/         # Server logs");
    println!("  └── config/       # Configuration files");

    println!("\nNext steps:");
    println!(
        "  1. Review configuration files in {}/",
        config_dir.display()
    );
    println!("  2. Start the server: z4j server start");

    ExitCode::SUCCESS
}

fn validate_external_bootstrap_source() -> io::Result<()> {
    let file = std::env::var_os("ZETA4J_BOOTSTRAP_CREDENTIAL_FILE");
    let username = std::env::var("ZETA4J_BOOTSTRAP_USERNAME").ok();
    let password = std::env::var("ZETA4J_BOOTSTRAP_PASSWORD").ok();
    let expiry = std::env::var("ZETA4J_BOOTSTRAP_EXPIRES_AT_UNIX").ok();
    if file.is_some() && (username.is_some() || password.is_some() || expiry.is_some()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bootstrap file and direct environment sources cannot be combined",
        ));
    }
    if let Some(path) = file {
        return validate_external_bootstrap_file(Path::new(&path));
    }
    match (username, password, expiry) {
        (Some(username), Some(password), Some(expiry)) => {
            if username.trim().is_empty() || !valid_bootstrap_password(&password) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "external bootstrap credential does not meet policy",
                ));
            }
            let expiry = expiry
                .parse::<u64>()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid expiry"))?;
            validate_expiry(expiry)?;
            Ok(())
        }
        (None, None, None) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "clean initialization requires an external or generated bootstrap credential",
        )),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bootstrap username, password, and expiry must be provided together",
        )),
    }
}

fn validate_external_bootstrap_file(path: &Path) -> io::Result<()> {
    let content = zeta4j_common::load_secret_file(path, "ZETA4J_BOOTSTRAP_CREDENTIAL_FILE")
        .map_err(io::Error::other)?;
    let envelope: toml::Value = toml::from_str(&content)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid bootstrap envelope"))?;
    let table = envelope
        .as_table()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid bootstrap envelope"))?;
    let version = table.get("version").and_then(toml::Value::as_integer);
    let username = table.get("username").and_then(toml::Value::as_str);
    let password = table.get("password").and_then(toml::Value::as_str);
    let expiry = table
        .get("expires_at_unix")
        .and_then(toml::Value::as_integer);
    if table.len() != 4
        || version != Some(i64::from(zeta4j_common::BOOTSTRAP_ENVELOPE_VERSION))
        || username.is_none_or(|value| value.trim().is_empty())
        || password.is_none_or(|value| !valid_bootstrap_password(value))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid bootstrap envelope",
        ));
    }
    let expiry = expiry
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid bootstrap expiry"))?;
    validate_expiry(expiry)
}

fn valid_bootstrap_password(password: &str) -> bool {
    zeta4j_credential::PasswordPolicy::default()
        .validate(&zeta4j_credential::CredentialSecret::new(password))
        .is_ok()
}

fn validate_expiry(expiry: u64) -> io::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    if expiry <= now {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "bootstrap credential has expired",
        ));
    }
    Ok(())
}

fn validate_bootstrap_output_location(home: &Path, output: &Path) -> io::Result<()> {
    let parent = output.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "bootstrap output requires an existing parent directory",
        )
    })?;
    let resolved_parent = parent.canonicalize()?;
    for removed_dir in [home.join("databases"), home.join("logs")] {
        let resolved_removed = removed_dir
            .canonicalize()
            .unwrap_or_else(|_| removed_dir.clone());
        if resolved_parent.starts_with(&resolved_removed) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output cannot be placed in a directory removed by --clean",
            ));
        }
    }
    let output_name = output.file_name();
    if resolved_parent
        == home
            .join("config")
            .canonicalize()
            .unwrap_or_else(|_| home.join("config"))
        && output_name.is_some_and(|name| {
            GENERATED_CONFIG_FILES
                .iter()
                .any(|generated| name == std::ffi::OsStr::new(generated))
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output cannot replace a generated configuration file",
        ));
    }
    Ok(())
}

fn create_bootstrap_envelope(path: &Path) -> io::Result<()> {
    let random: [u8; 32] = rand::random();
    let username = format!("operator-{}", hex::encode(&random[..8]));
    let password = hex::encode(random);
    let expires_at_unix = SystemTime::now()
        .checked_add(Duration::from_secs(DEFAULT_BOOTSTRAP_TTL_SECS))
        .ok_or_else(|| io::Error::other("bootstrap expiry overflow"))?
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    let content = render_bootstrap_envelope(&username, &password, expires_at_unix);

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()
}

/// 디렉토리 구조 생성
fn create_directories(home: &Path) -> std::io::Result<()> {
    prepare_owner_only_directory(home)?;
    let dirs = vec![
        home.join("databases"),
        home.join("logs"),
        home.join("config"),
    ];

    for dir in &dirs {
        if !dir.exists() {
            prepare_owner_only_directory(dir)?;
            println!("  Created: {}", dir.display());
        } else {
            prepare_owner_only_directory(dir)?;
            println!("  Exists:  {}", dir.display());
        }
    }

    Ok(())
}

fn prepare_owner_only_directory(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("runtime path is not a real directory: {}", path.display()),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    ensure_private_dir(path)
}

/// 기본 설정 파일 생성
pub(super) fn create_config_files(config_dir: &Path, force: bool) -> std::io::Result<()> {
    write_config_if_needed(config_dir, "core.toml", DEFAULT_CORE_TOML, force)?;
    write_config_if_needed(config_dir, "server.toml", DEFAULT_SERVER_TOML, force)
}

/// 설정 파일 작성 (force가 아니면 기존 파일 유지)
fn write_config_if_needed(
    config_dir: &Path,
    filename: &str,
    content: &str,
    force: bool,
) -> std::io::Result<()> {
    let path = config_dir.join(filename);
    let existed = path.exists();

    if existed && !force {
        println!(
            "  Skipped: {} (already exists, use --force to overwrite)",
            filename
        );
        return Ok(());
    }

    write_private_file(&path, content.as_bytes())?;

    if existed {
        println!("  Overwrote: {}", filename);
    } else {
        println!("  Created: {}", filename);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;
    use zeta4j_common::{STATE_MANIFEST_FILE, validate_data_directory};

    use super::{cmd_init, validate_external_bootstrap_file};

    #[cfg(unix)]
    #[test]
    fn bootstrap_output_is_mode_0600() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let home = root.path().join("home");
        let output = root.path().join("bootstrap.json");

        assert_eq!(
            cmd_init(home, false, false, true, true, Some(&output)),
            std::process::ExitCode::SUCCESS
        );
        assert_eq!(fs::metadata(&output)?.permissions().mode() & 0o777, 0o600);
        Ok(())
    }

    #[test]
    fn initializes_current_state_manifest() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let home = root.path().join("home");

        assert_eq!(
            cmd_init(home.clone(), false, false, true, false, None,),
            std::process::ExitCode::SUCCESS
        );
        validate_data_directory(home.join("databases"))?;
        assert!(home.join("databases").join(STATE_MANIFEST_FILE).is_file());
        assert!(home.join("config").join("server.toml").is_file());
        #[cfg(unix)]
        {
            for directory in [
                home.clone(),
                home.join("databases"),
                home.join("logs"),
                home.join("config"),
            ] {
                assert_eq!(fs::metadata(directory)?.permissions().mode() & 0o777, 0o700);
            }
            for config in ["core.toml", "server.toml"] {
                assert_eq!(
                    fs::metadata(home.join("config").join(config))?
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
        }
        Ok(())
    }

    #[test]
    fn refuses_to_adopt_unidentified_existing_data() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let home = root.path().join("home");
        let legacy = home.join("databases").join("system");
        fs::create_dir_all(&legacy)?;
        fs::write(legacy.join("system.redb"), b"legacy")?;

        assert_eq!(
            cmd_init(home.clone(), false, false, true, false, None,),
            std::process::ExitCode::FAILURE
        );
        assert_eq!(
            cmd_init(home.clone(), true, false, true, false, None,),
            std::process::ExitCode::FAILURE
        );
        assert_eq!(
            fs::read(home.join("databases/system/system.redb"))?,
            b"legacy"
        );
        assert!(!home.join("databases").join(STATE_MANIFEST_FILE).exists());
        assert!(!home.join("logs").exists());
        assert!(!home.join("config").exists());
        Ok(())
    }

    #[test]
    fn refuses_to_adopt_unidentified_generated_configuration()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let home = root.path().join("home");
        fs::create_dir_all(home.join("config"))?;
        fs::write(home.join("config/core.toml"), b"legacy = true\n")?;

        assert_eq!(
            cmd_init(home.clone(), false, false, true, false, None,),
            std::process::ExitCode::FAILURE
        );
        assert_eq!(fs::read(home.join("config/core.toml"))?, b"legacy = true\n");
        assert!(!home.join("databases").exists());
        assert!(!home.join("logs").exists());
        Ok(())
    }

    #[test]
    fn clean_reinitializes_generated_state_and_preserves_unowned_config()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let home = root.path().join("home");
        fs::create_dir_all(home.join("databases"))?;
        fs::write(home.join("databases/legacy.db"), b"legacy")?;
        fs::create_dir_all(home.join("config"))?;
        fs::write(home.join("config/core.toml"), b"legacy = true\n")?;
        fs::write(home.join("config/operator-notes.txt"), b"preserve")?;
        let bootstrap_path = root.path().join("bootstrap.toml");

        assert_eq!(
            cmd_init(home.clone(), false, true, true, true, Some(&bootstrap_path),),
            std::process::ExitCode::SUCCESS
        );
        assert!(bootstrap_path.is_file());
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&bootstrap_path)?.permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            cmd_init(
                home.clone(),
                false,
                false,
                true,
                true,
                Some(&bootstrap_path),
            ),
            std::process::ExitCode::FAILURE
        );
        validate_data_directory(home.join("databases"))?;
        assert!(!home.join("databases/legacy.db").exists());
        assert_ne!(fs::read(home.join("config/core.toml"))?, b"legacy = true\n");
        assert_eq!(
            fs::read(home.join("config/operator-notes.txt"))?,
            b"preserve"
        );
        Ok(())
    }

    #[test]
    fn clean_rejects_bootstrap_output_that_it_would_delete()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempdir()?;
        let home = root.path().join("home");
        fs::create_dir_all(home.join("databases"))?;
        fs::write(home.join("databases/legacy.db"), b"legacy")?;
        let bootstrap_path = home.join("databases/bootstrap.toml");

        assert_eq!(
            cmd_init(home.clone(), false, true, true, true, Some(&bootstrap_path),),
            std::process::ExitCode::FAILURE
        );
        assert_eq!(fs::read(home.join("databases/legacy.db"))?, b"legacy");
        assert!(!bootstrap_path.exists());
        Ok(())
    }

    #[test]
    fn invalid_external_envelope_is_rejected_before_use() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempdir()?;
        let bootstrap_path = root.path().join("bootstrap.toml");
        fs::write(&bootstrap_path, b"version = 1\npassword = \"too-short\"\n")?;
        #[cfg(unix)]
        fs::set_permissions(&bootstrap_path, fs::Permissions::from_mode(0o600))?;

        assert!(validate_external_bootstrap_file(&bootstrap_path).is_err());
        Ok(())
    }
}
