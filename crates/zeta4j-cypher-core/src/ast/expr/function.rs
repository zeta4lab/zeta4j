// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 스칼라 함수 AST
//!
//! 문자열, 수학, 리스트, 타입, 그래프 알고리즘 등 200+ 함수

use super::Expression;

/// 스칼라 함수 종류 (문자열, 수학, 리스트, 타입 함수)
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarFunction {
    // === 문자열 함수 ===
    ToUpper,
    ToLower,
    Trim,
    LTrim,
    RTrim,
    Replace,
    Reverse,
    Substring,
    Left,
    Right,
    Split,
    ToString,

    // === 수학 함수 ===
    Abs,
    Ceil,
    Floor,
    Round,
    Sign,
    Sqrt,
    Rand,
    Log,
    Log10,
    Exp,
    Power,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Degrees,
    Radians,
    Pi,
    E,

    // === 리스트 함수 ===
    Head,
    Tail,
    Last,
    Size,
    Range,
    Keys,

    // === 타입/그래프 함수 ===
    Type,
    Labels,
    Properties,
    Id,

    // === 경로 요소 함수 ===
    Nodes,
    Relationships,
    Length,

    // === 타입 변환 함수 ===
    ToInteger,
    ToFloat,
    ToBoolean,

    // === 술어 함수 ===
    Coalesce,

    // === 유틸리티 함수 ===
    RandomUUID,
    Timestamp,

    // === 시간 함수 ===
    Date,
    Datetime,
    Time,
    LocalTime,
    LocalDateTime,
    Duration,

    // === 경로 함수 ===
    ShortestPath,
    Dijkstra,
    BellmanFord,
    AStar,
    FloydWarshall,
    AllPairsDistance,
    HasNegativeCycle,

    // === 중심성 함수 ===
    PageRank,
    ArticleRank,
    Betweenness,
    Closeness,
    Harmonic,
    Eigenvector,
    Katz,
    KatzScore,
    Hits,
    HubScore,
    AuthorityScore,

    // === 커뮤니티 탐지 함수 ===
    ConnectedComponents,
    StronglyConnectedComponents,
    Louvain,
    ModularityOptimization,
    LabelPropagation,
    Lpa,
    LpaAnalyze,

    // === 유사도 함수 ===
    Jaccard,
    Overlap,
    Cosine,

    // === 링크 예측 함수 ===
    CommonNeighbors,
    AdamicAdar,
    PreferentialAttachment,
    ResourceAllocation,
    TotalNeighbors,

    // === 그래프 메트릭 함수 ===
    GraphDensity,
    GraphTriangleCount,
    GraphClusteringCoefficient,
    AverageClusteringCoefficient,

    // === 거리 메트릭 함수 ===
    GraphDiameter,
    GraphRadius,
    GraphEccentricity,
    AveragePathLength,

    // === 그래프 구조 분석 함수 ===
    ArticulationPoints,
    GraphBridges,
    IsArticulationPoint,
    IsBridge,
    BiconnectedComponents,
    BlockCutTree,
    GraphStructure,
    IsStructurallyRobust,

    // === K-Core 함수 ===
    KCoreNodes,
    CoreNumber,
    CoreNumbers,
    GraphDegeneracy,

    // === K-Shell 함수 ===
    KShellNodes,
    ShellDecomposition,

    // === K-Core Analysis 함수 ===
    KCoreAnalysis,

    // === Random Walk 함수 ===
    RandomWalk,
    RandomWalks,
    Rwr,
    Node2VecWalk,

    // === Graph Embedding 함수 ===
    FastRP,
    HashGNN,

    // === MST 함수 ===
    Mst,
    KruskalMst,
    SpanningForest,

    // === Topological Sort 함수 ===
    HasCycle,
    IsDAG,
    FindCycle,
    TopoSort,
    TopoSortDfs,
    DagSources,
    DagSinks,
    DagLevels,

    // === Maximum Flow 함수 ===
    MaxFlow,
    MaxFlowPR,
    MinCut,

    // === Bipartite Matching 함수 ===
    IsBipartite,
    BipartitePartition,
    MaxMatching,
    MaxMatchingKuhn,
    MinCostMatching,
    MaxWeightMatching,

    // === SCC 함수 ===
    Scc,
    SccCount,
    SccOf,
    IsStronglyConnected,
    LargestScc,
    CondensationGraph,

    // === Graph Coloring 함수 ===
    GraphColoring,
    GreedyColoring,
    WelshPowellColoring,
    ChromaticNumber,
    ColorOf,
    IsKColorable,

    // === Eulerian Path/Circuit 함수 ===
    HasEulerianCircuit,
    HasEulerianPath,
    EulerianCheck,
    EulerianCircuit,
    EulerianPath,

    // === Hamiltonian Path/Circuit 함수 ===
    HasHamiltonianPath,
    HasHamiltonianCircuit,
    HamiltonianPath,
    HamiltonianCircuit,

    // === Clique Detection 함수 ===
    IsClique,
    FindCliquesOfSize,
    FindAllMaximalCliques,
    FindMaximumClique,
    CliqueNumber,

    // === Vertex Cover 함수 ===
    IsVertexCover,
    GreedyVertexCover,
    MinimumVertexCover,
    HasVertexCoverOfSize,
    VertexCoverNumber,

    // === Independent Set 함수 ===
    IsIndependentSet,
    GreedyIndependentSet,
    MaximumIndependentSet,
    HasIndependentSetOfSize,
    IndependenceNumber,
    FindAllMaximalIndependentSets,

    // === Dominating Set 함수 ===
    IsDominatingSet,
    GreedyDominatingSet,
    MinimumDominatingSet,
    HasDominatingSetOfSize,
    DominationNumber,
    ConnectedDominatingSet,

    // === Feedback Set 함수 ===
    IsFeedbackVertexSet,
    IsFeedbackArcSet,
    GreedyFVS,
    GreedyFAS,
    MinimumFVS,
    MinimumFAS,
    HasFVSOfSize,
    HasFASOfSize,

    // === Graph Isomorphism 함수 ===
    IsIsomorphic,
    IsSubgraphIsomorphic,
    WLHash,
    ArePotentiallyIsomorphic,

    // === Steiner Tree 함수 ===
    SteinerTree,
    SteinerTreeHeuristic,
    SteinerCost,
    IsSteinerTree,
    SteinerVertices,

    // === Tree Decomposition 함수 ===
    TreeDecomposition,
    Treewidth,

    // === 공간 함수 ===
    Point,
    Distance,
    WithinBBox,

    // === 벡터 유사도 함수 ===
    VectorCosineSimilarity,
    VectorEuclideanDistance,
    VectorDotProduct,
    VectorDistance,
    VectorDimensions,
    VectorNormalize,
}

/// 스칼라 함수 호출
#[derive(Debug, Clone, PartialEq)]
pub struct ScalarFunctionCall {
    /// 함수 종류
    pub function: ScalarFunction,
    /// 인자 표현식들
    pub arguments: Vec<Expression>,
}
