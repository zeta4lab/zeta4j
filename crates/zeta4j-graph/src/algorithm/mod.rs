#![allow(dead_code, unused_variables)]
//! 그래프 알고리즘 엔진
//!
//! 경로 탐색(BFS, Dijkstra, Bellman-Ford, A*, Floyd-Warshall), 중심성 분석, 커뮤니티 탐지,
//! 유사도 분석, 링크 예측, 그래프 메트릭, 구조 분석, K-Core, Random Walk, MST,
//! Topological Sort, Maximum Flow, Bipartite Matching, SCC, Graph Coloring,
//! Eulerian Path/Circuit, Hamiltonian Path/Circuit, Clique Detection,
//! Vertex Cover, Independent Set, Dominating Set, Feedback Set, Graph Isomorphism,
//! Steiner Tree, Tree Decomposition 등의 그래프 알고리즘을 제공합니다.
//!
//! # petgraph 호환 레이어 (v1.76.0)
//!
//! `petgraph_compat` 모듈은 petgraph 라이브러리와의 양방향 변환을 지원합니다:
//!
//! - `GraphEngine` → `petgraph::Graph` 변환
//! - `petgraph::Graph` → `GraphEngine` 변환
//! - petgraph 알고리즘 래퍼 함수
//!
//! ```rust,ignore
//! use zeta4j_graph::engine::GraphEngine;
//! use zeta4j_graph::algorithm::{ToPetgraph, FromPetgraph, petgraph_algorithms};
//!
//! let engine = GraphEngine::new();
//! // ... 노드/엣지 추가
//!
//! // petgraph로 변환
//! let pg = engine.to_petgraph()?;
//!
//! // petgraph 알고리즘 사용
//! let distances = petgraph_algorithms::dijkstra_petgraph(&engine, 1, "weight")?;
//! ```
//!
//! # 대규모 그래프 스트리밍 (v1.44.0)
//!
//! `streaming` 모듈은 대규모 그래프에서 메모리 효율적인 알고리즘 실행을 지원합니다:
//!
//! - 청크 기반 처리: 한 번에 일부 노드만 메모리에 로드
//! - 진행 상황 콜백: 장시간 실행 알고리즘 모니터링
//! - Iterator 기반 결과: 결과를 한 번에 수집하지 않고 스트리밍
//!
//! ```rust,ignore
//! use zeta4j_graph::algorithm::streaming::*;
//!
//! // 스트리밍 PageRank
//! let config = StreamingConfig::default()
//!     .with_chunk_size(10000)
//!     .with_progress_callback(|p, phase, _| {
//!         println!("{}: {:.1}%", phase, p * 100.0);
//!     });
//!
//! let pr_config = StreamingPageRankConfig::default()
//!     .with_top_n(100);
//!
//! let streaming_pr = StreamingPageRank::with_config(&engine, pr_config, config);
//! for score in streaming_pr.compute()? {
//!     println!("Node {}: {:.6}", score.node_id, score.score);
//! }
//! ```

pub mod dfs_context;
pub mod types;

pub mod centrality;
pub mod clique;
pub mod coloring;
pub mod community;
pub mod dominating_set;
pub mod eulerian;
pub mod feedback;
pub mod flow;
pub mod hamiltonian;
pub mod independent_set;
pub mod isomorphism;
pub mod kcore;
pub mod link_prediction;
pub mod matching;
pub mod metrics;
pub mod mst;
pub mod parallel;
pub mod path;
pub mod petgraph_compat;
pub mod scc;
pub mod similarity;
pub mod steiner;
pub mod streaming;
pub mod structure;
pub mod topological;
pub mod tree_decomposition;
pub mod vertex_cover;
pub mod walk;

#[cfg(any(feature = "gpu-cuda", feature = "gpu-metal"))]
pub mod gpu;

// Re-exports - Types & Contexts
pub use dfs_context::{DfsContext, SccContext};
pub use types::{
    AdjacencyList, ColorMap, ComponentMap, CoreNumberMap, DeltaMap, DepthMap, DiscoveryTimeMap,
    DistanceMap, LowLinkMap, NodeScoreMap, ParentMap, PredecessorMap, SigmaMap,
    WeightedAdjacencyList,
};

