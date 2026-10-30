//! 대규모 그래프 스트리밍 알고리즘
//!
//! 메모리 효율적인 청크 기반 그래프 알고리즘을 제공합니다.
//!
//! # 주요 기능
//!
//! - 청크 단위 처리: 대규모 그래프를 메모리에 전부 로드하지 않고 처리
//! - 진행 상황 콜백: 장시간 실행 알고리즘의 진행률 모니터링
//! - Iterator 기반 결과: 결과를 한 번에 수집하지 않고 스트리밍
//!
//! # 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::algorithm::streaming::*;
//!
//! // 청크 기반 PageRank
//! let config = StreamingConfig::default()
//!     .with_chunk_size(10000)
//!     .with_progress_callback(|progress| {
//!         println!("Progress: {:.1}%", progress * 100.0);
//!     });
//!
//! let mut streaming_pr = StreamingPageRank::new(&engine, config);
//! for (node_id, score) in streaming_pr.compute()? {
//!     if score > 0.01 {
//!         println!("Node {} has high PageRank: {}", node_id, score);
//!     }
//! }
//! ```

mod centrality;
mod chunked_pagerank;
mod config;
mod path;

pub use centrality::{
    BetweennessScoreIterator, CentralityScore, PageRankScoreIterator, StreamingBetweenness,
    StreamingBetweennessConfig, StreamingPageRank, StreamingPageRankConfig,
};
pub use chunked_pagerank::{ChunkedPageRank, ChunkedPageRankConfig, ChunkedPageRankResult};
pub use config::{
    AlgorithmPhase, ChunkProgress, MemoryFallback, MemoryLimitExceeded, ProgressCallback,
    StreamingConfig, StreamingStats,
};
pub use path::{
    ChunkedDistanceResult, ChunkedFloydWarshall, ChunkedFloydWarshallConfig, DistancePair,
    PartialPathIterator,
};
