// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! # Zeta4j Core 설정 모듈
//!
//! 그래프 엔진 코어 설정: transaction, wal, compaction, mvcc, query, index
//!
//! ## 설정 우선순위
//!
//! 1. 환경 변수 (최우선)
//! 2. 설정 파일 (`~/.zeta4j/config.toml`)
//! 3. 기본값
//!
//! ## 환경 변수 형식
//!
//! `ZETA4J_<SECTION>_<KEY>` 형식으로 지정합니다.
//!
//! 예시:
//! - `ZETA4J_TRANSACTION_TIMEOUT_MS=10000`
//! - `ZETA4J_WAL_MAX_SEGMENT_SIZE=134217728`
//! - `ZETA4J_MVCC_MAX_WRITE_SETS=200`
//!
//! ## 설정 파일 예시
//!
//! ```toml
//! [transaction]
//! timeout_ms = 5000
//! [wal]
//! max_segment_size = 67108864
//! max_segments = 10
//! flush_interval = 100
//!
//! [mvcc]
//! max_write_sets = 100
//! ```

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::error::ConfigError;

/// Zeta4j 통합 설정
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Zeta4jConfig {
    /// 로깅 설정
    #[serde(default)]
    pub logging: LoggingSettings,

    /// 스토리지 설정
    #[serde(default)]
    pub storage: StorageSettings,

    /// 트랜잭션 설정
    #[serde(default)]
    pub transaction: TransactionSettings,

    /// WAL 설정
    #[serde(default)]
    pub wal: WalSettings,

    /// 압축 설정
    #[serde(default)]
    pub compaction: CompactionSettings,

    /// MVCC 설정
    #[serde(default)]
    pub mvcc: MvccSettings,

    /// 쿼리 설정
    #[serde(default)]
    pub query: QuerySettings,

    /// 인덱스 설정
    #[serde(default)]
    pub index: IndexSettings,
}

/// 로깅 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingSettings {
    /// 로그 레벨 (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// 로그 포맷 (json, text)
    #[serde(default = "default_log_format")]
    pub format: String,

    /// 로그 디렉터리
    #[serde(default = "default_log_directory")]
    pub directory: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "json".to_string()
}

fn default_log_directory() -> String {
    "~/.zeta4j/logs".to_string()
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            directory: default_log_directory(),
        }
    }
}

/// 스토리지 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageSettings {
    /// 데이터 디렉토리 경로
    #[serde(default = "default_data_dir")]
    pub data_dir: String,

    /// Optional database-scoped graph-engine memory admission limit.
    #[serde(default, deserialize_with = "deserialize_positive_optional_usize")]
    pub memory_limit_bytes: Option<usize>,
}

fn default_data_dir() -> String {
    "~/.zeta4j/databases".to_string()
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            data_dir: default_data_dir(),
            memory_limit_bytes: None,
        }
    }
}

fn deserialize_positive_optional_usize<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<usize>::deserialize(deserializer)?;
    if value == Some(0) {
        return Err(serde::de::Error::custom(
            "storage.memory_limit_bytes must be a positive integer",
        ));
    }
    Ok(value)
}

/// 트랜잭션 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransactionSettings {
    /// 기본 타임아웃 (밀리초)
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,

    /// 락 대기 시간 (밀리초)
    #[serde(default = "default_lock_wait_ms")]
    pub lock_wait_ms: u64,
}

fn default_timeout_ms() -> u64 {
    5000
}
fn default_lock_wait_ms() -> u64 {
    1000
}

impl Default for TransactionSettings {
    fn default() -> Self {
        Self {
            timeout_ms: default_timeout_ms(),
            lock_wait_ms: default_lock_wait_ms(),
        }
    }
}

/// WAL 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WalSettings {
    /// 세그먼트 최대 크기 (바이트)
    #[serde(default = "default_max_segment_size")]
    pub max_segment_size: u64,

    /// 최대 세그먼트 수
    #[serde(default = "default_max_segments")]
    pub max_segments: usize,

    /// 플러시 간격 (레코드 수)
    #[serde(default = "default_flush_interval")]
    pub flush_interval: usize,
}