// Re-exports - Path
pub use path::{AStar, AStarConfig, AStarResult, Heuristic};
pub use path::{BellmanFord, BellmanFordConfig, BellmanFordResult};
pub use path::{BfsShortestPath, ShortestPathResult};
pub use path::{Dijkstra, DijkstraConfig, DijkstraResult};
pub use path::{FloydWarshall, FloydWarshallConfig, FloydWarshallResult};
pub use path::{
    all_pairs_distances, all_pairs_distances_weighted, floyd_warshall, floyd_warshall_weighted,
};
pub use path::{astar, astar_euclidean, astar_manhattan, astar_weighted};
pub use path::{
    bellman_ford, bellman_ford_weighted, detect_negative_cycle, detect_negative_cycle_weighted,
};

// Re-exports - Centrality
pub use centrality::{BetweennessCentrality, BetweennessConfig, BetweennessResult};
pub use centrality::{ClosenessCentrality, ClosenessConfig, ClosenessResult, HarmonicCentrality};
pub use centrality::{DegreeCentrality, calculate_all_degrees, calculate_degree};
pub use centrality::{EigenvectorCentrality, EigenvectorConfig, EigenvectorResult};
pub use centrality::{HITS, HITSConfig, HITSResult};
pub use centrality::{KatzCentrality, KatzConfig, KatzResult};
pub use centrality::{PageRank, PageRankConfig, PageRankResult};
pub use centrality::{authority_score, authority_scores, hits, hub_score, hub_scores};
pub use centrality::{
    katz_centrality, katz_centrality_with_alpha, katz_centrality_with_params, katz_score,
};

// Re-exports - Community
pub use community::{ConnectedComponents, ConnectedComponentsConfig, ConnectedComponentsResult};
pub use community::{LabelPropagation, LabelPropagationConfig, LabelPropagationResult};
pub use community::{LabelPropagationAnalyzer, LabelPropagationStatistics};
pub use community::{Louvain, LouvainConfig, LouvainResult};
pub use community::{
    StronglyConnectedComponents, StronglyConnectedConfig, StronglyConnectedResult,
};
pub use community::{label_propagation, label_propagation_with_iterations};

// Re-exports - Similarity
pub use similarity::{JaccardConfig, JaccardSimilarity, SimilarityResult};
pub use similarity::{OverlapCoefficient, OverlapConfig};

// Re-exports - Link Prediction
pub use link_prediction::{AdamicAdar, AdamicAdarConfig};
pub use link_prediction::{CommonNeighbors, CommonNeighborsConfig};
pub use link_prediction::{LinkPredictionMetric, LinkPredictionResult, LinkPredictor};
pub use link_prediction::{PreferentialAttachment, PreferentialAttachmentConfig};
pub use link_prediction::{ResourceAllocation, ResourceAllocationConfig};
pub use link_prediction::{TotalNeighbors, TotalNeighborsConfig};

// Re-exports - Metrics
pub use metrics::{ClusteringCoefficient, ClusteringConfig};
pub use metrics::{Density, DensityConfig, calculate_density};
pub use metrics::{
    DistanceConfig, DistanceMetrics, average_path_length, diameter, eccentricity, radius,
};
pub use metrics::{GraphMetrics, GraphMetricsFullResult, GraphMetricsResult};
pub use metrics::{TriangleConfig, TriangleCount, count_node_triangles, count_triangles};
pub use metrics::{
    average_clustering_coefficient, global_clustering_coefficient, local_clustering_coefficient,
};

// Re-exports - Structure
pub use structure::{ArticulationConfig, ArticulationPoints, ArticulationResult};
pub use structure::{BiconnectedComponents, BiconnectedConfig, BiconnectedResult};
pub use structure::{BlockCutNodeType, BlockCutTreeResult};
pub use structure::{BridgeConfig, BridgeResult, Bridges};
pub use structure::{GraphStructure, GraphStructureConfig, GraphStructureResult};
pub use structure::{analyze_structure, analyze_structure_full, is_structurally_robust};
pub use structure::{build_block_cut_tree, find_biconnected_components};
pub use structure::{find_articulation_points, is_articulation_point};
pub use structure::{find_bridges, is_bridge};

// Re-exports - K-Core
pub use kcore::{CoreNumberResult, KCore, KCoreConfig, KCoreResult};
pub use kcore::{KCoreAnalysisResult, kcore_analyze};
pub use kcore::{KShellResult, ShellDecomposition, decompose_shells, find_k_shell};
pub use kcore::{compute_core_numbers, degeneracy, find_k_core, main_core, node_core_number};

// Re-exports - Walk
pub use walk::{BiasedRandomWalk, Node2VecConfig, Node2VecResult};
pub use walk::{RWRConfig, RWRResult, RandomWalkWithRestart};
pub use walk::{RandomWalk, RandomWalkConfig, RandomWalkResult, WalkDirection};
pub use walk::{node2vec_walk, node2vec_walks};
pub use walk::{random_walk, random_walks};
pub use walk::{rwr, rwr_with_restart_prob};

