//! Default values for Zeta4j server configuration.
//!
//! 이 모듈은 `zeta4j-common::defaults`를 re-export합니다.
//! 모든 디폴트값은 `zeta4j-common`에서 중앙 관리됩니다.

use std::path::PathBuf;

// Re-export all defaults from zeta4j-common
pub use zeta4j_common::defaults::*;

// =============================================================================
// Path Helper Functions (CLI specific)
// =============================================================================

/// Get default Zeta4j home directory
///
/// Priority:
/// 1. ZETA4J_HOME environment variable
/// 2. ~/.zeta4j (fallback)
pub fn default_zeta4j_home() -> PathBuf {
    std::env::var("ZETA4J_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".zeta4j")
        })
}

/// Get default data directory (~/.zeta4j/databases)
pub fn default_data_dir() -> PathBuf {
    default_zeta4j_home().join("databases")
}

/// Get default log directory (~/.zeta4j/logs)
pub fn default_log_dir() -> PathBuf {
    default_zeta4j_home().join("logs")
}

/// Get default config directory (~/.zeta4j/config)
pub fn default_config_dir() -> PathBuf {
    default_zeta4j_home().join("config")
}