fn default_max_segment_size() -> u64 {
    64 * 1024 * 1024
} // 64MB
fn default_max_segments() -> usize {
    10
}
fn default_flush_interval() -> usize {
    100
}
impl Default for WalSettings {
    fn default() -> Self {
        Self {
            max_segment_size: default_max_segment_size(),
            max_segments: default_max_segments(),
            flush_interval: default_flush_interval(),
        }
    }
}

/// 압축 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionSettings {
    /// 압축 시작 최소 레코드 수
    #[serde(default = "default_min_records")]
    pub min_records: usize,

    /// 한 번에 처리할 최대 레코드 수
    #[serde(default = "default_max_records")]
    pub max_records: usize,
}

fn default_min_records() -> usize {
    1000
}
fn default_max_records() -> usize {
    100000
}
impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            min_records: default_min_records(),
            max_records: default_max_records(),
        }
    }
}

/// MVCC 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MvccSettings {
    /// 커밋된 쓰기 세트 최대 보존 개수
    #[serde(default = "default_max_write_sets")]
    pub max_write_sets: usize,

    /// 스냅샷 캐시 크기
    #[serde(default = "default_snapshot_cache_size")]
    pub snapshot_cache_size: usize,
}

fn default_max_write_sets() -> usize {
    100
}
fn default_snapshot_cache_size() -> usize {
    1000
}

impl Default for MvccSettings {
    fn default() -> Self {
        Self {
            max_write_sets: default_max_write_sets(),
            snapshot_cache_size: default_snapshot_cache_size(),
        }
    }
}

/// 인덱스 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexSettings {
    /// 자동 인덱스 생성 활성화
    #[serde(default = "default_auto_create")]
    pub auto_create: bool,

    /// 기본 인덱스 타입 (btree, hash)
    #[serde(default = "default_index_type")]
    pub default_type: String,
}

fn default_auto_create() -> bool {
    true
}
fn default_index_type() -> String {
    "btree".to_string()
}

impl Default for IndexSettings {
    fn default() -> Self {
        Self {
            auto_create: default_auto_create(),
            default_type: default_index_type(),
        }
    }
}

/// 연결 풀 설정
///
/// driver 모듈의 연결 풀 생성에 사용되는 설정입니다.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// 최대 연결 수
    pub max_size: usize,
    /// 최소 유휴 연결 수
    pub min_idle: usize,
    /// 초기화 시 워밍업 수행 여부
    pub warmup_on_init: bool,
    /// 워밍업 연결 수
    pub warmup_size: usize,
    /// 연결 최대 수명
    pub max_lifetime: std::time::Duration,
    /// 유휴 타임아웃
    pub idle_timeout: std::time::Duration,
    /// 연결 타임아웃
    pub connection_timeout: std::time::Duration,
    /// 검증 타임아웃
    pub validation_timeout: std::time::Duration,
}

/// 드라이버 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverSettings {
    /// 연결 풀 최소 크기
    #[serde(default = "default_pool_min_size")]
    pub pool_min_size: usize,

    /// 연결 풀 최대 크기
    #[serde(default = "default_pool_max_size")]
    pub pool_max_size: usize,

    /// 연결 유휴 타임아웃 (초)
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,

    /// 연결 획득 타임아웃 (밀리초)
    #[serde(default = "default_acquire_timeout_ms")]
    pub acquire_timeout_ms: u64,

    /// 워밍업 활성화
    #[serde(default = "default_warmup_enabled")]
    pub warmup_enabled: bool,

    /// 워밍업 연결 수
    #[serde(default = "default_warmup_connections")]
    pub warmup_connections: usize,
}

