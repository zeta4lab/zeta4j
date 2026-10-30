//! 통합 설정 모듈
//!
//! 모든 설정 타입, 로더, 에러를 중앙 집중화합니다.
//!
//! # 구조
//!
//! - `core`: Zeta4jConfig (그래프 엔진 핵심 설정)
//! - `server`: UnifiedServerConfig (HTTP, Bolt, Auth, 모니터링)
//! - `vector`: VectorConfig (벡터 인덱스 설정)
//! - `loader`: ConfigLoader (TOML 로드 + 환경변수 치환)
//! - `error`: ConfigError (통합 에러 타입)
//!
//! # 설정 로드
//!
//! ```rust,ignore
//! use zeta4j_graph::config::{Edition, ConfigRegistry};
//!
//! let registry = ConfigRegistry::for_edition(Edition::Base)?;
//! let core = registry.core();
//! let server = registry.server().unwrap();
//! ```
//!
//! # 기본 사용 예제
//!
//! ```rust,ignore
//! use zeta4j_graph::config::{Zeta4jConfig, ConfigLoader, ConfigError};
//!
//! // 기본 설정 로드
//! let config = Zeta4jConfig::default();
//!
//! // 파일에서 로드 (${VAR} 치환 지원)
//! let config: Zeta4jConfig = ConfigLoader::from_file("zeta4j.toml")?;
//! ```

use std::path::Path;

pub mod core;
pub mod error;
pub mod loader;
pub mod server;
pub mod vector;

// Re-exports - Error & Loader
pub use error::ConfigError;
pub use loader::ConfigLoader;

// Re-exports - Core Config
pub use core::{
    CliSettings, CompactionSettings, DriverSettings, IndexSettings, LoggingSettings, MvccSettings,
    PoolConfig, QuerySettings, StorageSettings, TransactionSettings, WalSettings, Zeta4jConfig,
};

// Re-exports - Vector Config
pub use vector::{HnswConfig, SearchConfig, VectorConfig, VectorStorageConfig};

// Re-exports - Server Config
pub use server::{
    AuthServerConfig, BoltServerConfig, CorsServerConfig, GrpcServerConfig, HttpServerConfig,
    TransportServerConfig, UnifiedServerConfig,
};

// ============================================================================
// Edition & ConfigRegistry
// ============================================================================

/// Zeta4j 에디션 (zeta4j-common에서 정의, re-export)
pub use zeta4j_common::Edition;

/// 설정 레지스트리
///
/// 서버가 필요로 하는 설정 모듈을 로드합니다.
///
/// # 예제
///
/// ```rust,ignore
/// use zeta4j_graph::config::{Edition, ConfigRegistry};
///
/// // 기본값으로 설정 로드
/// let registry = ConfigRegistry::for_edition(Edition::Base)?;
///
/// // 설정 파일 경로 지정
/// let registry = ConfigRegistry::for_edition_with_path(
///     Edition::Base,
///     "~/.zeta4j"
/// )?;
///
/// // 개별 설정 접근
/// let core = registry.core();
/// let server = registry.server().unwrap();
/// ```
#[derive(Debug)]
pub struct ConfigRegistry {
    edition: Edition,
    core: Zeta4jConfig,
    server: Option<UnifiedServerConfig>,
}

impl ConfigRegistry {
    /// 에디션에 맞는 설정 로드 (기본 경로: ~/.zeta4j/config)
    pub fn for_edition(edition: Edition) -> Result<Self, ConfigError> {
        Self::for_edition_with_path(edition, "~/.zeta4j/config")
    }

    /// 에디션에 맞는 설정 로드 (경로 지정)
    pub fn for_edition_with_path<P: AsRef<Path>>(
        edition: Edition,
        config_dir: P,
    ) -> Result<Self, ConfigError> {
        let config_dir = expand_tilde(config_dir.as_ref());
        let mut registry = Self {
            edition,
            core: Self::load_core(&config_dir)?,
            server: None,
        };

        registry.server = Some(Self::load_server(&config_dir)?);

        Ok(registry)
    }

    /// 기본값으로 레지스트리 생성 (파일 로드 없음)
    pub fn with_defaults(edition: Edition) -> Self {
        Self {
            edition,
            core: Zeta4jConfig::default(),
            server: Some(UnifiedServerConfig::default()),
        }
    }

    /// 현재 에디션
    pub fn edition(&self) -> Edition {
        self.edition
    }

    /// Core 설정 (Phase 1) - 항상 존재
    pub fn core(&self) -> &Zeta4jConfig {
        &self.core
    }

    /// Core 설정 변경 가능 참조
    pub fn core_mut(&mut self) -> &mut Zeta4jConfig {
        &mut self.core
    }

    /// Server 설정 (Phase 2)
    pub fn server(&self) -> Option<&UnifiedServerConfig> {
        self.server.as_ref()
    }

    /// Server 설정 변경 가능 참조
    pub fn server_mut(&mut self) -> Option<&mut UnifiedServerConfig> {
        self.server.as_mut()
    }

    // Private loaders

    fn load_core(config_dir: &Path) -> Result<Zeta4jConfig, ConfigError> {
        let path = config_dir.join("core.toml");
        if !path.is_file() {
            return Err(ConfigError::MissingConfig(path.display().to_string()));
        }
        ConfigLoader::from_file(&path)
    }

    fn load_server(config_dir: &Path) -> Result<UnifiedServerConfig, ConfigError> {
        let path = config_dir.join("server.toml");
        if !path.is_file() {
            return Err(ConfigError::MissingConfig(path.display().to_string()));
        }
        ConfigLoader::from_file(&path)
    }
}

/// ~ 를 홈 디렉토리로 확장
fn expand_tilde(path: &Path) -> std::path::PathBuf {
    let path_str = path.to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if path_str == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeta4j_common::{DEFAULT_CORE_TOML, DEFAULT_SERVER_TOML};

    #[test]
    fn canonical_templates_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        std::fs::write(directory.path().join("core.toml"), DEFAULT_CORE_TOML)?;
        std::fs::write(directory.path().join("server.toml"), DEFAULT_SERVER_TOML)?;

        let registry = ConfigRegistry::for_edition_with_path(Edition::Base, directory.path())?;
        assert_eq!(registry.server().map(|server| server.grpc.port), Some(9046));
        Ok(())
    }

    #[test]
    fn unknown_server_field_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        std::fs::write(directory.path().join("core.toml"), DEFAULT_CORE_TOML)?;
        std::fs::write(
            directory.path().join("server.toml"),
            format!("{DEFAULT_SERVER_TOML}\nunknown = true\n"),
        )?;
        assert!(ConfigRegistry::for_edition_with_path(Edition::Base, directory.path()).is_err());
        Ok(())
    }
}