// Re-exports - MST
pub use mst::UnionFind;
pub use mst::{Kruskal, KruskalConfig, kruskal_mst};
pub use mst::{MSTAlgorithm, MSTResult, SpanningForestResult};
pub use mst::{Prim, PrimConfig, prim_mst, prim_mst_from};

// Re-exports - Topological
pub use topological::{CycleConfig, CycleDetector, CycleResult};
pub use topological::{DfsTopologicalSort, dfs_topological_sort};
pub use topological::{
    TopologicalAlgorithm, TopologicalConfig, TopologicalResult, TopologicalSort,
};
pub use topological::{dag_sinks, dag_sources, topological_sort, topological_sort_with_levels};
pub use topological::{find_cycle, has_cycle, is_dag};

// Re-exports - Flow
pub use flow::{EdmondsKarp, MaxFlowAlgorithm, MaxFlowConfig, MaxFlowResult};
pub use flow::{FlowEdge, FlowNetwork};
pub use flow::{MinCut, MinCutResult, min_cut, min_cut_with_capacity};
pub use flow::{PushRelabel, max_flow_pr, max_flow_pr_with_capacity};
pub use flow::{max_flow, max_flow_with_capacity};

// Re-exports - Matching
pub use matching::{BipartiteChecker, BipartiteConfig, BipartiteResult, MatchingResult};
pub use matching::{HopcroftKarp, HopcroftKarpConfig, hopcroft_karp};
pub use matching::{Hungarian, HungarianConfig, HungarianResult};
pub use matching::{bipartite_partition, is_bipartite, kuhn_matching, maximum_matching};
pub use matching::{hungarian, max_weight_matching, min_cost_matching};

// Re-exports - SCC
pub use scc::{CondensationBuilder, CondensationGraph, condensation_graph};
pub use scc::{KosarajuConfig, KosarajuSCC, kosaraju_scc};
pub use scc::{SCCConfig, SCCResult};
pub use scc::{TarjanConfig, TarjanSCC, tarjan_scc};
pub use scc::{is_strongly_connected, largest_scc, scc_count};

// Re-exports - Coloring
pub use coloring::{ColoringConfig, ColoringResult};
pub use coloring::{DSatur, chromatic_number, dsatur, graph_coloring, is_k_colorable};
pub use coloring::{GreedyColoring, greedy_coloring};
pub use coloring::{WelshPowell, welsh_powell};

// Re-exports - Eulerian
pub use eulerian::{EulerianCheckResult, EulerianChecker, EulerianConfig};
pub use eulerian::{EulerianResult, Hierholzer};
pub use eulerian::{eulerian_check, has_eulerian_circuit, has_eulerian_path};
pub use eulerian::{eulerian_circuit, eulerian_path};
pub use eulerian::{eulerian_circuit_directed, eulerian_path_directed};
pub use eulerian::{has_eulerian_circuit_directed, has_eulerian_path_directed};

// Re-exports - Hamiltonian
pub use hamiltonian::{HamiltonianBacktrack, HamiltonianConfig, HamiltonianResult};
pub use hamiltonian::{HeldKarp, WeightedHamiltonianResult};
pub use hamiltonian::{hamiltonian_circuit, hamiltonian_path};
pub use hamiltonian::{has_hamiltonian_circuit, has_hamiltonian_path};
pub use hamiltonian::{min_hamiltonian_circuit, min_hamiltonian_path};

// Re-exports - Clique
pub use clique::{BronKerbosch, find_all_maximal_cliques, find_maximum_clique};
pub use clique::{CliqueConfig, CliqueDetector, CliqueResult};
pub use clique::{clique_number, find_cliques_of_size, is_clique};

// Re-exports - Vertex Cover
pub use vertex_cover::{BranchAndBound, has_vertex_cover_of_size, minimum_vertex_cover};
pub use vertex_cover::{VertexCoverConfig, VertexCoverDetector, VertexCoverResult};
pub use vertex_cover::{
    greedy_max_degree_cover, greedy_vertex_cover, is_vertex_cover, vertex_cover_number,
};

// Re-exports - Independent Set
pub use independent_set::find_all_maximal_independent_sets;
pub use independent_set::{IndependentSetConfig, IndependentSetDetector, IndependentSetResult};
pub use independent_set::{
    MaximumIndependentSet, has_independent_set_of_size, maximum_independent_set,
};
pub use independent_set::{greedy_independent_set, independence_number, is_independent_set};