fn default_pool_min_size() -> usize {
    1
}
fn default_pool_max_size() -> usize {
    10
}
fn default_idle_timeout_secs() -> u64 {
    300
}
fn default_acquire_timeout_ms() -> u64 {
    5000
}
fn default_warmup_enabled() -> bool {
    true
}
fn default_warmup_connections() -> usize {
    2
}

impl Default for DriverSettings {
    fn default() -> Self {
        Self {
            pool_min_size: default_pool_min_size(),
            pool_max_size: default_pool_max_size(),
            idle_timeout_secs: default_idle_timeout_secs(),
            acquire_timeout_ms: default_acquire_timeout_ms(),
            warmup_enabled: default_warmup_enabled(),
            warmup_connections: default_warmup_connections(),
        }
    }
}

impl DriverSettings {
    /// PoolConfig로 변환
    ///
    /// PoolConfig 구조체로 변환하여 연결 풀 생성에 사용
    pub fn to_pool_config(&self) -> PoolConfig {
        use std::time::Duration;
        PoolConfig {
            max_size: self.pool_max_size,
            min_idle: self.pool_min_size,
            warmup_on_init: self.warmup_enabled,
            warmup_size: self.warmup_connections,
            max_lifetime: Duration::from_secs(3600), // 1시간 (고정)
            idle_timeout: Duration::from_secs(self.idle_timeout_secs),
            connection_timeout: Duration::from_millis(self.acquire_timeout_ms),
            validation_timeout: Duration::from_secs(5), // 5초 (고정)
        }
    }
}

/// CLI 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliSettings {
    /// 기본 출력 형식
    #[serde(default = "default_output_format")]
    pub output_format: String,

    /// 페이지 크기
    #[serde(default = "default_page_size")]
    pub page_size: usize,

    /// 히스토리 파일 경로
    #[serde(default)]
    pub history_file: Option<String>,

    /// 자동 완성 활성화
    #[serde(default = "default_autocomplete")]
    pub autocomplete: bool,
}

fn default_output_format() -> String {
    "table".to_string()
}
fn default_page_size() -> usize {
    100
}
fn default_autocomplete() -> bool {
    true
}

impl Default for CliSettings {
    fn default() -> Self {
        Self {
            output_format: default_output_format(),
            page_size: default_page_size(),
            history_file: None,
            autocomplete: default_autocomplete(),
        }
    }
}

/// 쿼리 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuerySettings {
    /// 최대 결과 크기
    #[serde(default = "default_max_result_size")]
    pub max_result_size: usize,

    /// 쿼리 타임아웃 (밀리초)
    #[serde(default = "default_query_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_max_result_size() -> usize {
    100000
}
fn default_query_timeout_ms() -> u64 {
    30000
}

impl Default for QuerySettings {
    fn default() -> Self {
        Self {
            max_result_size: default_max_result_size(),
            timeout_ms: default_query_timeout_ms(),
        }
    }
}

