// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 스칼라 함수 평가
//!
//! QueryExecutor의 스칼라 함수 평가 로직을 분리한 모듈입니다.
//!
//! # 함수 분류
//!
//! ## 순수 스칼라 함수 (functions 모듈 위임)
//! - 문자열: toUpper, toLower, trim, replace, reverse, substring, ...
//! - 수학: abs, ceil, floor, round, sqrt, log, sin, cos, ...
//! - 리스트: head, tail, last, size, range
//! - 타입 변환: toInteger, toFloat, toBoolean, coalesce
//!
//! ## 그래프 알고리즘 함수 (scalar_graph 모듈 위임)
//! - 경로: shortestPath, dijkstra, bellmanFord, astar
//! - 중심성: pageRank, betweenness, closeness, eigenvector
//! - 커뮤니티: louvain, labelPropagation, connectedComponents
//! - 유사도: jaccard, cosine, overlap
//!
//! ## 그래프 엔진 접근 함수 (이 모듈에서 구현)
//! - type(r): 관계 타입 반환
//! - labels(n): 노드 라벨 목록 반환
//! - properties(n): 노드/관계 프로퍼티 맵 반환
//! - id(n): 노드/관계 ID 반환
//! - keys(n): 프로퍼티 키 목록 반환
//! - nodes(p): 경로의 노드 목록 반환
//! - relationships(p): 경로의 관계 목록 반환
//! - length(p): 경로 길이 반환

use indexmap::IndexMap;
use zeta4j_graph::core::{PropertyContainer, Value};
use zeta4j_graph::error::{GraphError, Result};
use zeta4j_graph::transaction::TransactionId;

use super::temporal::validate_arg_count;
use super::{ExecutionContext, QueryExecutor};
use zeta4j_cypher_core::ast::{ScalarFunction, ScalarFunctionCall};

impl QueryExecutor {
    /// 스칼라 함수 평가
    ///
    /// 스칼라 함수 호출을 평가하여 결과 값을 반환합니다.
    /// 함수 종류에 따라 적절한 모듈로 위임합니다.
    ///
    pub(super) async fn evaluate_scalar_function(
        &self,
        call: &ScalarFunctionCall,
        context: &ExecutionContext,
    ) -> Result<Value> {
        // 인자들 평가 (v1.12.0: async)
        let mut args = Vec::with_capacity(call.arguments.len());
        for arg in &call.arguments {
            args.push(self.evaluate_expression(arg, context).await?);
        }

        self.evaluate_scalar_function_values(&call.function, &args, Some(context.tx_id))
            .await
    }

    /// Evaluate an already-resolved scalar call while preserving the graph
    /// transaction used to resolve entity values.
    pub(super) async fn evaluate_scalar_function_values(
        &self,
        function: &ScalarFunction,
        args: &[Value],
        tx_id: Option<TransactionId>,
    ) -> Result<Value> {
        if is_pure_scalar_function(function) {
            return function.evaluate(args);
        }

        // 그래프 알고리즘 함수는 scalar_graph 모듈로 위임
        if is_graph_algorithm_function(function) {
            return self.evaluate_graph_algorithm_function(function, args).await;
        }

        // 시간/공간/벡터 함수는 functions 모듈로 위임
        if is_temporal_spatial_vector_function(function) {
            return function.evaluate(args);
        }

        // 그래프 엔진 접근이 필요한 함수들
        let tx_id = tx_id.ok_or_else(|| GraphError::System {
            message: format!("graph scalar function {function:?} requires a transaction"),
        })?;
        self.evaluate_engine_function(function, args, tx_id).await
    }