// Re-exports - Dominating Set
pub use dominating_set::connected_dominating_set;
pub use dominating_set::{DominatingSetConfig, DominatingSetDetector, DominatingSetResult};
pub use dominating_set::{
    MinimumDominatingSet, has_dominating_set_of_size, minimum_dominating_set,
};
pub use dominating_set::{domination_number, greedy_dominating_set, is_dominating_set};

// Re-exports - Feedback Set
pub use feedback::{
    FeedbackArcSetResult, FeedbackSetConfig, FeedbackSetDetector, FeedbackVertexSetResult,
};
pub use feedback::{
    MinimumFeedbackSet, has_fas_of_size, has_fvs_of_size, minimum_fas, minimum_fvs,
};
pub use feedback::{greedy_fas, greedy_fvs, is_feedback_arc_set, is_feedback_vertex_set};

// Re-exports - Isomorphism
pub use isomorphism::{IsomorphismConfig, IsomorphismResult, SubgraphResult};
pub use isomorphism::{SubgraphMatcher, find_all_subgraphs, find_subgraph, is_subgraph_isomorphic};
pub use isomorphism::{VF2, find_all_isomorphisms, find_isomorphism, is_isomorphic};
pub use isomorphism::{
    WLConfig, WeisfeilerLehman, are_potentially_isomorphic, wl_hash, wl_hash_with_iterations,
};

// Re-exports - Steiner Tree
pub use steiner::{DreyfusWagner, steiner_cost, steiner_tree};
pub use steiner::{SteinerConfig, SteinerResult};
pub use steiner::{SteinerHeuristic, steiner_tree_approx, steiner_tree_heuristic};
pub use steiner::{find_steiner_vertices, is_steiner_tree, tree_cost};

// Re-exports - Tree Decomposition
pub use tree_decomposition::{Bag, tree_decomposition as compute_tree_decomposition, treewidth};
pub use tree_decomposition::{EliminationOrdering, Heuristic as TDHeuristic};
pub use tree_decomposition::{TreeDecomposition, TreeDecompositionConfig, TreeDecompositionResult};
pub use tree_decomposition::{
    is_valid_decomposition, verify_coherence, verify_edge_coverage, verify_vertex_coverage,
};
pub use tree_decomposition::{min_degree_ordering, min_fill_ordering};

// Re-exports - Parallel
pub use parallel::{
    ParallelBetweenness, ParallelCloseness, ParallelConfig, ParallelHarmonic, ParallelPageRank,
};
pub use parallel::{
    ParallelConnectedComponents, ParallelConnectedResult, connected_components_parallel,
};
pub use parallel::{ParallelLouvain, louvain_parallel};
pub use parallel::{ParallelTriangleCount, ParallelTriangleResult, triangle_count_parallel};
pub use parallel::{
    betweenness_parallel, closeness_parallel, harmonic_parallel, pagerank_parallel,
};

// Re-exports - Streaming (v1.44.0)
pub use streaming::CentralityScore;
pub use streaming::{
    AlgorithmPhase, ChunkProgress, ProgressCallback, StreamingConfig, StreamingStats,
};
pub use streaming::{BetweennessScoreIterator, StreamingBetweenness, StreamingBetweennessConfig};
pub use streaming::{ChunkedDistanceResult, ChunkedFloydWarshall, ChunkedFloydWarshallConfig};
pub use streaming::{DistancePair, PartialPathIterator};
pub use streaming::{PageRankScoreIterator, StreamingPageRank, StreamingPageRankConfig};

// Re-exports - petgraph Compatibility (v1.76.0)
pub use petgraph_compat::algorithms as petgraph_algorithms;
pub use petgraph_compat::stats as petgraph_stats;
pub use petgraph_compat::{
    FromPetgraph, GraphConverter, NodeMapping, PetgraphEdgeData, PetgraphError, PetgraphNodeData,
    PetgraphResult, ToPetgraph,
};

// Re-exports - GPU Acceleration (v1.85.0)
#[cfg(any(feature = "gpu-cuda", feature = "gpu-metal"))]
pub use gpu::{
    BackendType, CsrGraph, GpuBackend, GpuBuffer, GpuCsrGraph, GpuDeviceInfo, GpuError, GpuResult,
    KernelArg, auto_select_backend, available_backends, is_gpu_available,
};

#[cfg(feature = "gpu-cuda")]
pub use gpu::CudaBackend;

#[cfg(all(feature = "gpu-metal", target_os = "macos"))]
pub use gpu::MetalBackend;
