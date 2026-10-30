//! Commands for the server's real split configuration directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::init::create_config_files;

fn resolve_config_dir(home: &Path, explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    let path = explicit.unwrap_or_else(|| home.join("config"));
    if !path.is_dir() {
        return Err(format!(
            "configuration directory not found: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn config_files(config_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(config_dir)
        .map_err(|error| format!("failed to read {}: {error}", config_dir.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("failed to inspect {}: {error}", config_dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("toml") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

const REQUIRED_FILES: [&str; 2] = ["core.toml", "server.toml"];

/// Validate the same split TOML files that `zeta4jd` loads.
pub fn cmd_config_check(home: &Path, explicit: Option<PathBuf>) -> ExitCode {
    let config_dir = match resolve_config_dir(home, explicit) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("Error: {error}");
            eprintln!("Run `z4j server config init` first.");
            return ExitCode::FAILURE;
        }
    };
    let mut errors = Vec::new();
    for name in REQUIRED_FILES {
        let path = config_dir.join(name);
        if !path.is_file() {
            errors.push(format!(
                "required configuration file is missing: {}",
                path.display()
            ));
        }
    }

    let files = match config_files(&config_dir) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };
    for path in &files {
        match fs::read_to_string(path) {
            Ok(content) => {
                if let Err(error) = content.parse::<toml::Table>() {
                    errors.push(format!("invalid {}: {error}", path.display()));
                }
            }
            Err(error) => errors.push(format!("failed to read {}: {error}", path.display())),
        }
    }

    println!("Configuration: {}", config_dir.display());
    println!("Files checked: {}", files.len());
    if errors.is_empty() {
        println!("Configuration check PASSED");
        ExitCode::SUCCESS
    } else {
        for error in errors {
            eprintln!("Error: {error}");
        }
        eprintln!("Configuration check FAILED");
        ExitCode::FAILURE
    }
}

/// Print the TOML files present in the server configuration directory.
pub fn cmd_config_show(home: &Path) -> ExitCode {
    let config_dir = home.join("config");
    let files = match config_files(&config_dir) {
        Ok(files) if !files.is_empty() => files,
        Ok(_) => {
            eprintln!(
                "Error: no TOML configuration files in {}",
                config_dir.display()
            );
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("Error: {error}");
            return ExitCode::FAILURE;
        }
    };

    for path in files {
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                eprintln!("Error: failed to read {}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        };
        println!("# {}", path.display());
        print!("{content}");
        if !content.ends_with('\n') {
            println!();
        }
        println!();
    }
    ExitCode::SUCCESS
}

/// Initialize the split configuration files used by the server.
pub fn cmd_config_init(home: &Path, force: bool) -> ExitCode {
    let config_dir = home.join("config");
    if let Err(error) = fs::create_dir_all(&config_dir) {
        eprintln!("Error: failed to create {}: {error}", config_dir.display());
        return ExitCode::FAILURE;
    }
    if let Err(error) = create_config_files(&config_dir, force) {
        eprintln!("Error: failed to initialize configuration: {error}");
        return ExitCode::FAILURE;
    }
    println!("Configuration initialized: {}", config_dir.display());
    ExitCode::SUCCESS
}