    /// 그래프 엔진 접근이 필요한 함수 평가
    async fn evaluate_engine_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
        tx_id: TransactionId,
    ) -> Result<Value> {
        match func {
            ScalarFunction::Type => self.eval_type(args, tx_id).await,
            ScalarFunction::Labels => self.eval_labels(args, tx_id).await,
            ScalarFunction::Properties => self.eval_properties(args, tx_id).await,
            ScalarFunction::Id => self.eval_id(args),
            ScalarFunction::Keys => self.eval_keys(args, tx_id).await,
            ScalarFunction::Nodes => self.eval_nodes(args),
            ScalarFunction::Relationships => self.eval_relationships(args),
            ScalarFunction::Length => self.eval_length(args),

            // early return으로 처리되어야 하는 함수들
            _ if is_pure_scalar_function(func) => {
                unreachable!("Pure functions should be handled by early return")
            }
            _ if is_temporal_spatial_vector_function(func) => {
                unreachable!("Temporal/spatial/vector functions should be handled by early return")
            }
            _ if is_graph_algorithm_function(func) => {
                unreachable!("Graph algorithm functions should be handled by early return")
            }

            #[allow(unreachable_patterns)]
            _ => Err(GraphError::NotImplemented {
                feature: format!("Scalar function: {:?}", func),
            }),
        }
    }

    // ========================================================================
    // 개별 함수 구현
    // ========================================================================

    /// type(r) - 관계 타입 반환
    async fn eval_type(&self, args: &[Value], tx_id: TransactionId) -> Result<Value> {
        validate_arg_count("type", args, 1)?;
        match &args[0] {
            Value::Edge(edge_id) => {
                if let Some(edge) = self.engine.get_edge_in_transaction(tx_id, *edge_id).await? {
                    Ok(Value::String(edge.relation_type().to_string()))
                } else {
                    Ok(Value::Null)
                }
            }
            Value::Null => Ok(Value::Null),
            _ => Err(GraphError::Validation {
                message: format!("type() expected Relationship, got {}", args[0].type_name()),
            }),
        }
    }

    /// labels(n) - 노드 라벨 목록 반환
    async fn eval_labels(&self, args: &[Value], tx_id: TransactionId) -> Result<Value> {
        validate_arg_count("labels", args, 1)?;
        match &args[0] {
            Value::Node(node_id) => {
                if let Some(node) = self.engine.get_node_in_transaction(tx_id, *node_id).await? {
                    // 전역 등록 순서대로 정렬
                    let raw_labels: Vec<String> = node.labels().iter().cloned().collect();
                    let sorted_labels = self.engine.sort_labels_by_order(&raw_labels);
                    let labels: Vec<Value> = sorted_labels.into_iter().map(Value::String).collect();
                    Ok(Value::Array(labels))
                } else {
                    Ok(Value::Null)
                }
            }
            Value::Null => Ok(Value::Null),
            _ => Err(GraphError::Validation {
                message: format!("labels() expected Node, got {}", args[0].type_name()),
            }),
        }
    }

    /// properties(n) - 노드/관계 프로퍼티 맵 반환
    async fn eval_properties(&self, args: &[Value], tx_id: TransactionId) -> Result<Value> {
        validate_arg_count("properties", args, 1)?;
        match &args[0] {
            Value::Node(node_id) => {
                if let Some(node) = self.engine.get_node_in_transaction(tx_id, *node_id).await? {
                    let mut map = IndexMap::new();
                    for (k, v) in node.properties().iter() {
                        map.insert(k.clone(), v.clone());
                    }
                    Ok(Value::Map(map))
                } else {
                    Ok(Value::Null)
                }
            }
            Value::Edge(edge_id) => {
                if let Some(edge) = self.engine.get_edge_in_transaction(tx_id, *edge_id).await? {
                    let mut map = IndexMap::new();
                    for (k, v) in edge.properties().iter() {
                        map.insert(k.clone(), v.clone());
                    }
                    Ok(Value::Map(map))
                } else {
                    Ok(Value::Null)
                }
            }
            Value::Null => Ok(Value::Null),
            _ => Err(GraphError::Validation {
                message: format!(
                    "properties() expected Node or Relationship, got {}",
                    args[0].type_name()
                ),
            }),
        }
    }

    /// id(n) - 노드/관계 ID 반환
    fn eval_id(&self, args: &[Value]) -> Result<Value> {
        validate_arg_count("id", args, 1)?;
        match &args[0] {
            Value::Node(node_id) => Ok(Value::Integer(*node_id as i64)),
            Value::Edge(edge_id) => Ok(Value::Integer(*edge_id as i64)),
            Value::Null => Ok(Value::Null),
            _ => Err(GraphError::Validation {
                message: format!(
                    "id() expected Node or Relationship, got {}",
                    args[0].type_name()
                ),
            }),
        }
    }

    /// keys(n) - 프로퍼티 키 목록 반환
    async fn eval_keys(&self, args: &[Value], tx_id: TransactionId) -> Result<Value> {
        validate_arg_count("keys", args, 1)?;
        match &args[0] {
            Value::Node(node_id) => {
                if let Some(node) = self.engine.get_node_in_transaction(tx_id, *node_id).await? {
                    // null 값 프로퍼티 제외, 삽입 순서 유지
                    let keys: Vec<Value> = node
                        .properties()
                        .iter()
                        .filter(|(_, v)| !matches!(v, Value::Null))
                        .map(|(k, _)| Value::String(k.clone()))
                        .collect();
                    Ok(Value::Array(keys))
                } else {
                    Ok(Value::Null)
                }
            }
            Value::Edge(edge_id) => {
                if let Some(edge) = self.engine.get_edge_in_transaction(tx_id, *edge_id).await? {
                    // null 값 프로퍼티 제외, 삽입 순서 유지
                    let keys: Vec<Value> = edge
                        .properties()
                        .iter()
                        .filter(|(_, v)| !matches!(v, Value::Null))
                        .map(|(k, _)| Value::String(k.clone()))
                        .collect();
                    Ok(Value::Array(keys))
                } else {
                    Ok(Value::Null)
                }
            }
            Value::Map(map) => {
                // null 값 프로퍼티 제외, 삽입 순서 유지
                let keys: Vec<Value> = map
                    .iter()
                    .filter(|(_, v)| !matches!(v, Value::Null))
                    .map(|(k, _)| Value::String(k.clone()))
                    .collect();
                Ok(Value::Array(keys))
            }
            Value::Null => Ok(Value::Null),
            _ => Err(GraphError::Validation {
                message: format!(
                    "keys() expected Node, Relationship or Map, got {}",
                    args[0].type_name()
                ),
            }),
        }
    }

    /// nodes(p) - 경로의 노드 목록 반환
    fn eval_nodes(&self, args: &[Value]) -> Result<Value> {
        validate_arg_count("nodes", args, 1)?;
        match &args[0] {
            Value::Path(path) => {
                // 경로의 노드 ID들을 Node 값으로 변환
                let nodes: Vec<Value> = path.nodes.iter().map(|&id| Value::Node(id)).collect();
                Ok(Value::Array(nodes))
            }
            Value::Null => Ok(Value::Null),
            _ => Err(GraphError::Validation {
                message: format!("nodes() expected Path, got {}", args[0].type_name()),
            }),
        }
    }

    /// relationships(p) - 경로의 관계 목록 반환
    fn eval_relationships(&self, args: &[Value]) -> Result<Value> {
        validate_arg_count("relationships", args, 1)?;
        match &args[0] {
            Value::Path(path) => {
                // 경로의 엣지 ID들을 Edge 값으로 변환
                let edges: Vec<Value> = path.edges.iter().map(|&id| Value::Edge(id)).collect();
                Ok(Value::Array(edges))
            }
            Value::Null => Ok(Value::Null),
            _ => Err(GraphError::Validation {
                message: format!("relationships() expected Path, got {}", args[0].type_name()),
            }),
        }
    }

    /// length(p) - 경로 길이 반환
    fn eval_length(&self, args: &[Value]) -> Result<Value> {
        validate_arg_count("length", args, 1)?;
        match &args[0] {
            Value::Path(path) => {
                // 경로 길이 = 엣지 수
                Ok(Value::Integer(path.len() as i64))
            }
            // length()는 문자열과 리스트에도 사용 가능 (size와 동일)
            Value::String(s) => Ok(Value::Integer(s.chars().count() as i64)),
            Value::Array(arr) => Ok(Value::Integer(arr.len() as i64)),
            Value::Null => Ok(Value::Null),
            _ => Err(GraphError::Validation {
                message: format!(
                    "length() expected Path, String, or List, got {}",
                    args[0].type_name()
                ),
            }),
        }
    }
}

