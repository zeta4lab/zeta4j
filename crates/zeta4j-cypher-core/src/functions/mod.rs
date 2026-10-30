//! 스칼라 함수 구현 모듈
//!
//! `ScalarFunction` enum에 대한 `evaluate()` 메서드를 제공합니다.
//! 각 카테고리별 함수는 하위 모듈로 분리되어 있습니다.
//!
//! # 구조
//!
//! - `string.rs`: 문자열 함수 (toUpper, toLower, trim, etc.)
//! - `math.rs`: 수학 함수 (abs, ceil, floor, etc.)
//! - `list.rs`: 리스트 함수 (head, tail, size, etc.)
//! - `type_conv.rs`: 타입 변환 함수 (toInteger, toFloat, etc.)
//! - `temporal.rs`: 시간 함수 (date, datetime, duration, etc.)
//! - `graph.rs`: 그래프 알고리즘 함수 (pageRank, shortestPath, etc.)
//! - `spatial.rs`: 공간 함수 (point, distance, etc.)
//! - `vector.rs`: 벡터 유사도 함수 (cosine, euclidean, dotProduct, etc.)
//!
//! # 매크로
//!
//! - [`check_args!`]: 함수 인자 개수 검증
//! - [`propagate_null!`]: NULL 값 전파 처리

pub mod graph;
pub mod list;
pub mod math;
pub mod spatial;
pub mod string;
pub mod temporal;
pub mod type_conv;
pub mod vector;

use crate::ast::ScalarFunction;
use zeta4j_common::{ZetaError, codes};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

// =============================================================================
// 함수 인자 검증 매크로
// =============================================================================

/// 함수 인자 개수 검증 매크로
///
/// # 사용법
///
/// ```ignore
/// // 정확히 1개 인자 필요
/// check_args!(args, 1, "toUpper");
///
/// // 1~3개 인자 허용
/// check_args!(args, 1..=3, "substring");
/// ```
#[macro_export]
macro_rules! check_args {
    ($args:expr_2021, $expected:literal, $func:expr_2021) => {
        if $args.len() != $expected {
            return Err(zeta4j_graph::GraphError::Zeta(
                zeta4j_common::ZetaError::InvalidArgument {
                    code: zeta4j_common::codes::cypher::INVALID_ARGUMENT_COUNT,
                    message: format!(
                        "{}() requires {} argument(s), got {}",
                        $func,
                        $expected,
                        $args.len()
                    ),
                },
            ));
        }
    };
    ($args:expr_2021, $min:literal..=$max:literal, $func:expr_2021) => {
        if $args.len() < $min || $args.len() > $max {
            return Err(zeta4j_graph::GraphError::Zeta(
                zeta4j_common::ZetaError::InvalidArgument {
                    code: zeta4j_common::codes::cypher::INVALID_ARGUMENT_COUNT,
                    message: format!(
                        "{}() requires {}-{} argument(s), got {}",
                        $func,
                        $min,
                        $max,
                        $args.len()
                    ),
                },
            ));
        }
    };
}

/// NULL 전파 처리 매크로
///
/// Cypher 시맨틱: NULL 입력 → NULL 출력
///
/// # 사용법
///
/// ```ignore
/// // 모든 인자 중 하나라도 NULL이면 NULL 반환
/// propagate_null!(args);
///
/// // 첫 번째 인자만 검사
/// propagate_null!(args, first);
/// ```
#[macro_export]
macro_rules! propagate_null {
    ($args:expr_2021) => {
        if $args
            .iter()
            .any(|v| matches!(v, zeta4j_graph::core::Value::Null))
        {
            return Ok(zeta4j_graph::core::Value::Null);
        }
    };
    ($args:expr_2021, first) => {
        if matches!($args.first(), Some(zeta4j_graph::core::Value::Null)) {
            return Ok(zeta4j_graph::core::Value::Null);
        }
    };
}

// Re-export macros for use in submodules
pub use check_args;
pub use propagate_null;

// =============================================================================
// 기존 헬퍼 함수 (하위 호환성 유지, 에러 코드 적용)
// =============================================================================

