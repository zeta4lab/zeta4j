//! Persisted launch parameters for the daemon managed by `z4j server`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeState {
    pub pid: u32,
    pub http_host: String,
    pub https_host: String,
    pub bolt_host: String,
    pub grpc_host: String,
    pub http_port: u16,
    pub https_port: Option<u16>,
    pub bolt_port: u16,
    pub grpc_port: u16,
}

fn state_path(home: &Path) -> PathBuf {
    home.join("zeta4j.state.json")
}

pub fn read_runtime_state(home: &Path) -> Result<RuntimeState, String> {
    let path = state_path(home);
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    serde_json::from_str(&content).map_err(|error| format!("invalid {}: {error}", path.display()))
}

pub fn write_runtime_state(home: &Path, state: &RuntimeState) -> Result<(), String> {
    let path = state_path(home);
    let temporary = path.with_extension("json.tmp");
    let content = serde_json::to_vec_pretty(state)
        .map_err(|error| format!("failed to serialize runtime state: {error}"))?;
    fs::write(&temporary, content)
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path).map_err(|error| {
        format!(
            "failed to publish runtime state {}: {error}",
            path.display()
        )
    })
}

pub fn remove_runtime_state(home: &Path) -> Result<(), String> {
    let path = state_path(home);
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
    }
    Ok(())
}

pub fn runtime_state_path(home: &Path) -> PathBuf {
    state_path(home)
}

/// HTTP endpoint URL in the server's transport mode.
pub fn web_url(tls: bool, host: &str, port: u16) -> String {
    format!("{}://{host}:{port}", if tls { "https" } else { "http" })
}

/// Bolt endpoint URL in the server's transport mode.
pub fn bolt_url(tls: bool, host: &str, port: u16) -> String {
    format!("{}://{host}:{port}", if tls { "bolt+s" } else { "bolt" })
}
