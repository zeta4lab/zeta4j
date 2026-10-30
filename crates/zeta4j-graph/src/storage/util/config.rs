//! 스토리지 설정
//!
//! DiskNodeStorage 및 관련 컴포넌트의 설정을 관리합니다.

use super::compression::CompressionType;

/// WAL 동기화 모드
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WalSyncMode {
    /// 동기화 없음 (가장 빠르지만 데이터 손실 위험)
    None,
    /// 각 쓰기 후 fsync 호출
    #[default]
    FSync,
    /// fdatasync 사용 (메타데이터는 동기화하지 않음)
    FDataSync,
}

/// 스토리지 설정
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// 버퍼 풀 크기 (페이지 수)
    pub buffer_pool_size: usize,

    /// 페이지 크기 (바이트)
    pub page_size: usize,

    /// WAL 활성화 여부
    pub enable_wal: bool,

    /// WAL 동기화 모드
    pub wal_sync_mode: WalSyncMode,

    /// 체크포인트 간격 (초)
    pub checkpoint_interval_secs: u64,

    /// 캐시 히트율 목표 (0.0 ~ 1.0)
    pub target_cache_hit_ratio: f64,

    /// 압축 활성화 여부
    pub enable_compression: bool,

    /// 압축 타입 (LZ4, LZ4HC 등)
    pub compression_type: CompressionType,

    /// 압축 최소 크기 (이보다 작은 데이터는 압축하지 않음)
    pub compression_min_size: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            buffer_pool_size: 10000, // 40MB 기본값 (10000 * 4KB)
            page_size: 4096,         // 4KB
            enable_wal: true,
            wal_sync_mode: WalSyncMode::FSync,
            checkpoint_interval_secs: 60,
            target_cache_hit_ratio: 0.9,
            enable_compression: false, // 기본값: 압축 비활성화 (호환성)
            compression_type: CompressionType::Lz4,
            compression_min_size: 64, // 64바이트 미만은 압축하지 않음
        }
    }
}

impl StorageConfig {
    /// 새 설정 빌더 생성
    pub fn builder() -> StorageConfigBuilder {
        StorageConfigBuilder::default()
    }

    /// 프로덕션용 권장 설정
    pub fn for_production() -> Self {
        Self {
            buffer_pool_size: 10000, // 40MB
            page_size: 4096,
            enable_wal: true,
            wal_sync_mode: WalSyncMode::FSync,
            checkpoint_interval_secs: 60,
            target_cache_hit_ratio: 0.95,
            enable_compression: true, // 프로덕션은 압축 활성화
            compression_type: CompressionType::Lz4,
            compression_min_size: 64,
        }
    }

    /// 메모리 집약적 워크로드용 설정
    pub fn for_memory_intensive() -> Self {
        Self {
            buffer_pool_size: 50000, // 200MB
            page_size: 4096,
            enable_wal: true,
            wal_sync_mode: WalSyncMode::FDataSync,
            checkpoint_interval_secs: 120,
            target_cache_hit_ratio: 0.98,
            enable_compression: true,
            compression_type: CompressionType::Lz4,
            compression_min_size: 64,
        }
    }

    /// 고압축 스토리지 설정 (저장공간 최적화)
    pub fn for_high_compression() -> Self {
        Self {
            buffer_pool_size: 10000,
            page_size: 4096,
            enable_wal: true,
            wal_sync_mode: WalSyncMode::FSync,
            checkpoint_interval_secs: 60,
            target_cache_hit_ratio: 0.95,
            enable_compression: true,
            compression_type: CompressionType::Lz4Hc, // 고압축 모드
            compression_min_size: 32,
        }
    }

    /// 버퍼 풀 총 메모리 사용량 (바이트)
    pub fn buffer_pool_memory_bytes(&self) -> usize {
        self.buffer_pool_size * self.page_size
    }
}

/// StorageConfig 빌더
#[derive(Debug, Default)]
pub struct StorageConfigBuilder {
    buffer_pool_size: Option<usize>,
    page_size: Option<usize>,
    enable_wal: Option<bool>,
    wal_sync_mode: Option<WalSyncMode>,
    checkpoint_interval_secs: Option<u64>,
    target_cache_hit_ratio: Option<f64>,
    enable_compression: Option<bool>,
    compression_type: Option<CompressionType>,
    compression_min_size: Option<usize>,
}

impl StorageConfigBuilder {
    /// 버퍼 풀 크기 (페이지 수)
    pub fn buffer_pool_size(mut self, size: usize) -> Self {
        self.buffer_pool_size = Some(size);
        self
    }

    /// 버퍼 풀 크기 (MB 단위)
    pub fn buffer_pool_mb(mut self, mb: usize) -> Self {
        // 4KB 페이지 기준: 1MB = 256 페이지
        self.buffer_pool_size = Some(mb * 256);
        self
    }

    /// 버퍼 풀 크기 (GB 단위)
    pub fn buffer_pool_gb(mut self, gb: usize) -> Self {
        // 4KB 페이지 기준: 1GB = 262144 페이지
        self.buffer_pool_size = Some(gb * 262144);
        self
    }

    pub fn page_size(mut self, size: usize) -> Self {
        self.page_size = Some(size);
        self
    }

    pub fn enable_wal(mut self, enable: bool) -> Self {
        self.enable_wal = Some(enable);
        self
    }

    pub fn wal_sync_mode(mut self, mode: WalSyncMode) -> Self {
        self.wal_sync_mode = Some(mode);
        self
    }

    pub fn checkpoint_interval_secs(mut self, secs: u64) -> Self {
        self.checkpoint_interval_secs = Some(secs);
        self
    }

    pub fn target_cache_hit_ratio(mut self, ratio: f64) -> Self {
        self.target_cache_hit_ratio = Some(ratio);
        self
    }

    pub fn enable_compression(mut self, enable: bool) -> Self {
        self.enable_compression = Some(enable);
        self
    }

    pub fn compression_type(mut self, compression: CompressionType) -> Self {
        self.compression_type = Some(compression);
        self
    }

    pub fn compression_min_size(mut self, size: usize) -> Self {
        self.compression_min_size = Some(size);
        self
    }

    pub fn build(self) -> StorageConfig {
        let default = StorageConfig::default();
        StorageConfig {
            buffer_pool_size: self.buffer_pool_size.unwrap_or(default.buffer_pool_size),
            page_size: self.page_size.unwrap_or(default.page_size),
            enable_wal: self.enable_wal.unwrap_or(default.enable_wal),
            wal_sync_mode: self.wal_sync_mode.unwrap_or(default.wal_sync_mode),
            checkpoint_interval_secs: self
                .checkpoint_interval_secs
                .unwrap_or(default.checkpoint_interval_secs),
            target_cache_hit_ratio: self
                .target_cache_hit_ratio
                .unwrap_or(default.target_cache_hit_ratio),
            enable_compression: self
                .enable_compression
                .unwrap_or(default.enable_compression),
            compression_type: self.compression_type.unwrap_or(default.compression_type),
            compression_min_size: self
                .compression_min_size
                .unwrap_or(default.compression_min_size),
        }
    }
}
