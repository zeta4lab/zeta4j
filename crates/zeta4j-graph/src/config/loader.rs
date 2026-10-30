//! 통합 설정 로더
//!
//! TOML 파일 및 환경변수에서 설정을 로드하고, `${VAR}` 구문을 치환합니다.
//!
//! # 사용 예제
//!
//! ```rust,ignore
//! use zeta4j_graph::config::ConfigLoader;
//!
//! // TOML 파일에서 로드 (제네릭)
//! let config: MyConfig = ConfigLoader::from_file("config.toml")?;
//!
//! // TOML 문자열 파싱
//! let config: MyConfig = ConfigLoader::parse(toml_content)?;
//! ```
//!
//! # 환경변수 치환
//!
//! TOML 파일 내에서 `${VAR_NAME}` 구문을 사용하면 환경변수 값으로 치환됩니다.
//!
//! ```toml
//! [storage]
//! data_dir = "${ZETA4J_DATA_DIR}"
//! ```

use std::path::Path;

use regex::Regex;
use serde::de::DeserializeOwned;

use super::error::ConfigError;

/// 설정 로더
///
/// TOML 파일 로드 및 환경변수 치환을 담당합니다.
pub struct ConfigLoader;

impl ConfigLoader {
    /// TOML 파일에서 설정 로드
    ///
    /// 파일 내용을 읽고 `${VAR}` 패턴을 환경변수 값으로 치환한 후 파싱합니다.
    pub fn from_file<T, P>(path: P) -> Result<T, ConfigError>
    where
        T: DeserializeOwned,
        P: AsRef<Path>,
    {
        let content = std::fs::read_to_string(path)?;
        Self::parse(&content)
    }

    /// TOML 문자열 파싱
    ///
    /// `${VAR}` 패턴을 환경변수 값으로 치환한 후 파싱합니다.
    pub fn parse<T: DeserializeOwned>(content: &str) -> Result<T, ConfigError> {
        let resolved = Self::resolve_all_env_vars(content);
        toml::from_str(&resolved).map_err(ConfigError::from)
    }

    /// 문자열 내 모든 `${VAR}` 패턴을 환경변수 값으로 치환
    ///
    /// 환경변수가 없으면 원래 패턴을 유지합니다.
    pub fn resolve_all_env_vars(content: &str) -> String {
        let Ok(re) = Regex::new(r"\$\{([^}]+)\}") else {
            return content.to_owned();
        };
        re.replace_all(content, |caps: &regex::Captures| {
            let var_name = &caps[1];
            std::env::var(var_name).unwrap_or_else(|_| format!("${{{}}}", var_name))
        })
        .to_string()
    }

    /// 단일 문자열에서 `${VAR}` 추출
    ///
    /// `${VAR_NAME}` 형식이면 `VAR_NAME`을 반환합니다.
    pub fn extract_env_var_name(s: &str) -> Option<&str> {
        if s.starts_with("${") && s.ends_with("}") && s.len() > 3 {
            Some(&s[2..s.len() - 1])
        } else {
            None
        }
    }

    /// 파일 우선, 없으면 fallback 로더에서 로드
    ///
    /// 파일이 존재하면 파일에서 로드하고, 없으면 fallback 함수를 호출합니다.
    pub fn load_with_fallback<T, F, P>(path: P, fallback: F) -> Result<T, ConfigError>
    where
        T: DeserializeOwned,
        F: FnOnce() -> T,
        P: AsRef<Path>,
    {
        if path.as_ref().exists() {
            Self::from_file(path)
        } else {
            Ok(fallback())
        }
    }

    /// 설정을 TOML 문자열로 직렬화
    pub fn to_toml_string<T: serde::Serialize>(config: &T) -> Result<String, ConfigError> {
        toml::to_string_pretty(config).map_err(ConfigError::from)
    }

    /// 설정을 파일로 저장
    pub fn save_to_file<T, P>(config: &T, path: P) -> Result<(), ConfigError>
    where
        T: serde::Serialize,
        P: AsRef<Path>,
    {
        let content = Self::to_toml_string(config)?;

        // 디렉토리 생성
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, content)?;
        Ok(())
    }
}
