// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 페이지네이션 설정 모듈

/// 페이지네이션 설정
///
/// 커서 및 페이지네이션 동작을 제어하는 설정입니다.
#[derive(Debug, Clone)]
pub struct PaginationConfig {
    /// 기본 페이지 크기
    pub default_page_size: usize,
    /// 최대 페이지 크기
    pub max_page_size: usize,
    /// 커서 TTL (초)
    pub cursor_ttl_seconds: u64,
    /// 최대 커서 수
    pub max_cursors: usize,
    /// 총 개수 계산 여부
    pub compute_total_count: bool,
    /// 빈 페이지 허용 여부
    pub allow_empty_pages: bool,
    /// 커서 자동 정리 간격 (초)
    pub cleanup_interval_seconds: u64,
}

impl PaginationConfig {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 커스텀 설정 생성
    pub fn custom(
        default_page_size: usize,
        max_page_size: usize,
        cursor_ttl_seconds: u64,
        max_cursors: usize,
    ) -> Self {
        Self {
            default_page_size,
            max_page_size,
            cursor_ttl_seconds,
            max_cursors,
            ..Default::default()
        }
    }

    /// 대용량 처리용 설정
    pub fn for_large_datasets() -> Self {
        Self {
            default_page_size: 100,
            max_page_size: 10000,
            cursor_ttl_seconds: 600,
            max_cursors: 1000,
            compute_total_count: false,
            allow_empty_pages: false,
            cleanup_interval_seconds: 60,
        }
    }

    /// 페이지 크기 검증 및 조정
    pub fn validate_page_size(&self, requested: usize) -> usize {
        if requested == 0 {
            self.default_page_size
        } else {
            requested.min(self.max_page_size)
        }
    }

    /// 기본 페이지 크기 설정
    pub fn with_default_page_size(mut self, size: usize) -> Self {
        self.default_page_size = size;
        self
    }

    /// 최대 페이지 크기 설정
    pub fn with_max_page_size(mut self, size: usize) -> Self {
        self.max_page_size = size;
        self
    }

    /// 커서 TTL 설정
    pub fn with_cursor_ttl(mut self, seconds: u64) -> Self {
        self.cursor_ttl_seconds = seconds;
        self
    }

    /// 최대 커서 수 설정
    pub fn with_max_cursors(mut self, count: usize) -> Self {
        self.max_cursors = count;
        self
    }

    /// 총 개수 계산 여부 설정
    pub fn with_compute_total_count(mut self, compute: bool) -> Self {
        self.compute_total_count = compute;
        self
    }

    /// 설정 유효성 검사
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.default_page_size == 0 {
            return Err(ConfigError::InvalidPageSize(
                "default_page_size must be > 0".to_string(),
            ));
        }

        if self.max_page_size == 0 {
            return Err(ConfigError::InvalidPageSize(
                "max_page_size must be > 0".to_string(),
            ));
        }

        if self.default_page_size > self.max_page_size {
            return Err(ConfigError::InvalidPageSize(
                "default_page_size cannot exceed max_page_size".to_string(),
            ));
        }

        if self.max_cursors == 0 {
            return Err(ConfigError::InvalidCursorLimit(
                "max_cursors must be > 0".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for PaginationConfig {
    fn default() -> Self {
        Self {
            default_page_size: 20,
            max_page_size: 1000,
            cursor_ttl_seconds: 300, // 5분
            max_cursors: 100,
            compute_total_count: false,
            allow_empty_pages: true,
            cleanup_interval_seconds: 60,
        }
    }
}

/// 설정 오류
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// 잘못된 페이지 크기
    InvalidPageSize(String),
    /// 잘못된 커서 제한
    InvalidCursorLimit(String),
    /// 잘못된 TTL
    InvalidTtl(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::InvalidPageSize(msg) => write!(f, "Invalid page size: {}", msg),
            ConfigError::InvalidCursorLimit(msg) => write!(f, "Invalid cursor limit: {}", msg),
            ConfigError::InvalidTtl(msg) => write!(f, "Invalid TTL: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}

// =============================================================================
// From<ConfigError> for ZetaError
// =============================================================================

impl From<ConfigError> for zeta4j_common::ZetaError {
    fn from(err: ConfigError) -> Self {
        use zeta4j_common::codes;

        match err {
            ConfigError::InvalidPageSize(msg) => zeta4j_common::ZetaError::InvalidConfig {
                code: codes::cypher::INVALID_PAGE_SIZE,
                key: "page_size".to_string(),
                value: msg,
            },
            ConfigError::InvalidCursorLimit(msg) => zeta4j_common::ZetaError::InvalidConfig {
                code: codes::cypher::INVALID_CURSOR_LIMIT,
                key: "cursor_limit".to_string(),
                value: msg,
            },
            ConfigError::InvalidTtl(msg) => zeta4j_common::ZetaError::InvalidConfig {
                code: codes::cypher::INVALID_TTL,
                key: "ttl".to_string(),
                value: msg,
            },
        }
    }
}
