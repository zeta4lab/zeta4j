//! 벡터 인덱스 설정 모듈
//!
//! 벡터 인덱스(HNSW)의 파라미터 및 검색 설정을 관리합니다.
//!
//! # 예제 설정 파일
//!
//! ```toml
//! [vector]
//! dimensions = 1536
//! distance = "cosine"
//!
//! [vector.hnsw]
//! m = 16
//! m_max0 = 32
//! ef_construction = 200
//! ef_search = 50
//!
//! [vector.search]
//! default_limit = 10
//! max_limit = 1000
//! ```

use serde::{Deserialize, Serialize};
use zeta4j_common::defaults::*;

/// 벡터 인덱스 전역 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorConfig {
    /// 기본 차원 수
    #[serde(default = "default_dimensions")]
    pub dimensions: u32,

    /// 거리 함수 (cosine, euclidean, dot_product)
    #[serde(default = "default_distance")]
    pub distance: String,

    /// HNSW 인덱스 파라미터
    #[serde(default)]
    pub hnsw: HnswConfig,

    /// 검색 설정
    #[serde(default)]
    pub search: SearchConfig,

    /// 저장소 설정
    #[serde(default)]
    pub storage: VectorStorageConfig,
}

fn default_dimensions() -> u32 {
    DEFAULT_VECTOR_DIMENSIONS as u32
}

fn default_distance() -> String {
    DEFAULT_VECTOR_DISTANCE.to_string()
}

impl Default for VectorConfig {
    fn default() -> Self {
        Self {
            dimensions: DEFAULT_VECTOR_DIMENSIONS as u32,
            distance: DEFAULT_VECTOR_DISTANCE.to_string(),
            hnsw: HnswConfig::default(),
            search: SearchConfig::default(),
            storage: VectorStorageConfig::default(),
        }
    }
}

impl VectorConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 차원 수 설정
    pub fn with_dimensions(mut self, dimensions: u32) -> Self {
        self.dimensions = dimensions;
        self
    }

    /// 거리 함수 설정
    pub fn with_distance(mut self, distance: impl Into<String>) -> Self {
        self.distance = distance.into();
        self
    }

    /// HNSW 파라미터 설정
    pub fn with_hnsw(mut self, hnsw: HnswConfig) -> Self {
        self.hnsw = hnsw;
        self
    }

    /// 검색 설정
    pub fn with_search(mut self, search: SearchConfig) -> Self {
        self.search = search;
        self
    }
}

/// HNSW 인덱스 파라미터
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HnswConfig {
    /// 각 노드의 최대 이웃 수 (기본: 16)
    #[serde(default = "default_m")]
    pub m: usize,

    /// Layer 0의 최대 이웃 수 (기본: 2×M = 32)
    #[serde(default = "default_m_max0")]
    pub m_max0: usize,

    /// 삽입 시 탐색 크기 (기본: 200)
    #[serde(default = "default_ef_construction")]
    pub ef_construction: usize,

    /// 기본 검색 시 탐색 크기 (기본: 50)
    #[serde(default = "default_ef_search")]
    pub ef_search: usize,

    /// 벡터 페이지 시작 오프셋 (FIRST_HEAP_PAGE_ID 기준)
    ///
    /// 대형 벡터 저장을 위한 페이지 영역 시작 위치입니다.
    /// 값이 클수록 더 많은 HNSW 노드를 저장할 수 있습니다.
    /// 기본값: 10,000 (약 1백만 노드 지원)
    #[serde(default = "default_vector_page_offset")]
    pub vector_page_offset: u32,
}

fn default_m() -> usize {
    DEFAULT_HNSW_M
}
fn default_m_max0() -> usize {
    DEFAULT_HNSW_M_MAX0
}
fn default_ef_construction() -> usize {
    DEFAULT_HNSW_EF_CONSTRUCTION
}
fn default_ef_search() -> usize {
    DEFAULT_HNSW_EF_SEARCH
}
fn default_vector_page_offset() -> u32 {
    DEFAULT_VECTOR_PAGE_OFFSET as u32
}