/// 인자 개수 검증 헬퍼
///
/// 새 코드에서는 `check_args!` 매크로 사용을 권장합니다.
pub fn validate_arg_count(name: &str, args: &[Value], expected: usize) -> Result<()> {
    if args.len() != expected {
        return Err(GraphError::Zeta(ZetaError::InvalidArgument {
            code: codes::cypher::INVALID_ARGUMENT_COUNT,
            message: format!(
                "{}() requires {} argument(s), got {}",
                name,
                expected,
                args.len()
            ),
        }));
    }
    Ok(())
}

/// 인자 개수 범위 검증 헬퍼
///
/// 새 코드에서는 `check_args!` 매크로 사용을 권장합니다.
pub fn validate_arg_count_range(name: &str, args: &[Value], min: usize, max: usize) -> Result<()> {
    if args.len() < min || args.len() > max {
        return Err(GraphError::Zeta(ZetaError::InvalidArgument {
            code: codes::cypher::INVALID_ARGUMENT_COUNT,
            message: format!(
                "{}() requires {}-{} argument(s), got {}",
                name,
                min,
                max,
                args.len()
            ),
        }));
    }
    Ok(())
}

impl ScalarFunction {
    /// 스칼라 함수 실행
    ///
    /// 각 카테고리별로 하위 모듈에 위임합니다.
    pub fn evaluate(&self, args: &[Value]) -> Result<Value> {
        match self {
            // === 문자열 함수 ===
            ScalarFunction::ToUpper
            | ScalarFunction::ToLower
            | ScalarFunction::Trim
            | ScalarFunction::LTrim
            | ScalarFunction::RTrim
            | ScalarFunction::Replace
            | ScalarFunction::Reverse
            | ScalarFunction::Substring
            | ScalarFunction::Left
            | ScalarFunction::Right
            | ScalarFunction::Split
            | ScalarFunction::ToString => string::evaluate(self, args),

            // === 수학 함수 ===
            ScalarFunction::Abs
            | ScalarFunction::Ceil
            | ScalarFunction::Floor
            | ScalarFunction::Round
            | ScalarFunction::Sign
            | ScalarFunction::Sqrt
            | ScalarFunction::Rand
            | ScalarFunction::Log
            | ScalarFunction::Log10
            | ScalarFunction::Exp
            | ScalarFunction::Power
            | ScalarFunction::Sin
            | ScalarFunction::Cos
            | ScalarFunction::Tan
            | ScalarFunction::Asin
            | ScalarFunction::Acos
            | ScalarFunction::Atan
            | ScalarFunction::Atan2
            | ScalarFunction::Degrees
            | ScalarFunction::Radians
            | ScalarFunction::Pi
            | ScalarFunction::E => math::evaluate(self, args),

            // === 리스트 함수 ===
            ScalarFunction::Head
            | ScalarFunction::Tail
            | ScalarFunction::Last
            | ScalarFunction::Size
            | ScalarFunction::Range => list::evaluate(self, args),

            // === 그래프 요소 함수 (엔진 접근 필요 - executor에서 처리) ===
            ScalarFunction::Type
            | ScalarFunction::Labels
            | ScalarFunction::Properties
            | ScalarFunction::Id
            | ScalarFunction::Keys  // Node/Edge의 키를 가져오려면 엔진 접근 필요
            // 경로 함수
            | ScalarFunction::Nodes
            | ScalarFunction::Relationships => {
                Err(GraphError::Validation {
                    message: format!(
                        "{:?} requires graph engine access - should be handled by executor",
                        self
                    ),
                })
            }

            // length() - 경로/문자열/배열 길이 (그래프 엔진 없이 처리 가능)
            ScalarFunction::Length => {
                if args.len() != 1 {
                    return Err(GraphError::Validation {
                        message: format!("length() requires 1 argument, got {}", args.len()),
                    });
                }
                match &args[0] {
                    Value::Path(path) => Ok(Value::Integer(path.len() as i64)),
                    Value::String(s) => Ok(Value::Integer(s.chars().count() as i64)),
                    Value::Array(arr) => Ok(Value::Integer(arr.len() as i64)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(GraphError::Validation {
                        message: format!("length() expected Path, String, or List, got {}", args[0].type_name()),
                    }),
                }
            }

            // === 타입 변환 함수 ===
            ScalarFunction::ToInteger
            | ScalarFunction::ToFloat
            | ScalarFunction::ToBoolean
            | ScalarFunction::Coalesce
            | ScalarFunction::RandomUUID
            | ScalarFunction::Timestamp => type_conv::evaluate(self, args),

            // === 시간 함수 ===
            ScalarFunction::Date
            | ScalarFunction::Datetime
            | ScalarFunction::Time
            | ScalarFunction::LocalTime
            | ScalarFunction::LocalDateTime
            | ScalarFunction::Duration => temporal::evaluate(self, args),

            // === 공간 함수 ===
            ScalarFunction::Point
            | ScalarFunction::Distance
            | ScalarFunction::WithinBBox => spatial::evaluate(self, args),

            // === 벡터 유사도 함수 ===
            ScalarFunction::VectorCosineSimilarity
            | ScalarFunction::VectorEuclideanDistance
            | ScalarFunction::VectorDotProduct
            | ScalarFunction::VectorDistance
            | ScalarFunction::VectorDimensions
            | ScalarFunction::VectorNormalize => vector::evaluate(self, args),

            // === 그래프 알고리즘 함수 ===
            // 경로 탐색
            ScalarFunction::ShortestPath
            | ScalarFunction::Dijkstra
            | ScalarFunction::BellmanFord
            | ScalarFunction::AStar
            | ScalarFunction::FloydWarshall
            | ScalarFunction::AllPairsDistance
            | ScalarFunction::HasNegativeCycle
            // 중심성
            | ScalarFunction::PageRank
            | ScalarFunction::ArticleRank
            | ScalarFunction::Betweenness
            | ScalarFunction::Closeness
            | ScalarFunction::Harmonic
            | ScalarFunction::Eigenvector
            | ScalarFunction::Katz
            | ScalarFunction::KatzScore
            | ScalarFunction::Hits
            | ScalarFunction::HubScore
            | ScalarFunction::AuthorityScore
            // 커뮤니티 탐지
            | ScalarFunction::ConnectedComponents
            | ScalarFunction::StronglyConnectedComponents
            | ScalarFunction::Louvain
            | ScalarFunction::LabelPropagation
            | ScalarFunction::Lpa
            | ScalarFunction::LpaAnalyze
            | ScalarFunction::ModularityOptimization
            // 유사도
            | ScalarFunction::Jaccard
            | ScalarFunction::Overlap
            | ScalarFunction::Cosine
            // 링크 예측
            | ScalarFunction::CommonNeighbors
            | ScalarFunction::AdamicAdar
            | ScalarFunction::PreferentialAttachment
            | ScalarFunction::ResourceAllocation
            | ScalarFunction::TotalNeighbors
            // 그래프 메트릭
            | ScalarFunction::GraphDensity
            | ScalarFunction::GraphTriangleCount
            | ScalarFunction::GraphClusteringCoefficient
            | ScalarFunction::AverageClusteringCoefficient
            // 거리 메트릭
            | ScalarFunction::GraphDiameter
            | ScalarFunction::GraphRadius
            | ScalarFunction::GraphEccentricity
            | ScalarFunction::AveragePathLength
            // 그래프 구조 분석
            | ScalarFunction::ArticulationPoints
            | ScalarFunction::GraphBridges
            | ScalarFunction::IsArticulationPoint
            | ScalarFunction::IsBridge
            | ScalarFunction::BiconnectedComponents
            | ScalarFunction::BlockCutTree
            | ScalarFunction::GraphStructure
            | ScalarFunction::IsStructurallyRobust
            // K-Core
            | ScalarFunction::KCoreNodes
            | ScalarFunction::CoreNumber
            | ScalarFunction::CoreNumbers
            | ScalarFunction::GraphDegeneracy
            // K-Shell
            | ScalarFunction::KShellNodes
            | ScalarFunction::ShellDecomposition
            | ScalarFunction::KCoreAnalysis
            // Random Walk
            | ScalarFunction::RandomWalk
            | ScalarFunction::RandomWalks
            | ScalarFunction::Rwr
            | ScalarFunction::Node2VecWalk
            // Graph Embedding
            | ScalarFunction::FastRP
            | ScalarFunction::HashGNN
            // MST
            | ScalarFunction::Mst
            | ScalarFunction::KruskalMst
            | ScalarFunction::SpanningForest
            // Topological Sort
            | ScalarFunction::HasCycle
            | ScalarFunction::IsDAG
            | ScalarFunction::FindCycle
            | ScalarFunction::TopoSort
            | ScalarFunction::TopoSortDfs
            | ScalarFunction::DagSources
            | ScalarFunction::DagSinks
            | ScalarFunction::DagLevels
            // Maximum Flow
            | ScalarFunction::MaxFlow
            | ScalarFunction::MaxFlowPR
            | ScalarFunction::MinCut
            // Bipartite Matching
            | ScalarFunction::IsBipartite
            | ScalarFunction::BipartitePartition
            | ScalarFunction::MaxMatching
            | ScalarFunction::MaxMatchingKuhn
            | ScalarFunction::MinCostMatching
            | ScalarFunction::MaxWeightMatching
            // SCC
            | ScalarFunction::Scc
            | ScalarFunction::SccCount
            | ScalarFunction::SccOf
            | ScalarFunction::IsStronglyConnected
            | ScalarFunction::LargestScc
            | ScalarFunction::CondensationGraph
            // Graph Coloring
            | ScalarFunction::GraphColoring
            | ScalarFunction::GreedyColoring
            | ScalarFunction::WelshPowellColoring
            | ScalarFunction::ChromaticNumber
            | ScalarFunction::ColorOf
            | ScalarFunction::IsKColorable
            // Eulerian
            | ScalarFunction::HasEulerianCircuit
            | ScalarFunction::HasEulerianPath
            | ScalarFunction::EulerianCheck
            | ScalarFunction::EulerianCircuit
            | ScalarFunction::EulerianPath
            // Hamiltonian
            | ScalarFunction::HasHamiltonianPath
            | ScalarFunction::HasHamiltonianCircuit
            | ScalarFunction::HamiltonianPath
            | ScalarFunction::HamiltonianCircuit
            // Clique
            | ScalarFunction::IsClique
            | ScalarFunction::FindCliquesOfSize
            | ScalarFunction::FindAllMaximalCliques
            | ScalarFunction::FindMaximumClique
            | ScalarFunction::CliqueNumber
            // Vertex Cover
            | ScalarFunction::IsVertexCover
            | ScalarFunction::GreedyVertexCover
            | ScalarFunction::MinimumVertexCover
            | ScalarFunction::HasVertexCoverOfSize
            | ScalarFunction::VertexCoverNumber
            // Independent Set
            | ScalarFunction::IsIndependentSet
            | ScalarFunction::GreedyIndependentSet
            | ScalarFunction::MaximumIndependentSet
            | ScalarFunction::HasIndependentSetOfSize
            | ScalarFunction::IndependenceNumber
            | ScalarFunction::FindAllMaximalIndependentSets
            // Dominating Set
            | ScalarFunction::IsDominatingSet
            | ScalarFunction::GreedyDominatingSet
            | ScalarFunction::MinimumDominatingSet
            | ScalarFunction::HasDominatingSetOfSize
            | ScalarFunction::DominationNumber
            | ScalarFunction::ConnectedDominatingSet
            // Feedback Set
            | ScalarFunction::IsFeedbackVertexSet
            | ScalarFunction::IsFeedbackArcSet
            | ScalarFunction::GreedyFVS
            | ScalarFunction::GreedyFAS
            | ScalarFunction::MinimumFVS
            | ScalarFunction::MinimumFAS
            | ScalarFunction::HasFVSOfSize
            | ScalarFunction::HasFASOfSize
            // Isomorphism
            | ScalarFunction::IsIsomorphic
            | ScalarFunction::IsSubgraphIsomorphic
            | ScalarFunction::WLHash
            | ScalarFunction::ArePotentiallyIsomorphic
            // Steiner Tree
            | ScalarFunction::SteinerTree
            | ScalarFunction::SteinerTreeHeuristic
            | ScalarFunction::SteinerCost
            | ScalarFunction::IsSteinerTree
            | ScalarFunction::SteinerVertices
            // Tree Decomposition
            | ScalarFunction::TreeDecomposition
            | ScalarFunction::Treewidth => graph::evaluate(self, args),
        }
    }
}
