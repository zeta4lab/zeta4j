//! Function name mapping
//!
//! 스칼라 함수 이름을 ScalarFunction 열거형으로 변환
//!
//! ## 성능 노트 (v1.33.0)
//!
//! Rust 컴파일러는 match 문을 점프 테이블로 최적화하여
//! 150개 이상의 패턴에서도 O(1) 성능을 제공합니다.
//! 벤치마크 결과, match 문이 HashMap보다 ~5% 더 빠릅니다.

use crate::ast::ScalarFunction;

/// 함수명을 ScalarFunction 열거형으로 변환
///
/// 지원되는 모든 스칼라 함수의 매핑을 담당합니다.
/// 알 수 없는 함수명인 경우 None을 반환합니다.
///
/// # Arguments
/// * `name` - 소문자로 변환된 함수명
///
/// # Returns
/// * `Some(ScalarFunction)` - 매칭되는 함수
/// * `None` - 알 수 없는 함수명
///
/// # Performance
/// Rust 컴파일러가 점프 테이블로 최적화하여 O(1) 성능 제공
#[inline]
pub fn scalar_function_from_name(name: &str) -> Option<ScalarFunction> {
    match name {
        // === 문자열 함수 (12개) ===
        "toupper" => Some(ScalarFunction::ToUpper),
        "tolower" => Some(ScalarFunction::ToLower),
        "trim" => Some(ScalarFunction::Trim),
        "ltrim" => Some(ScalarFunction::LTrim),
        "rtrim" => Some(ScalarFunction::RTrim),
        "replace" => Some(ScalarFunction::Replace),
        "reverse" => Some(ScalarFunction::Reverse),
        "substring" => Some(ScalarFunction::Substring),
        "left" => Some(ScalarFunction::Left),
        "right" => Some(ScalarFunction::Right),
        "split" => Some(ScalarFunction::Split),
        "tostring" => Some(ScalarFunction::ToString),

        // === 수학 함수 (22개) ===
        "abs" => Some(ScalarFunction::Abs),
        "ceil" => Some(ScalarFunction::Ceil),
        "floor" => Some(ScalarFunction::Floor),
        "round" => Some(ScalarFunction::Round),
        "sign" => Some(ScalarFunction::Sign),
        "sqrt" => Some(ScalarFunction::Sqrt),
        "rand" => Some(ScalarFunction::Rand),
        "log" => Some(ScalarFunction::Log),
        "log10" => Some(ScalarFunction::Log10),
        "exp" => Some(ScalarFunction::Exp),
        "power" => Some(ScalarFunction::Power),
        "sin" => Some(ScalarFunction::Sin),
        "cos" => Some(ScalarFunction::Cos),
        "tan" => Some(ScalarFunction::Tan),
        "asin" => Some(ScalarFunction::Asin),
        "acos" => Some(ScalarFunction::Acos),
        "atan" => Some(ScalarFunction::Atan),
        "atan2" => Some(ScalarFunction::Atan2),
        "degrees" => Some(ScalarFunction::Degrees),
        "radians" => Some(ScalarFunction::Radians),
        "pi" => Some(ScalarFunction::Pi),
        "e" => Some(ScalarFunction::E),

        // === 리스트 함수 (6개) ===
        "head" => Some(ScalarFunction::Head),
        "tail" => Some(ScalarFunction::Tail),
        "last" => Some(ScalarFunction::Last),
        "size" => Some(ScalarFunction::Size),
        "range" => Some(ScalarFunction::Range),
        "keys" => Some(ScalarFunction::Keys),

        // === 타입/그래프 함수 (4개) ===
        "type" => Some(ScalarFunction::Type),
        "labels" => Some(ScalarFunction::Labels),
        "properties" => Some(ScalarFunction::Properties),
        "id" => Some(ScalarFunction::Id),

        // === 경로 함수 (4개) ===
        "nodes" => Some(ScalarFunction::Nodes),
        "relationships" => Some(ScalarFunction::Relationships),
        "rels" => Some(ScalarFunction::Relationships), // relationships의 별칭
        "length" => Some(ScalarFunction::Length),

        // === 타입 변환 함수 (3개) ===
        "tointeger" => Some(ScalarFunction::ToInteger),
        "tofloat" => Some(ScalarFunction::ToFloat),
        "toboolean" => Some(ScalarFunction::ToBoolean),

        // === 술어 함수 (1개) ===
        "coalesce" => Some(ScalarFunction::Coalesce),

        // === 유틸리티 함수 (2개) ===
        "randomuuid" => Some(ScalarFunction::RandomUUID),
        "timestamp" => Some(ScalarFunction::Timestamp),

        // === 시간 함수 (6개) ===
        "date" => Some(ScalarFunction::Date),
        "datetime" => Some(ScalarFunction::Datetime),
        "time" => Some(ScalarFunction::Time),
        "localtime" => Some(ScalarFunction::LocalTime),
        "localdatetime" => Some(ScalarFunction::LocalDateTime),
        "duration" => Some(ScalarFunction::Duration),

        // === 공간 함수 (3개) ===
        "point" => Some(ScalarFunction::Point),
        "distance" => Some(ScalarFunction::Distance),
        "withinbbox" => Some(ScalarFunction::WithinBBox),

        // === 경로 함수 (7개) ===
        "shortestpath" => Some(ScalarFunction::ShortestPath),
        "dijkstra" => Some(ScalarFunction::Dijkstra),
        "bellmanford" => Some(ScalarFunction::BellmanFord),
        "astar" => Some(ScalarFunction::AStar),
        "floydwarshall" => Some(ScalarFunction::FloydWarshall),
        "allpairsdistance" => Some(ScalarFunction::AllPairsDistance),
        "hasnegativecycle" => Some(ScalarFunction::HasNegativeCycle),

        // === 중심성 함수 (11개) ===
        "pagerank" => Some(ScalarFunction::PageRank),
        "articlerank" => Some(ScalarFunction::ArticleRank),
        "betweenness" => Some(ScalarFunction::Betweenness),
        "closeness" => Some(ScalarFunction::Closeness),
        "harmonic" => Some(ScalarFunction::Harmonic),
        "eigenvector" => Some(ScalarFunction::Eigenvector),
        "katz" => Some(ScalarFunction::Katz),
        "katzscore" => Some(ScalarFunction::KatzScore),
        "hits" => Some(ScalarFunction::Hits),
        "hubscore" => Some(ScalarFunction::HubScore),
        "authorityscore" => Some(ScalarFunction::AuthorityScore),

        // === 커뮤니티 함수 (7개) ===
        "connectedcomponents" => Some(ScalarFunction::ConnectedComponents),
        "stronglyconnectedcomponents" => Some(ScalarFunction::StronglyConnectedComponents),
        "louvain" => Some(ScalarFunction::Louvain),
        "modularityoptimization" => Some(ScalarFunction::ModularityOptimization),
        "labelpropagation" => Some(ScalarFunction::LabelPropagation),
        "lpa" => Some(ScalarFunction::Lpa),
        "lpaanalyze" => Some(ScalarFunction::LpaAnalyze),

        // === 유사도 함수 (3개) ===
        "jaccard" => Some(ScalarFunction::Jaccard),
        "overlap" => Some(ScalarFunction::Overlap),
        "cosine" => Some(ScalarFunction::Cosine),

        // === 링크 예측 함수 (5개) ===
        "commonneighbors" => Some(ScalarFunction::CommonNeighbors),
        "adamicadar" => Some(ScalarFunction::AdamicAdar),
        "preferentialattachment" => Some(ScalarFunction::PreferentialAttachment),
        "resourceallocation" => Some(ScalarFunction::ResourceAllocation),
        "totalneighbors" => Some(ScalarFunction::TotalNeighbors),

        // === 그래프 메트릭 함수 (4개) ===
        "density" => Some(ScalarFunction::GraphDensity),
        "trianglecount" => Some(ScalarFunction::GraphTriangleCount),
        "clusteringcoefficient" => Some(ScalarFunction::GraphClusteringCoefficient),
        "averageclusteringcoefficient" => Some(ScalarFunction::AverageClusteringCoefficient),

        // === 거리 메트릭 함수 (4개) ===
        "diameter" => Some(ScalarFunction::GraphDiameter),
        "radius" => Some(ScalarFunction::GraphRadius),
        "eccentricity" => Some(ScalarFunction::GraphEccentricity),
        "averagepathlength" => Some(ScalarFunction::AveragePathLength),

        // === 그래프 구조 분석 (8개) ===
        "articulationpoints" => Some(ScalarFunction::ArticulationPoints),
        "bridges" => Some(ScalarFunction::GraphBridges),
        "isarticulationpoint" => Some(ScalarFunction::IsArticulationPoint),
        "isbridge" => Some(ScalarFunction::IsBridge),
        "biconnectedcomponents" => Some(ScalarFunction::BiconnectedComponents),
        "blockcuttree" => Some(ScalarFunction::BlockCutTree),
        "graphstructure" => Some(ScalarFunction::GraphStructure),
        "isstructurallyrobust" => Some(ScalarFunction::IsStructurallyRobust),

        // === K-Core (4개) ===
        "kcore" => Some(ScalarFunction::KCoreNodes),
        "corenumber" => Some(ScalarFunction::CoreNumber),
        "corenumbers" => Some(ScalarFunction::CoreNumbers),
        "degeneracy" => Some(ScalarFunction::GraphDegeneracy),

        // === K-Shell (3개) ===
        "kshell" => Some(ScalarFunction::KShellNodes),
        "shelldecomposition" => Some(ScalarFunction::ShellDecomposition),
        "kcoreanalysis" => Some(ScalarFunction::KCoreAnalysis),

        // === Random Walk (4개) ===
        "randomwalk" => Some(ScalarFunction::RandomWalk),
        "randomwalks" => Some(ScalarFunction::RandomWalks),
        "rwr" => Some(ScalarFunction::Rwr),
        "node2vecwalk" => Some(ScalarFunction::Node2VecWalk),

        // === Graph Embedding (2개) ===
        "fastrp" => Some(ScalarFunction::FastRP),
        "hashgnn" => Some(ScalarFunction::HashGNN),

        // === MST (3개) ===
        "mst" => Some(ScalarFunction::Mst),
        "kruskal" => Some(ScalarFunction::KruskalMst),
        "spanningforest" => Some(ScalarFunction::SpanningForest),

        // === Topological Sort (8개) ===
        "hascycle" => Some(ScalarFunction::HasCycle),
        "isdag" => Some(ScalarFunction::IsDAG),
        "findcycle" => Some(ScalarFunction::FindCycle),
        "toposort" => Some(ScalarFunction::TopoSort),
        "toposortdfs" => Some(ScalarFunction::TopoSortDfs),
        "dagsources" => Some(ScalarFunction::DagSources),
        "dagsinks" => Some(ScalarFunction::DagSinks),
        "daglevels" => Some(ScalarFunction::DagLevels),

        // === Maximum Flow (3개) ===
        "maxflow" => Some(ScalarFunction::MaxFlow),
        "maxflowpr" => Some(ScalarFunction::MaxFlowPR),
        "mincut" => Some(ScalarFunction::MinCut),

        // === Bipartite Matching (6개) ===
        "isbipartite" => Some(ScalarFunction::IsBipartite),
        "bipartitepartition" => Some(ScalarFunction::BipartitePartition),
        "maxmatching" => Some(ScalarFunction::MaxMatching),
        "maxmatchingkuhn" => Some(ScalarFunction::MaxMatchingKuhn),
        "mincostmatching" => Some(ScalarFunction::MinCostMatching),
        "maxweightmatching" => Some(ScalarFunction::MaxWeightMatching),

        // === SCC (6개) ===
        "scc" => Some(ScalarFunction::Scc),
        "scccount" => Some(ScalarFunction::SccCount),
        "sccof" => Some(ScalarFunction::SccOf),
        "isstronglyconnected" => Some(ScalarFunction::IsStronglyConnected),
        "largestscc" => Some(ScalarFunction::LargestScc),
        "condensationgraph" => Some(ScalarFunction::CondensationGraph),

        // === Graph Coloring (6개) ===
        "graphcoloring" => Some(ScalarFunction::GraphColoring),
        "greedycoloring" => Some(ScalarFunction::GreedyColoring),
        "welshpowellcoloring" => Some(ScalarFunction::WelshPowellColoring),
        "chromaticnumber" => Some(ScalarFunction::ChromaticNumber),
        "colorof" => Some(ScalarFunction::ColorOf),
        "iskcolorable" => Some(ScalarFunction::IsKColorable),

        // === Eulerian (5개) ===
        "haseuleriancircuit" => Some(ScalarFunction::HasEulerianCircuit),
        "haseulerianpath" => Some(ScalarFunction::HasEulerianPath),
        "euleriancheck" => Some(ScalarFunction::EulerianCheck),
        "euleriancircuit" => Some(ScalarFunction::EulerianCircuit),
        "eulerianpath" => Some(ScalarFunction::EulerianPath),

        // === Hamiltonian (4개) ===
        "hashamiltonianpath" => Some(ScalarFunction::HasHamiltonianPath),
        "hashamiltoniancircuit" => Some(ScalarFunction::HasHamiltonianCircuit),
        "hamiltonianpath" => Some(ScalarFunction::HamiltonianPath),
        "hamiltoniancircuit" => Some(ScalarFunction::HamiltonianCircuit),

        // === Clique (5개) ===
        "isclique" => Some(ScalarFunction::IsClique),
        "findcliquesofsize" => Some(ScalarFunction::FindCliquesOfSize),
        "findallmaximalcliques" => Some(ScalarFunction::FindAllMaximalCliques),
        "findmaximumclique" => Some(ScalarFunction::FindMaximumClique),
        "cliquenumber" => Some(ScalarFunction::CliqueNumber),

        // === Vertex Cover (5개) ===
        "isvertexcover" => Some(ScalarFunction::IsVertexCover),
        "greedyvertexcover" => Some(ScalarFunction::GreedyVertexCover),
        "minimumvertexcover" => Some(ScalarFunction::MinimumVertexCover),
        "hasvertexcoverofsize" => Some(ScalarFunction::HasVertexCoverOfSize),
        "vertexcovernumber" => Some(ScalarFunction::VertexCoverNumber),

        // === Independent Set (6개) ===
        "isindependentset" => Some(ScalarFunction::IsIndependentSet),
        "greedyindependentset" => Some(ScalarFunction::GreedyIndependentSet),
        "maximumindependentset" => Some(ScalarFunction::MaximumIndependentSet),
        "hasindependentsetofsize" => Some(ScalarFunction::HasIndependentSetOfSize),
        "independencenumber" => Some(ScalarFunction::IndependenceNumber),
        "findallmaximalindependentsets" => Some(ScalarFunction::FindAllMaximalIndependentSets),

        // === Dominating Set (6개) ===
        "isdominatingset" => Some(ScalarFunction::IsDominatingSet),
        "greedydominatingset" => Some(ScalarFunction::GreedyDominatingSet),
        "minimumdominatingset" => Some(ScalarFunction::MinimumDominatingSet),
        "hasdominatingsetofsize" => Some(ScalarFunction::HasDominatingSetOfSize),
        "dominationnumber" => Some(ScalarFunction::DominationNumber),
        "connecteddominatingset" => Some(ScalarFunction::ConnectedDominatingSet),

        // === Feedback Set (8개) ===
        "isfeedbackvertexset" => Some(ScalarFunction::IsFeedbackVertexSet),
        "isfeedbackarcset" => Some(ScalarFunction::IsFeedbackArcSet),
        "greedyfvs" => Some(ScalarFunction::GreedyFVS),
        "greedyfas" => Some(ScalarFunction::GreedyFAS),
        "minimumfvs" => Some(ScalarFunction::MinimumFVS),
        "minimumfas" => Some(ScalarFunction::MinimumFAS),
        "hasfvsofsize" => Some(ScalarFunction::HasFVSOfSize),
        "hasfasofsize" => Some(ScalarFunction::HasFASOfSize),

        // === Graph Isomorphism (4개) ===
        "isisomorphic" => Some(ScalarFunction::IsIsomorphic),
        "issubgraphisomorphic" => Some(ScalarFunction::IsSubgraphIsomorphic),
        "wlhash" => Some(ScalarFunction::WLHash),
        "arepotentiallyisomorphic" => Some(ScalarFunction::ArePotentiallyIsomorphic),

        // === Steiner Tree (5개) ===
        "steinertree" => Some(ScalarFunction::SteinerTree),
        "steinertreeheuristic" => Some(ScalarFunction::SteinerTreeHeuristic),
        "steinercost" => Some(ScalarFunction::SteinerCost),
        "issteinertree" => Some(ScalarFunction::IsSteinerTree),
        "steinervertices" => Some(ScalarFunction::SteinerVertices),

        // === Tree Decomposition (2개) ===
        "treedecomposition" => Some(ScalarFunction::TreeDecomposition),
        "treewidth" => Some(ScalarFunction::Treewidth),

        // === 벡터 유사도 함수 (6개 + 별칭 3개) ===
        "vector.similarity.cosine" => Some(ScalarFunction::VectorCosineSimilarity),
        "vector.similarity.euclidean" => Some(ScalarFunction::VectorEuclideanDistance),
        "vector.similarity.dotproduct" => Some(ScalarFunction::VectorDotProduct),
        "vector.distance" => Some(ScalarFunction::VectorDistance),
        "vector.dimensions" => Some(ScalarFunction::VectorDimensions),
        "vector.normalize" => Some(ScalarFunction::VectorNormalize),
        // 별칭 (Neo4j GDS 스타일)
        "vector.cosine" => Some(ScalarFunction::VectorCosineSimilarity),
        "vector.euclidean" => Some(ScalarFunction::VectorEuclideanDistance),
        "vector.dot" => Some(ScalarFunction::VectorDotProduct),

        // 알 수 없는 함수
        _ => None,
    }
}
