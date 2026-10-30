//! 병렬 그래프 알고리즘
//!
//! Rayon을 사용하여 그래프 알고리즘의 병렬 처리를 지원합니다.
//! CPU 코어를 활용하여 대규모 그래프에서 알고리즘 성능을 향상시킵니다.
//!
//! # 지원 알고리즘
//!
//! ## 중심성 (Centrality)
//! - **Betweenness Centrality**: 노드별 BFS 병렬 실행
//! - **PageRank**: 노드별 점수 계산 병렬화
//! - **Closeness/Harmonic Centrality**: 노드별 BFS 병렬 실행
//!
//! ## 커뮤니티 탐지 (Community Detection)
//! - **Louvain**: 로컬 이동 최적화 병렬화
//! - **Connected Components**: Union-Find 병렬 처리
//!
//! ## 메트릭 (Metrics)
//! - **Triangle Count**: 노드별 삼각형 계수 병렬화
//!
//! # 예제
//!
//! ```ignore
//! use zeta4j_graph::algorithm::parallel::{ParallelConfig, betweenness_parallel};
//! use zeta4j_graph::algorithm::BetweennessConfig;
//!
//! let config = BetweennessConfig::default();
//! let par_config = ParallelConfig::default();
//! let result = betweenness_parallel(&engine, &config, &par_config, None)?;
//! ```

pub mod betweenness;
pub mod closeness;
pub mod config;
pub mod connected;
pub mod louvain;
pub mod pagerank;
pub mod triangle;

pub use betweenness::{ParallelBetweenness, betweenness_parallel};
pub use closeness::{ParallelCloseness, ParallelHarmonic, closeness_parallel, harmonic_parallel};
pub use config::ParallelConfig;
pub use connected::{
    ParallelConnectedComponents, ParallelConnectedResult, connected_components_parallel,
};
pub use louvain::{ParallelLouvain, louvain_parallel};
pub use pagerank::{ParallelPageRank, pagerank_parallel};
pub use triangle::{ParallelTriangleCount, ParallelTriangleResult, triangle_count_parallel};