impl Zeta4jConfig {
    /// 기본 설정 경로
    pub fn default_config_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".zeta4j")
            .join("config.toml")
    }

    /// 설정 로드 (파일 + 환경 변수)
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(Self::default_config_path())
    }

    /// 지정된 경로에서 설정 로드
    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let mut config = if path.as_ref().exists() {
            let content = fs::read_to_string(&path)?;
            toml::from_str(&content).map_err(|e| ConfigError::TomlParse(e.to_string()))?
        } else {
            Self::default()
        };

        // 환경 변수로 오버라이드
        config.apply_env_overrides()?;

        Ok(config)
    }

    /// 설정 저장
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)?;

        // 디렉토리 생성
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(path, content)?;

        Ok(())
    }

    /// 환경 변수 오버라이드 적용
    fn apply_env_overrides(&mut self) -> Result<(), ConfigError> {
        if let Ok(val) = env::var("ZETA4J_STORAGE_MEMORY_LIMIT_BYTES") {
            let parsed: usize = val.parse().map_err(|_| {
                ConfigError::EnvParse(format!(
                    "Invalid ZETA4J_STORAGE_MEMORY_LIMIT_BYTES: {}",
                    val
                ))
            })?;
            if parsed == 0 {
                return Err(ConfigError::EnvParse(
                    "ZETA4J_STORAGE_MEMORY_LIMIT_BYTES must be positive".to_string(),
                ));
            }
            self.storage.memory_limit_bytes = Some(parsed);
        }

        // Transaction 설정
        if let Ok(val) = env::var("ZETA4J_TRANSACTION_TIMEOUT_MS") {
            self.transaction.timeout_ms = val.parse().map_err(|_| {
                ConfigError::EnvParse(format!("Invalid ZETA4J_TRANSACTION_TIMEOUT_MS: {}", val))
            })?;
        }
        if let Ok(val) = env::var("ZETA4J_TRANSACTION_LOCK_WAIT_MS") {
            self.transaction.lock_wait_ms = val.parse().map_err(|_| {
                ConfigError::EnvParse(format!("Invalid ZETA4J_TRANSACTION_LOCK_WAIT_MS: {}", val))
            })?;
        }

        // WAL 설정
        if let Ok(val) = env::var("ZETA4J_WAL_MAX_SEGMENT_SIZE") {
            self.wal.max_segment_size = val.parse().map_err(|_| {
                ConfigError::EnvParse(format!("Invalid ZETA4J_WAL_MAX_SEGMENT_SIZE: {}", val))
            })?;
        }
        if let Ok(val) = env::var("ZETA4J_WAL_MAX_SEGMENTS") {
            self.wal.max_segments = val.parse().map_err(|_| {
                ConfigError::EnvParse(format!("Invalid ZETA4J_WAL_MAX_SEGMENTS: {}", val))
            })?;
        }
        if let Ok(val) = env::var("ZETA4J_WAL_FLUSH_INTERVAL") {
            self.wal.flush_interval = val.parse().map_err(|_| {
                ConfigError::EnvParse(format!("Invalid ZETA4J_WAL_FLUSH_INTERVAL: {}", val))
            })?;
        }

        // Compaction 설정
        if let Ok(val) = env::var("ZETA4J_COMPACTION_MIN_RECORDS") {
            self.compaction.min_records = val.parse().map_err(|_| {
                ConfigError::EnvParse(format!("Invalid ZETA4J_COMPACTION_MIN_RECORDS: {}", val))
            })?;
        }

        // MVCC 설정
        if let Ok(val) = env::var("ZETA4J_MVCC_MAX_WRITE_SETS") {
            self.mvcc.max_write_sets = val.parse().map_err(|_| {
                ConfigError::EnvParse(format!("Invalid ZETA4J_MVCC_MAX_WRITE_SETS: {}", val))
            })?;
        }

        Ok(())
    }

    /// 기본 설정 파일 생성
    pub fn create_default_config() -> Result<PathBuf, ConfigError> {
        let path = Self::default_config_path();
        let config = Self::default();
        config.save(&path)?;
        Ok(path)
    }
}

#[cfg(test)]
mod memory_limit_tests {
    use super::*;

    #[test]
    fn memory_limit_is_opt_in_and_positive() -> Result<(), Box<dyn std::error::Error>> {
        let absent: Zeta4jConfig = toml::from_str("[storage]\ndata_dir = \"data\"\n")?;
        assert_eq!(absent.storage.memory_limit_bytes, None);

        let configured: Zeta4jConfig =
            toml::from_str("[storage]\ndata_dir = \"data\"\nmemory_limit_bytes = 4096\n")?;
        assert_eq!(configured.storage.memory_limit_bytes, Some(4096));

        assert!(
            toml::from_str::<Zeta4jConfig>(
                "[storage]\ndata_dir = \"data\"\nmemory_limit_bytes = 0\n"
            )
            .is_err()
        );
        assert!(
            toml::from_str::<Zeta4jConfig>(
                "[storage]\ndata_dir = \"data\"\nmemory_limit_bytes = \"invalid\"\n"
            )
            .is_err()
        );
        Ok(())
    }
}
