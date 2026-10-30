//! 통합 설정 에러
//!
//! 모든 설정 모듈에서 사용하는 공통 에러 타입입니다.

use thiserror::Error;

/// 설정 로드/파싱 에러
#[derive(Debug, Error)]
pub enum ConfigError {
    /// IO 에러 (파일 읽기/쓰기)
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// TOML 파싱 에러
    #[error("TOML parse error: {0}")]
    TomlParse(String),

    /// TOML 직렬화 에러
    #[error("TOML serialize error: {0}")]
    TomlSerialize(String),

    /// 환경 변수 파싱 에러
    #[error("Environment variable parse error: {0}")]
    EnvParse(String),

    /// 환경 변수 없음
    #[error("Environment variable not found: {0}")]
    EnvVarNotFound(String),

    /// 필수 설정 누락
    #[error("Missing required config: {0}")]
    MissingConfig(String),

    /// 유효하지 않은 설정
    #[error("Invalid config: {0}")]
    InvalidConfig(String),
}

impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        ConfigError::TomlParse(e.to_string())
    }
}

impl From<toml::ser::Error> for ConfigError {
    fn from(e: toml::ser::Error) -> Self {
        ConfigError::TomlSerialize(e.to_string())
    }
}
