//! 공통 설정 타입
//!
//! 모든 zeta4j 크레이트에서 사용하는 공통 설정 구조체입니다.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

use crate::defaults::*;

/// 스토리지 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// 데이터 디렉토리
    pub data_dir: PathBuf,

    /// 캐시 크기 (bytes)
    pub cache_size: usize,

    /// fsync 활성화 여부
    pub fsync: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("data"),
            cache_size: 256 * 1024 * 1024, // 256MB
            fsync: true,
        }
    }
}

/// 서버 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// HTTP 호스트
    pub http_host: String,

    /// HTTP 포트
    pub http_port: u16,

    /// Bolt 호스트
    pub bolt_host: String,

    /// Bolt 포트
    pub bolt_port: u16,

    /// 작업자 스레드 수
    pub worker_threads: usize,

    /// 최대 연결 수
    pub max_connections: usize,

    /// 요청 타임아웃
    #[serde(with = "humantime_serde")]
    pub request_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_host: DEFAULT_HOST.to_string(),
            http_port: DEFAULT_BOLT_PORT, // Note: This config uses different port mapping
            bolt_host: DEFAULT_HOST.to_string(),
            bolt_port: DEFAULT_GRPC_PORT,
            worker_threads: num_cpus::get(),
            max_connections: DEFAULT_BOLT_MAX_CONNECTIONS,
            request_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

/// 로깅 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// 로그 레벨
    pub level: String,

    /// 로그 포맷 (json, text)
    pub format: String,

    /// 로그 파일 경로
    pub file: Option<PathBuf>,

    /// 로그 롤링 설정
    pub rolling: Option<RollingConfig>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: DEFAULT_LOG_LEVEL.to_string(),
            format: DEFAULT_LOG_FORMAT.to_string(),
            file: None,
            rolling: None,
        }
    }
}

/// 로그 롤링 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingConfig {
    /// 최대 파일 크기 (bytes)
    pub max_size: usize,

    /// 보관할 파일 수
    pub max_files: usize,
}

/// TLS 설정
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TlsConfig {
    /// TLS 활성화 여부
    pub enabled: bool,

    /// 인증서 경로
    pub cert_file: Option<PathBuf>,

    /// 키 경로
    pub key_file: Option<PathBuf>,

    /// CA 인증서 경로
    pub ca_file: Option<PathBuf>,
}

/// humantime 직렬화 모듈
mod humantime_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&humantime::format_duration(*duration).to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        humantime::parse_duration(&s).map_err(serde::de::Error::custom)
    }
}
