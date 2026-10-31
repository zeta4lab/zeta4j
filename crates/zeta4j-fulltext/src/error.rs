// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 전문 검색 에러 타입

use thiserror::Error;
use zeta4j_common::{ZetaError, codes};

/// 전문 검색 결과 타입
pub type Result<T> = std::result::Result<T, FulltextError>;

/// 전문 검색 에러
#[derive(Error, Debug)]
pub enum FulltextError {
    /// Tantivy 에러
    #[error("Tantivy error: {0}")]
    Tantivy(#[from] tantivy::TantivyError),

    /// 쿼리 파싱 에러
    #[error("Query parse error: {0}")]
    QueryParse(#[from] tantivy::query::QueryParserError),

    /// 디렉토리 에러
    #[error("Directory error: {0}")]
    Directory(#[from] tantivy::directory::error::OpenDirectoryError),

    /// IO 에러
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// 인덱스를 찾을 수 없음
    #[error("Index not found: {0}")]
    IndexNotFound(String),

    /// 필드를 찾을 수 없음
    #[error("Field not found: {0}")]
    FieldNotFound(String),

    /// 잘못된 설정
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// 스키마 에러
    #[error("Schema error: {0}")]
    Schema(String),

    /// 직렬화 에러
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// 토크나이저 에러
    #[error("Tokenizer error: {0}")]
    Tokenizer(String),

    /// 인덱스 손상
    #[error("Index corrupted: {0}")]
    IndexCorrupted(String),

    /// 복구 실패
    #[error("Recovery failed: {0}")]
    RecoveryFailed(String),

    /// 기타 에러
    #[error("{0}")]
    Other(String),
}

// =============================================================================
// From<FulltextError> for ZetaError
// =============================================================================

impl From<FulltextError> for ZetaError {
    fn from(err: FulltextError) -> Self {
        match err {
            FulltextError::Tantivy(e) => ZetaError::Fulltext {
                code: codes::fulltext::TANTIVY_ERROR,
                message: e.to_string(),
            },
            FulltextError::QueryParse(e) => ZetaError::Fulltext {
                code: codes::fulltext::QUERY_PARSE_FAILED,
                message: e.to_string(),
            },
            FulltextError::Directory(e) => ZetaError::Fulltext {
                code: codes::fulltext::TANTIVY_ERROR,
                message: e.to_string(),
            },
            FulltextError::Io(e) => ZetaError::StorageIo {
                code: codes::graph::STORAGE_IO,
                message: e.to_string(),
            },
            FulltextError::IndexNotFound(name) => ZetaError::Fulltext {
                code: codes::fulltext::INDEX_NOT_FOUND,
                message: format!("Index not found: {}", name),
            },
            FulltextError::FieldNotFound(name) => ZetaError::Fulltext {
                code: codes::fulltext::FIELD_NOT_FOUND,
                message: format!("Field not found: {}", name),
            },
            FulltextError::InvalidConfig(msg) => ZetaError::Fulltext {
                code: codes::fulltext::INVALID_CONFIG,
                message: msg,
            },
            FulltextError::Schema(msg) => ZetaError::Fulltext {
                code: codes::fulltext::SCHEMA_ERROR,
                message: msg,
            },
            FulltextError::Serialization(e) => ZetaError::Serialization {
                code: codes::graph::SERIALIZATION_FAILED,
                message: e.to_string(),
            },
            FulltextError::Tokenizer(msg) => ZetaError::Fulltext {
                code: codes::fulltext::TOKENIZER_ERROR,
                message: msg,
            },
            FulltextError::IndexCorrupted(msg) => ZetaError::Fulltext {
                code: codes::fulltext::INDEX_CORRUPTED,
                message: msg,
            },
            FulltextError::RecoveryFailed(msg) => ZetaError::Fulltext {
                code: codes::fulltext::RECOVERY_FAILED,
                message: msg,
            },
            FulltextError::Other(msg) => ZetaError::Fulltext {
                code: codes::fulltext::TANTIVY_ERROR,
                message: msg,
            },
        }
    }
}
