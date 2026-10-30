//! 벡터 인덱스 모듈
//!
//! 저장소가 소유한 canonical vector record로부터 immutable HNSW generation을 만들고
//! ANN (Approximate Nearest Neighbor) 검색을 제공합니다.
//!
//! # 개요
//!
//! - `HnswGeneration`: immutable HNSW 검색 snapshot
//! - `DistanceFunction`: 거리 함수 (Cosine, Euclidean, DotProduct)
//! - `HnswGenerationParams`: generation build/search 파라미터
//!
//! # 예제
//!
//! ```ignore
//! use zeta4j_vector::index::{DistanceFunction, HnswGeneration, HnswGenerationParams};
//!
//! let generation = HnswGeneration::build(
//!     vec![(1, vec![0.1; 128]), (2, vec![0.2; 128])],
//!     HnswGenerationParams::new(128, DistanceFunction::Cosine),
//! )?;
//! let results = generation.search(&vec![0.15; 128], 10, None)?;
//!
//! // 결과 출력
//! for result in &results {
//!     println!("ID: {}, Distance: {}", result.id, result.distance);
//! }
//! ```

mod distance;
mod hnsw_generation;
mod types;

pub use distance::{
    DistanceFunction, cosine_distance, cosine_similarity, dot_product, euclidean_distance,
    euclidean_distance_squared, manhattan_distance,
};

pub use hnsw_generation::{HnswGeneration, HnswGenerationNode, HnswGenerationParams};
pub use types::{HNSWError, SearchResult};