// ============================================================================
// 함수 분류 헬퍼
// ============================================================================

/// 순수 스칼라 함수 (그래프 엔진 접근 불필요)
fn is_pure_scalar_function(func: &ScalarFunction) -> bool {
    matches!(
        func,
        // 문자열 함수
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
        | ScalarFunction::ToString
        // 수학 함수
        | ScalarFunction::Abs
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
        | ScalarFunction::E
        // 리스트 함수
        | ScalarFunction::Head
        | ScalarFunction::Tail
        | ScalarFunction::Last
        | ScalarFunction::Size
        | ScalarFunction::Range
        // 타입 변환 함수
        | ScalarFunction::ToInteger
        | ScalarFunction::ToFloat
        | ScalarFunction::ToBoolean
        | ScalarFunction::Coalesce
        | ScalarFunction::RandomUUID
        | ScalarFunction::Timestamp
    )
}

/// 시간/공간/벡터 함수
fn is_temporal_spatial_vector_function(func: &ScalarFunction) -> bool {
    matches!(
        func,
        // 시간 함수
        ScalarFunction::Date
        | ScalarFunction::Datetime
        | ScalarFunction::Time
        | ScalarFunction::LocalTime
        | ScalarFunction::LocalDateTime
        | ScalarFunction::Duration
        // 공간 함수
        | ScalarFunction::Point
        | ScalarFunction::Distance
        | ScalarFunction::WithinBBox
        // 벡터 유사도 함수
        | ScalarFunction::VectorCosineSimilarity
        | ScalarFunction::VectorEuclideanDistance
        | ScalarFunction::VectorDotProduct
        | ScalarFunction::VectorDistance
        | ScalarFunction::VectorDimensions
        | ScalarFunction::VectorNormalize
    )
}

/// 그래프 알고리즘 함수
fn is_graph_algorithm_function(func: &ScalarFunction) -> bool {
    matches!(
        func,
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
        | ScalarFunction::ModularityOptimization
        | ScalarFunction::LabelPropagation
        | ScalarFunction::Lpa
        | ScalarFunction::LpaAnalyze
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
        | ScalarFunction::Treewidth
    )
}