impl Default for HnswConfig {
    fn default() -> Self {
        Self {
            m: DEFAULT_HNSW_M,
            m_max0: DEFAULT_HNSW_M_MAX0,
            ef_construction: DEFAULT_HNSW_EF_CONSTRUCTION,
            ef_search: DEFAULT_HNSW_EF_SEARCH,
            vector_page_offset: DEFAULT_VECTOR_PAGE_OFFSET as u32,
        }
    }
}

impl HnswConfig {
    /// 빠른 빌드 프리셋 (정확도 감소, 속도 향상)
    pub fn fast_build() -> Self {
        Self {
            m: 12,
            m_max0: 24,
            ef_construction: 100,
            ef_search: 30,
            vector_page_offset: DEFAULT_VECTOR_PAGE_OFFSET as u32,
        }
    }

    /// 고정밀 프리셋 (정확도 향상, 속도 감소)
    pub fn high_accuracy() -> Self {
        Self {
            m: 24,
            m_max0: 48,
            ef_construction: 400,
            ef_search: 100,
            vector_page_offset: DEFAULT_VECTOR_PAGE_OFFSET as u32,
        }
    }

    /// 메모리 효율 프리셋
    pub fn memory_efficient() -> Self {
        Self {
            m: 8,
            m_max0: 16,
            ef_construction: 150,
            ef_search: 40,
            vector_page_offset: DEFAULT_VECTOR_PAGE_OFFSET as u32,
        }
    }
}

/// 검색 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchConfig {
    /// 기본 결과 수
    #[serde(default = "default_limit")]
    pub default_limit: usize,

    /// 최대 결과 수
    #[serde(default = "default_max_limit")]
    pub max_limit: usize,

    /// 최소 유사도 (0.0 ~ 1.0, cosine distance 기준)
    #[serde(default)]
    pub min_score: Option<f32>,

    /// 검색 타임아웃 (밀리초)
    #[serde(default = "default_search_timeout")]
    pub timeout_ms: u64,

    /// 병렬 검색 활성화
    #[serde(default = "default_true")]
    pub parallel: bool,
}

fn default_limit() -> usize {
    DEFAULT_VECTOR_SEARCH_LIMIT
}
fn default_max_limit() -> usize {
    DEFAULT_VECTOR_SEARCH_MAX_LIMIT
}
fn default_search_timeout() -> u64 {
    DEFAULT_VECTOR_SEARCH_TIMEOUT_MS
}
fn default_true() -> bool {
    true
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: DEFAULT_VECTOR_SEARCH_LIMIT,
            max_limit: DEFAULT_VECTOR_SEARCH_MAX_LIMIT,
            min_score: None,
            timeout_ms: DEFAULT_VECTOR_SEARCH_TIMEOUT_MS,
            parallel: true,
        }
    }
}

impl SearchConfig {
    /// 결과 수 제한 설정
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.default_limit = limit;
        self
    }

    /// 최대 결과 수 설정
    pub fn with_max_limit(mut self, max_limit: usize) -> Self {
        self.max_limit = max_limit;
        self
    }

    /// 최소 유사도 설정
    pub fn with_min_score(mut self, min_score: f32) -> Self {
        self.min_score = Some(min_score);
        self
    }
}

/// 벡터 저장소 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorStorageConfig {
    /// 인라인 저장 최대 차원 (기본: 64)
    /// 이 차원 이하는 노드 프로퍼티에 직접 저장
    #[serde(default = "default_inline_threshold")]
    pub inline_threshold: usize,

    /// 디스크 저장소 활성화
    #[serde(default = "default_true")]
    pub disk_enabled: bool,

    /// 메모리 캐시 크기 (항목 수)
    #[serde(default = "default_cache_size")]
    pub cache_size: usize,

    /// mmap 사용 여부
    #[serde(default = "default_true")]
    pub use_mmap: bool,
}

fn default_inline_threshold() -> usize {
    DEFAULT_VECTOR_INLINE_THRESHOLD
}
fn default_cache_size() -> usize {
    DEFAULT_VECTOR_CACHE_SIZE
}

impl Default for VectorStorageConfig {
    fn default() -> Self {
        Self {
            inline_threshold: DEFAULT_VECTOR_INLINE_THRESHOLD,
            disk_enabled: true,
            cache_size: DEFAULT_VECTOR_CACHE_SIZE,
            use_mmap: true,
        }
    }
}
