//! 그래프 알고리즘 스칼라 함수 실행 모듈
//!
//! Cypher의 그래프 알고리즘 함수들을 처리합니다.
//! 실제 알고리즘 구현은 `src/algorithm/` 모듈에 있으며,
//! 이 모듈은 Cypher 인자 파싱과 결과 변환(glue code)을 담당합니다.

mod centrality;
mod classic;
mod community;
mod kcore;
mod metrics;
mod path;
mod similarity;
mod walk;

use super::super::QueryExecutor;
use zeta4j_cypher_core::ast::ScalarFunction;
use zeta4j_graph::core::Value;
use zeta4j_graph::error::Result;

// temporal 모듈의 validate_arg_count를 하위 모듈에서 사용할 수 있도록 re-export
pub(super) use super::temporal::validate_arg_count;

impl QueryExecutor {
    /// 그래프 알고리즘 스칼라 함수 실행
    pub(super) async fn evaluate_graph_algorithm_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
    ) -> Result<Value> {
        match func {
            // 경로 알고리즘
            ScalarFunction::ShortestPath
            | ScalarFunction::Dijkstra
            | ScalarFunction::BellmanFord
            | ScalarFunction::AStar
            | ScalarFunction::FloydWarshall
            | ScalarFunction::AllPairsDistance
            | ScalarFunction::HasNegativeCycle => self.evaluate_path_function(func, args).await,

            // 중심성 알고리즘
            ScalarFunction::PageRank
            | ScalarFunction::ArticleRank
            | ScalarFunction::Betweenness
            | ScalarFunction::Closeness
            | ScalarFunction::Harmonic
            | ScalarFunction::Eigenvector
            | ScalarFunction::Katz
            | ScalarFunction::KatzScore
            | ScalarFunction::Hits
            | ScalarFunction::HubScore
            | ScalarFunction::AuthorityScore => self.evaluate_centrality_function(func, args).await,

            // 커뮤니티 탐지
            ScalarFunction::ConnectedComponents
            | ScalarFunction::StronglyConnectedComponents
            | ScalarFunction::Louvain
            | ScalarFunction::ModularityOptimization
            | ScalarFunction::LabelPropagation
            | ScalarFunction::Lpa
            | ScalarFunction::LpaAnalyze => self.evaluate_community_function(func, args).await,

            // 유사도 및 링크 예측
            ScalarFunction::Jaccard
            | ScalarFunction::Overlap
            | ScalarFunction::Cosine
            | ScalarFunction::CommonNeighbors
            | ScalarFunction::AdamicAdar
            | ScalarFunction::PreferentialAttachment
            | ScalarFunction::ResourceAllocation
            | ScalarFunction::TotalNeighbors => self.evaluate_similarity_function(func, args).await,

            // 그래프 메트릭 및 구조 분석
            ScalarFunction::GraphDensity
            | ScalarFunction::GraphTriangleCount
            | ScalarFunction::GraphClusteringCoefficient
            | ScalarFunction::AverageClusteringCoefficient
            | ScalarFunction::GraphDiameter
            | ScalarFunction::GraphRadius
            | ScalarFunction::GraphEccentricity
            | ScalarFunction::AveragePathLength
            | ScalarFunction::ArticulationPoints
            | ScalarFunction::GraphBridges
            | ScalarFunction::IsArticulationPoint
            | ScalarFunction::IsBridge
            | ScalarFunction::BiconnectedComponents
            | ScalarFunction::BlockCutTree
            | ScalarFunction::GraphStructure
            | ScalarFunction::IsStructurallyRobust => {
                self.evaluate_metrics_function(func, args).await
            }

            // K-Core 분석
            ScalarFunction::KCoreNodes
            | ScalarFunction::CoreNumber
            | ScalarFunction::CoreNumbers
            | ScalarFunction::GraphDegeneracy
            | ScalarFunction::KShellNodes
            | ScalarFunction::ShellDecomposition
            | ScalarFunction::KCoreAnalysis => self.evaluate_kcore_function(func, args).await,

            // 랜덤 워크 및 임베딩
            ScalarFunction::RandomWalk
            | ScalarFunction::RandomWalks
            | ScalarFunction::Rwr
            | ScalarFunction::Node2VecWalk
            | ScalarFunction::FastRP
            | ScalarFunction::HashGNN => self.evaluate_walk_function(func, args).await,

            // 클래식 그래프 알고리즘
            ScalarFunction::Mst
            | ScalarFunction::KruskalMst
            | ScalarFunction::SpanningForest
            | ScalarFunction::HasCycle
            | ScalarFunction::IsDAG
            | ScalarFunction::FindCycle
            | ScalarFunction::TopoSort
            | ScalarFunction::TopoSortDfs
            | ScalarFunction::DagSources
            | ScalarFunction::DagSinks
            | ScalarFunction::DagLevels
            | ScalarFunction::MaxFlow
            | ScalarFunction::MaxFlowPR
            | ScalarFunction::MinCut
            | ScalarFunction::IsBipartite
            | ScalarFunction::BipartitePartition
            | ScalarFunction::MaxMatching
            | ScalarFunction::MaxMatchingKuhn
            | ScalarFunction::MinCostMatching
            | ScalarFunction::MaxWeightMatching
            | ScalarFunction::Scc
            | ScalarFunction::SccCount
            | ScalarFunction::SccOf
            | ScalarFunction::IsStronglyConnected
            | ScalarFunction::LargestScc
            | ScalarFunction::CondensationGraph
            | ScalarFunction::GraphColoring
            | ScalarFunction::GreedyColoring
            | ScalarFunction::WelshPowellColoring
            | ScalarFunction::ChromaticNumber
            | ScalarFunction::ColorOf
            | ScalarFunction::IsKColorable
            | ScalarFunction::HasEulerianCircuit
            | ScalarFunction::HasEulerianPath
            | ScalarFunction::EulerianCheck
            | ScalarFunction::EulerianCircuit
            | ScalarFunction::EulerianPath
            | ScalarFunction::HasHamiltonianPath
            | ScalarFunction::HasHamiltonianCircuit
            | ScalarFunction::HamiltonianPath
            | ScalarFunction::HamiltonianCircuit
            | ScalarFunction::IsClique
            | ScalarFunction::FindCliquesOfSize
            | ScalarFunction::FindAllMaximalCliques
            | ScalarFunction::FindMaximumClique
            | ScalarFunction::CliqueNumber
            | ScalarFunction::IsVertexCover
            | ScalarFunction::GreedyVertexCover
            | ScalarFunction::MinimumVertexCover
            | ScalarFunction::HasVertexCoverOfSize
            | ScalarFunction::VertexCoverNumber
            | ScalarFunction::IsIndependentSet
            | ScalarFunction::GreedyIndependentSet
            | ScalarFunction::MaximumIndependentSet
            | ScalarFunction::HasIndependentSetOfSize
            | ScalarFunction::IndependenceNumber
            | ScalarFunction::FindAllMaximalIndependentSets
            | ScalarFunction::IsDominatingSet
            | ScalarFunction::GreedyDominatingSet
            | ScalarFunction::MinimumDominatingSet
            | ScalarFunction::HasDominatingSetOfSize
            | ScalarFunction::DominationNumber
            | ScalarFunction::ConnectedDominatingSet
            | ScalarFunction::IsFeedbackVertexSet
            | ScalarFunction::IsFeedbackArcSet
            | ScalarFunction::GreedyFVS
            | ScalarFunction::GreedyFAS
            | ScalarFunction::MinimumFVS
            | ScalarFunction::MinimumFAS
            | ScalarFunction::HasFVSOfSize
            | ScalarFunction::HasFASOfSize
            | ScalarFunction::IsIsomorphic
            | ScalarFunction::IsSubgraphIsomorphic
            | ScalarFunction::WLHash
            | ScalarFunction::ArePotentiallyIsomorphic
            | ScalarFunction::SteinerTree
            | ScalarFunction::SteinerTreeHeuristic
            | ScalarFunction::SteinerCost
            | ScalarFunction::IsSteinerTree
            | ScalarFunction::SteinerVertices
            | ScalarFunction::TreeDecomposition
            | ScalarFunction::Treewidth => self.evaluate_classic_function(func, args).await,

            // 그래프 알고리즘이 아닌 함수는 여기서 처리하지 않음
            _ => unreachable!(
                "Non-graph algorithm function passed to evaluate_graph_algorithm_function"
            ),
        }
    }
}
