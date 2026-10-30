//! Storage-neutral vector index types.

use thiserror::Error;
use zeta4j_common::{ZetaError, codes};

#[derive(Debug, Error)]
pub enum HNSWError {
    #[error("Dimension mismatch: expected {expected}, got {got}")]
    DimensionMismatch { expected: u32, got: u32 },
    #[error("Vector not found: {0}")]
    VectorNotFound(u64),
    #[error("Invalid parameters: {0}")]
    InvalidParams(String),
}

impl From<HNSWError> for ZetaError {
    fn from(error: HNSWError) -> Self {
        match error {
            HNSWError::DimensionMismatch { expected, got } => ZetaError::DimensionMismatch {
                code: codes::vector::DIMENSION_MISMATCH,
                expected: expected as usize,
                found: got as usize,
            },
            HNSWError::VectorNotFound(id) => ZetaError::VectorIndex {
                code: codes::vector::VECTOR_NOT_FOUND,
                message: format!("Vector not found: {id}"),
            },
            HNSWError::InvalidParams(message) => ZetaError::VectorIndex {
                code: codes::vector::HNSW_BUILD_FAILED,
                message,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub id: u64,
    pub distance: f32,
}

impl SearchResult {
    pub const fn new(id: u64, distance: f32) -> Self {
        Self { id, distance }
    }
}
