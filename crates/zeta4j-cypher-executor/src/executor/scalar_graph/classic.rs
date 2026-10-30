//! 클래식 그래프 알고리즘 스칼라 함수
//!
//! MST, Topological Sort, Max Flow, Bipartite, SCC,
//! Coloring, Eulerian, Hamiltonian, Clique, Vertex Cover,
//! Independent Set, Dominating Set, Feedback Set, Isomorphism,
//! Steiner Tree, Tree Decomposition 등

use super::super::{QueryExecutor, graph_algorithms};
use indexmap::IndexMap;
use zeta4j_cypher_core::ast::ScalarFunction;
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    pub(super) async fn evaluate_classic_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
    ) -> Result<Value> {
        match func {
            // === MST 함수 ===
            ScalarFunction::Mst => {
                // mst(startNode?) - Prim's MST
                // 반환: Map { edges, totalWeight, nodeCount, algorithm }
                use zeta4j_graph::algorithm::mst::PrimConfig;

                let config = if !args.is_empty() {
                    let start_node = match &args[0] {
                        Value::Node(id) => *id,
                        Value::Integer(id) => *id as u64,
                        Value::Null => {
                            // Null이면 기본값 사용
                            return self.run_prim_mst(PrimConfig::default()).await;
                        }
                        _ => {
                            return Err(GraphError::Validation {
                                message: "mst() argument must be a Node or Integer".to_string(),
                            });
                        }
                    };
                    PrimConfig::default().with_start_node(start_node)
                } else {
                    PrimConfig::default()
                };

                self.run_prim_mst(config).await
            }

            ScalarFunction::KruskalMst => {
                // kruskal() - Kruskal's MST
                use zeta4j_graph::algorithm::mst::Kruskal;

                let kruskal = Kruskal::new(&self.engine);
                let result = kruskal.run().await?;

                graph_algorithms::mst_result_to_value(result)
            }

            ScalarFunction::SpanningForest => {
                // spanningForest() - Spanning Forest for disconnected graphs
                use zeta4j_graph::algorithm::mst::Kruskal;

                let kruskal = Kruskal::new(&self.engine);
                let forest = kruskal.spanning_forest().await?;

                // 각 트리를 배열로 변환
                let trees: Vec<Value> = forest
                    .trees
                    .iter()
                    .map(|tree| {
                        let mut map = IndexMap::new();
                        let edges: Vec<Value> = tree
                            .edges
                            .iter()
                            .map(|(from, to, weight)| {
                                let mut edge_map = IndexMap::new();
                                edge_map.insert("from".to_string(), Value::Integer(*from as i64));
                                edge_map.insert("to".to_string(), Value::Integer(*to as i64));
                                edge_map.insert("weight".to_string(), Value::Float(*weight));
                                Value::Map(edge_map)
                            })
                            .collect();
                        map.insert("edges".to_string(), Value::Array(edges));
                        map.insert("totalWeight".to_string(), Value::Float(tree.total_weight));
                        map.insert(
                            "nodeCount".to_string(),
                            Value::Integer(tree.node_count as i64),
                        );
                        Value::Map(map)
                    })
                    .collect();

                let mut result = IndexMap::new();
                result.insert("trees".to_string(), Value::Array(trees));
                result.insert("totalWeight".to_string(), Value::Float(forest.total_weight));
                result.insert(
                    "totalNodes".to_string(),
                    Value::Integer(forest.total_nodes as i64),
                );
                result.insert(
                    "treeCount".to_string(),
                    Value::Integer(forest.tree_count as i64),
                );

                Ok(Value::Map(result))
            }

            // === Topological Sort 함수 ===
            ScalarFunction::HasCycle => {
                // hasCycle() - 사이클 존재 여부
                use zeta4j_graph::algorithm::topological::has_cycle;
                let result = has_cycle(&self.engine).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::IsDAG => {
                // isDAG() - DAG 여부 (사이클 없음)
                use zeta4j_graph::algorithm::topological::is_dag;
                let result = is_dag(&self.engine).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::FindCycle => {
                // findCycle() - 사이클 탐지 및 경로 반환
                use zeta4j_graph::algorithm::topological::find_cycle;
                let result = find_cycle(&self.engine).await?;

                let mut map = IndexMap::new();
                map.insert("hasCycle".to_string(), Value::Boolean(result.has_cycle));

                if let Some(path) = &result.cycle_path {
                    let path_values: Vec<Value> =
                        path.iter().map(|&id| Value::Integer(id as i64)).collect();
                    map.insert("cyclePath".to_string(), Value::Array(path_values));
                } else {
                    map.insert("cyclePath".to_string(), Value::Null);
                }

                if let Some((from, to)) = result.back_edge {
                    let mut edge_map = IndexMap::new();
                    edge_map.insert("from".to_string(), Value::Integer(from as i64));
                    edge_map.insert("to".to_string(), Value::Integer(to as i64));
                    map.insert("backEdge".to_string(), Value::Map(edge_map));
                } else {
                    map.insert("backEdge".to_string(), Value::Null);
                }

                map.insert(
                    "cycleLength".to_string(),
                    Value::Integer(result.cycle_length() as i64),
                );

                Ok(Value::Map(map))
            }

            ScalarFunction::TopoSort => {
                // topoSort() - 위상 정렬 (Kahn's algorithm)
                use zeta4j_graph::algorithm::topological::topological_sort;
                let result = topological_sort(&self.engine).await?;

                graph_algorithms::topological_result_to_value(result)
            }

            ScalarFunction::TopoSortDfs => {
                // topoSortDfs() - DFS 기반 위상 정렬
                use zeta4j_graph::algorithm::topological::dfs_topological_sort;
                let result = dfs_topological_sort(&self.engine).await?;

                graph_algorithms::topological_result_to_value(result)
            }

            ScalarFunction::DagSources => {
                // dagSources() - 시작 노드들 (진입차수 0)
                use zeta4j_graph::algorithm::topological::dag_sources;
                let sources = dag_sources(&self.engine).await?;

                let values: Vec<Value> = sources
                    .iter()
                    .map(|&id| Value::Integer(id as i64))
                    .collect();
                Ok(Value::Array(values))
            }

            ScalarFunction::DagSinks => {
                // dagSinks() - 종료 노드들 (진출차수 0)
                use zeta4j_graph::algorithm::topological::dag_sinks;
                let sinks = dag_sinks(&self.engine).await?;

                let values: Vec<Value> =
                    sinks.iter().map(|&id| Value::Integer(id as i64)).collect();
                Ok(Value::Array(values))
            }

            ScalarFunction::DagLevels => {
                // dagLevels() - 레벨별 노드 그룹화
                use zeta4j_graph::algorithm::topological::topological_sort_with_levels;
                let result = topological_sort_with_levels(&self.engine).await?;

                if let Some(levels) = result.levels {
                    let level_values: Vec<Value> = levels
                        .iter()
                        .map(|level| {
                            let nodes: Vec<Value> =
                                level.iter().map(|&id| Value::Integer(id as i64)).collect();
                            Value::Array(nodes)
                        })
                        .collect();
                    Ok(Value::Array(level_values))
                } else {
                    Ok(Value::Array(vec![]))
                }
            }

            // === Maximum Flow 함수 ===
            ScalarFunction::MaxFlow => {
                // maxFlow(source, sink, capacityProp?) - 최대 유량 (Edmonds-Karp)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "maxFlow requires at least 2 arguments: source and sink"
                            .to_string(),
                    });
                }

                let source = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "maxFlow() source must be a Node or Integer".to_string(),
                        });
                    }
                };
                let sink = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "maxFlow() sink must be a Node or Integer".to_string(),
                        });
                    }
                };

                let result = if args.len() > 2 {
                    if let Value::String(prop) = &args[2] {
                        use zeta4j_graph::algorithm::flow::max_flow_with_capacity;
                        max_flow_with_capacity(&self.engine, source, sink, prop).await?
                    } else {
                        use zeta4j_graph::algorithm::flow::max_flow;
                        max_flow(&self.engine, source, sink).await?
                    }
                } else {
                    use zeta4j_graph::algorithm::flow::max_flow;
                    max_flow(&self.engine, source, sink).await?
                };

                graph_algorithms::max_flow_result_to_value(result)
            }

            ScalarFunction::MaxFlowPR => {
                // maxFlowPR(source, sink, capacityProp?) - 최대 유량 (Push-Relabel)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "maxFlowPR requires at least 2 arguments: source and sink"
                            .to_string(),
                    });
                }

                let source = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "maxFlowPR() source must be a Node or Integer".to_string(),
                        });
                    }
                };
                let sink = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "maxFlowPR() sink must be a Node or Integer".to_string(),
                        });
                    }
                };

                let result = if args.len() > 2 {
                    if let Value::String(prop) = &args[2] {
                        use zeta4j_graph::algorithm::flow::max_flow_pr_with_capacity;
                        max_flow_pr_with_capacity(&self.engine, source, sink, prop).await?
                    } else {
                        use zeta4j_graph::algorithm::flow::max_flow_pr;
                        max_flow_pr(&self.engine, source, sink).await?
                    }
                } else {
                    use zeta4j_graph::algorithm::flow::max_flow_pr;
                    max_flow_pr(&self.engine, source, sink).await?
                };

                graph_algorithms::max_flow_result_to_value(result)
            }

            ScalarFunction::MinCut => {
                // minCut(source, sink, capacityProp?) - 최소 컷
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "minCut requires at least 2 arguments: source and sink"
                            .to_string(),
                    });
                }

                let source = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "minCut() source must be a Node or Integer".to_string(),
                        });
                    }
                };
                let sink = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "minCut() sink must be a Node or Integer".to_string(),
                        });
                    }
                };

                let result = if args.len() > 2 {
                    if let Value::String(prop) = &args[2] {
                        use zeta4j_graph::algorithm::flow::min_cut_with_capacity;
                        min_cut_with_capacity(&self.engine, source, sink, prop).await?
                    } else {
                        use zeta4j_graph::algorithm::flow::min_cut;
                        min_cut(&self.engine, source, sink).await?
                    }
                } else {
                    use zeta4j_graph::algorithm::flow::min_cut;
                    min_cut(&self.engine, source, sink).await?
                };

                graph_algorithms::min_cut_result_to_value(result)
            }

            // === Bipartite & Matching 함수 ===
            ScalarFunction::IsBipartite => {
                // isBipartite()
                use zeta4j_graph::algorithm::matching::is_bipartite;
                let is_bipartite = is_bipartite(&self.engine).await?;
                Ok(Value::Boolean(is_bipartite))
            }

            ScalarFunction::BipartitePartition => {
                // bipartitePartition()
                use zeta4j_graph::algorithm::matching::bipartite_partition;
                let result = bipartite_partition(&self.engine).await?;
                graph_algorithms::bipartite_result_to_value(result)
            }

            ScalarFunction::MaxMatching => {
                // maxMatching() - Hopcroft-Karp
                use zeta4j_graph::algorithm::matching::hopcroft_karp;
                let result = hopcroft_karp(&self.engine).await?;
                graph_algorithms::matching_result_to_value(result)
            }

            ScalarFunction::MaxMatchingKuhn => {
                // maxMatchingKuhn() - Kuhn's Algorithm
                use zeta4j_graph::algorithm::matching::kuhn_matching;
                let result = kuhn_matching(&self.engine).await?;
                graph_algorithms::matching_result_to_value(result)
            }

            ScalarFunction::MinCostMatching => {
                // minCostMatching(weightProp?)
                use zeta4j_graph::algorithm::matching::{hungarian, min_cost_matching};

                let result = if !args.is_empty() {
                    if let Value::String(prop) = &args[0] {
                        min_cost_matching(&self.engine, prop).await?
                    } else {
                        hungarian(&self.engine).await?
                    }
                } else {
                    hungarian(&self.engine).await?
                };

                graph_algorithms::hungarian_result_to_value(result)
            }

            ScalarFunction::MaxWeightMatching => {
                // maxWeightMatching(weightProp?)
                use zeta4j_graph::algorithm::matching::max_weight_matching;

                let weight_prop = if !args.is_empty() {
                    if let Value::String(prop) = &args[0] {
                        prop.clone()
                    } else {
                        "weight".to_string()
                    }
                } else {
                    "weight".to_string()
                };

                let result = max_weight_matching(&self.engine, &weight_prop).await?;
                graph_algorithms::hungarian_result_to_value(result)
            }

            // === SCC 함수 ===
            ScalarFunction::Scc => {
                // scc() - 모든 SCC 반환
                use zeta4j_graph::algorithm::scc::tarjan_scc;
                let result = tarjan_scc(&self.engine).await?;
                graph_algorithms::scc_result_to_value(result)
            }

            ScalarFunction::SccCount => {
                // sccCount() - SCC 개수
                use zeta4j_graph::algorithm::scc::scc_count;
                let count = scc_count(&self.engine).await?;
                Ok(Value::Integer(count as i64))
            }

            ScalarFunction::SccOf => {
                // sccOf(nodeId) - 노드의 SCC
                use zeta4j_graph::algorithm::scc::tarjan_scc;

                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "sccOf() requires a node ID argument".to_string(),
                    });
                }

                let node_id = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "sccOf() argument must be an integer node ID".to_string(),
                        });
                    }
                };

                let result = tarjan_scc(&self.engine).await?;
                if let Some(scc_nodes) = result.scc_nodes_of(node_id) {
                    let nodes: Vec<Value> = scc_nodes
                        .iter()
                        .map(|&n| Value::Integer(n as i64))
                        .collect();
                    Ok(Value::Array(nodes))
                } else {
                    Ok(Value::Null)
                }
            }

            ScalarFunction::IsStronglyConnected => {
                // isStronglyConnected() - 강연결 여부
                use zeta4j_graph::algorithm::scc::is_strongly_connected;
                let is_sc = is_strongly_connected(&self.engine).await?;
                Ok(Value::Boolean(is_sc))
            }

            ScalarFunction::LargestScc => {
                // largestScc() - 가장 큰 SCC
                use zeta4j_graph::algorithm::scc::largest_scc;
                let nodes = largest_scc(&self.engine).await?;
                let values: Vec<Value> = nodes.iter().map(|&n| Value::Integer(n as i64)).collect();
                Ok(Value::Array(values))
            }

            ScalarFunction::CondensationGraph => {
                // condensationGraph() - 압축 그래프
                use zeta4j_graph::algorithm::scc::condensation_graph;
                let result = condensation_graph(&self.engine).await?;
                graph_algorithms::condensation_to_value(result)
            }

            // === Graph Coloring 함수 ===
            ScalarFunction::GraphColoring => {
                // graphColoring() - DSatur 색칠
                use zeta4j_graph::algorithm::coloring::graph_coloring;
                let result = graph_coloring(&self.engine).await?;
                graph_algorithms::coloring_result_to_value(result)
            }

            ScalarFunction::GreedyColoring => {
                // greedyColoring() - Greedy 색칠
                use zeta4j_graph::algorithm::coloring::greedy_coloring;
                let result = greedy_coloring(&self.engine).await?;
                graph_algorithms::coloring_result_to_value(result)
            }

            ScalarFunction::WelshPowellColoring => {
                // welshPowellColoring() - Welsh-Powell 색칠
                use zeta4j_graph::algorithm::coloring::welsh_powell;
                let result = welsh_powell(&self.engine).await?;
                graph_algorithms::coloring_result_to_value(result)
            }

            ScalarFunction::ChromaticNumber => {
                // chromaticNumber() - 색상 수
                use zeta4j_graph::algorithm::coloring::chromatic_number;
                let num = chromatic_number(&self.engine).await?;
                Ok(Value::Integer(num as i64))
            }

            ScalarFunction::ColorOf => {
                // colorOf(nodeId) - 노드의 색상
                use zeta4j_graph::algorithm::coloring::graph_coloring;

                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "colorOf() requires a node ID argument".to_string(),
                    });
                }

                let node_id = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "colorOf() argument must be an integer node ID".to_string(),
                        });
                    }
                };

                let result = graph_coloring(&self.engine).await?;
                match result.color_of(node_id) {
                    Some(color) => Ok(Value::Integer(color as i64)),
                    None => Ok(Value::Null),
                }
            }

            ScalarFunction::IsKColorable => {
                // isKColorable(k) - k-색칠 가능 여부
                use zeta4j_graph::algorithm::coloring::is_k_colorable;

                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "isKColorable() requires a k argument".to_string(),
                    });
                }

                let k = match &args[0] {
                    Value::Integer(k) => *k as usize,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "isKColorable() argument must be an integer".to_string(),
                        });
                    }
                };

                let is_colorable = is_k_colorable(&self.engine, k).await?;
                Ok(Value::Boolean(is_colorable))
            }

            // === Eulerian Path/Circuit 함수 ===
            ScalarFunction::HasEulerianCircuit => {
                use zeta4j_graph::algorithm::eulerian::has_eulerian_circuit;
                let result = has_eulerian_circuit(&self.engine).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::HasEulerianPath => {
                use zeta4j_graph::algorithm::eulerian::has_eulerian_path;
                let result = has_eulerian_path(&self.engine).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::EulerianCheck => {
                use zeta4j_graph::algorithm::eulerian::eulerian_check;
                let result = eulerian_check(&self.engine).await?;
                graph_algorithms::eulerian_check_result_to_value(result)
            }

            ScalarFunction::EulerianCircuit => {
                use zeta4j_graph::algorithm::eulerian::eulerian_circuit;
                match eulerian_circuit(&self.engine).await? {
                    Some(result) => graph_algorithms::eulerian_result_to_value(result),
                    None => Ok(Value::Null),
                }
            }

            ScalarFunction::EulerianPath => {
                use zeta4j_graph::algorithm::eulerian::eulerian_path;
                match eulerian_path(&self.engine).await? {
                    Some(result) => graph_algorithms::eulerian_result_to_value(result),
                    None => Ok(Value::Null),
                }
            }

            // === Hamiltonian Path/Circuit 함수 ===
            ScalarFunction::HasHamiltonianPath => {
                use zeta4j_graph::algorithm::hamiltonian::has_hamiltonian_path;
                let result = has_hamiltonian_path(&self.engine).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::HasHamiltonianCircuit => {
                use zeta4j_graph::algorithm::hamiltonian::has_hamiltonian_circuit;
                let result = has_hamiltonian_circuit(&self.engine).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::HamiltonianPath => {
                use zeta4j_graph::algorithm::hamiltonian::hamiltonian_path;
                match hamiltonian_path(&self.engine).await? {
                    Some(result) => graph_algorithms::hamiltonian_result_to_value(result),
                    None => Ok(Value::Null),
                }
            }

            ScalarFunction::HamiltonianCircuit => {
                use zeta4j_graph::algorithm::hamiltonian::hamiltonian_circuit;
                match hamiltonian_circuit(&self.engine).await? {
                    Some(result) => graph_algorithms::hamiltonian_result_to_value(result),
                    None => Ok(Value::Null),
                }
            }

            // === Clique Detection 함수 ===
            ScalarFunction::IsClique => {
                use zeta4j_graph::algorithm::clique::is_clique;
                // 인자: 노드 ID 리스트
                let nodes: Vec<u64> = args
                    .iter()
                    .filter_map(|v| match v {
                        Value::Integer(id) => Some(*id as u64),
                        _ => None,
                    })
                    .collect();
                let result = is_clique(&self.engine, &nodes).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::FindCliquesOfSize => {
                use zeta4j_graph::algorithm::clique::find_cliques_of_size;
                let k = match args.first() {
                    Some(Value::Integer(k)) => *k as usize,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "findCliquesOfSize requires an integer argument".to_string(),
                        });
                    }
                };
                let cliques = find_cliques_of_size(&self.engine, k).await?;
                let result: Vec<Value> = cliques
                    .iter()
                    .map(|clique| {
                        Value::Array(clique.iter().map(|&id| Value::Integer(id as i64)).collect())
                    })
                    .collect();
                Ok(Value::Array(result))
            }

            ScalarFunction::FindAllMaximalCliques => {
                use zeta4j_graph::algorithm::clique::find_all_maximal_cliques;
                let result = find_all_maximal_cliques(&self.engine).await?;
                graph_algorithms::clique_result_to_value(result)
            }

            ScalarFunction::FindMaximumClique => {
                use zeta4j_graph::algorithm::clique::find_maximum_clique;
                let clique = find_maximum_clique(&self.engine).await?;
                Ok(Value::Array(
                    clique.iter().map(|&id| Value::Integer(id as i64)).collect(),
                ))
            }

            ScalarFunction::CliqueNumber => {
                use zeta4j_graph::algorithm::clique::clique_number;
                let result = clique_number(&self.engine).await?;
                Ok(Value::Integer(result as i64))
            }

            // === Vertex Cover 함수 ===
            ScalarFunction::IsVertexCover => {
                use zeta4j_graph::algorithm::vertex_cover::is_vertex_cover;
                // 인자: 노드 ID 리스트
                let nodes: Vec<u64> = args
                    .iter()
                    .filter_map(|v| match v {
                        Value::Integer(id) => Some(*id as u64),
                        _ => None,
                    })
                    .collect();
                let result = is_vertex_cover(&self.engine, &nodes).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::GreedyVertexCover => {
                use zeta4j_graph::algorithm::vertex_cover::greedy_vertex_cover;
                let cover = greedy_vertex_cover(&self.engine).await?;
                Ok(Value::Array(
                    cover.iter().map(|&id| Value::Integer(id as i64)).collect(),
                ))
            }

            ScalarFunction::MinimumVertexCover => {
                use zeta4j_graph::algorithm::vertex_cover::minimum_vertex_cover;
                let cover = minimum_vertex_cover(&self.engine).await?;
                Ok(Value::Array(
                    cover.iter().map(|&id| Value::Integer(id as i64)).collect(),
                ))
            }

            ScalarFunction::HasVertexCoverOfSize => {
                use zeta4j_graph::algorithm::vertex_cover::has_vertex_cover_of_size;
                let k = match args.first() {
                    Some(Value::Integer(k)) => *k as usize,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "hasVertexCoverOfSize requires an integer argument"
                                .to_string(),
                        });
                    }
                };
                let result = has_vertex_cover_of_size(&self.engine, k).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::VertexCoverNumber => {
                use zeta4j_graph::algorithm::vertex_cover::vertex_cover_number;
                let result = vertex_cover_number(&self.engine).await?;
                Ok(Value::Integer(result as i64))
            }

            // === Independent Set 함수 ===
            ScalarFunction::IsIndependentSet => {
                use zeta4j_graph::algorithm::independent_set::is_independent_set;
                // 인자: 노드 ID 리스트
                let nodes: Vec<u64> = args
                    .iter()
                    .filter_map(|v| match v {
                        Value::Integer(id) => Some(*id as u64),
                        _ => None,
                    })
                    .collect();
                let result = is_independent_set(&self.engine, &nodes).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::GreedyIndependentSet => {
                use zeta4j_graph::algorithm::independent_set::greedy_independent_set;
                let set = greedy_independent_set(&self.engine).await?;
                Ok(Value::Array(
                    set.iter().map(|&id| Value::Integer(id as i64)).collect(),
                ))
            }

            ScalarFunction::MaximumIndependentSet => {
                use zeta4j_graph::algorithm::independent_set::maximum_independent_set;
                let set = maximum_independent_set(&self.engine).await?;
                Ok(Value::Array(
                    set.iter().map(|&id| Value::Integer(id as i64)).collect(),
                ))
            }

            ScalarFunction::HasIndependentSetOfSize => {
                use zeta4j_graph::algorithm::independent_set::has_independent_set_of_size;
                let k = match args.first() {
                    Some(Value::Integer(k)) => *k as usize,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "hasIndependentSetOfSize requires an integer argument"
                                .to_string(),
                        });
                    }
                };
                let result = has_independent_set_of_size(&self.engine, k).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::IndependenceNumber => {
                use zeta4j_graph::algorithm::independent_set::independence_number;
                let result = independence_number(&self.engine).await?;
                Ok(Value::Integer(result as i64))
            }

            ScalarFunction::FindAllMaximalIndependentSets => {
                use zeta4j_graph::algorithm::independent_set::find_all_maximal_independent_sets;
                let result = find_all_maximal_independent_sets(&self.engine).await?;
                graph_algorithms::independent_set_result_to_value(result)
            }

            // === Dominating Set 함수 ===
            ScalarFunction::IsDominatingSet => {
                use zeta4j_graph::algorithm::dominating_set::is_dominating_set;
                // 인자: 노드 ID 리스트
                let nodes: Vec<u64> = args
                    .iter()
                    .filter_map(|v| match v {
                        Value::Integer(id) => Some(*id as u64),
                        _ => None,
                    })
                    .collect();
                let result = is_dominating_set(&self.engine, &nodes).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::GreedyDominatingSet => {
                use zeta4j_graph::algorithm::dominating_set::greedy_dominating_set;
                let result = greedy_dominating_set(&self.engine).await?;
                let nodes: Vec<Value> =
                    result.iter().map(|&id| Value::Integer(id as i64)).collect();
                Ok(Value::Array(nodes))
            }

            ScalarFunction::MinimumDominatingSet => {
                use zeta4j_graph::algorithm::dominating_set::MinimumDominatingSet;
                let finder = MinimumDominatingSet::new(&self.engine);
                let result = finder.minimum_dominating_set().await?;
                graph_algorithms::dominating_set_result_to_value(result)
            }

            ScalarFunction::HasDominatingSetOfSize => {
                use zeta4j_graph::algorithm::dominating_set::has_dominating_set_of_size;
                let k = match args.first() {
                    Some(Value::Integer(k)) => *k as usize,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "hasDominatingSetOfSize requires an integer argument"
                                .to_string(),
                        });
                    }
                };
                let result = has_dominating_set_of_size(&self.engine, k).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::DominationNumber => {
                use zeta4j_graph::algorithm::dominating_set::domination_number;
                let result = domination_number(&self.engine).await?;
                Ok(Value::Integer(result as i64))
            }

            ScalarFunction::ConnectedDominatingSet => {
                use zeta4j_graph::algorithm::dominating_set::connected_dominating_set;
                let result = connected_dominating_set(&self.engine).await?;
                let nodes: Vec<Value> =
                    result.iter().map(|&id| Value::Integer(id as i64)).collect();
                Ok(Value::Array(nodes))
            }

            // === Feedback Set 함수 ===
            ScalarFunction::IsFeedbackVertexSet => {
                use zeta4j_graph::algorithm::feedback::is_feedback_vertex_set;
                // 인자: 노드 ID 리스트
                let nodes: Vec<u64> = args
                    .iter()
                    .filter_map(|v| match v {
                        Value::Integer(id) => Some(*id as u64),
                        _ => None,
                    })
                    .collect();
                let result = is_feedback_vertex_set(&self.engine, &nodes).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::IsFeedbackArcSet => {
                use zeta4j_graph::algorithm::feedback::is_feedback_arc_set;
                // 인자: [[from, to], ...] 형태의 엣지 리스트
                let edges: Vec<(u64, u64)> = args
                    .iter()
                    .filter_map(|v| match v {
                        Value::Array(arr) if arr.len() == 2 => match (&arr[0], &arr[1]) {
                            (Value::Integer(from), Value::Integer(to)) => {
                                Some((*from as u64, *to as u64))
                            }
                            _ => None,
                        },
                        _ => None,
                    })
                    .collect();
                let result = is_feedback_arc_set(&self.engine, &edges).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::GreedyFVS => {
                use zeta4j_graph::algorithm::feedback::greedy_fvs;
                let result = greedy_fvs(&self.engine).await?;
                let nodes: Vec<Value> =
                    result.iter().map(|&id| Value::Integer(id as i64)).collect();
                Ok(Value::Array(nodes))
            }

            ScalarFunction::GreedyFAS => {
                use zeta4j_graph::algorithm::feedback::greedy_fas;
                let result = greedy_fas(&self.engine).await?;
                let edges: Vec<Value> = result
                    .iter()
                    .map(|(from, to)| {
                        Value::Array(vec![
                            Value::Integer(*from as i64),
                            Value::Integer(*to as i64),
                        ])
                    })
                    .collect();
                Ok(Value::Array(edges))
            }

            ScalarFunction::MinimumFVS => {
                use zeta4j_graph::algorithm::feedback::MinimumFeedbackSet;
                let finder = MinimumFeedbackSet::new(&self.engine);
                let result = finder.minimum_fvs().await?;
                graph_algorithms::feedback_vertex_set_result_to_value(result)
            }

            ScalarFunction::MinimumFAS => {
                use zeta4j_graph::algorithm::feedback::MinimumFeedbackSet;
                let finder = MinimumFeedbackSet::new(&self.engine);
                let result = finder.minimum_fas().await?;
                graph_algorithms::feedback_arc_set_result_to_value(result)
            }

            ScalarFunction::HasFVSOfSize => {
                use zeta4j_graph::algorithm::feedback::has_fvs_of_size;
                let k = match args.first() {
                    Some(Value::Integer(k)) => *k as usize,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "hasFVSOfSize requires an integer argument".to_string(),
                        });
                    }
                };
                let result = has_fvs_of_size(&self.engine, k).await?;
                Ok(Value::Boolean(result))
            }

            ScalarFunction::HasFASOfSize => {
                use zeta4j_graph::algorithm::feedback::has_fas_of_size;
                let k = match args.first() {
                    Some(Value::Integer(k)) => *k as usize,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "hasFASOfSize requires an integer argument".to_string(),
                        });
                    }
                };
                let result = has_fas_of_size(&self.engine, k).await?;
                Ok(Value::Boolean(result))
            }

            // === Graph Isomorphism 함수 ===
            ScalarFunction::IsIsomorphic => {
                // 두 그래프 비교는 현재 단일 엔진 환경에서 지원하지 않음
                Err(GraphError::Validation {
                    message: "isIsomorphic requires two separate graphs. Use wlHash() for single graph fingerprinting.".to_string()
                })
            }

            ScalarFunction::IsSubgraphIsomorphic => {
                // 서브그래프 매칭은 현재 단일 엔진 환경에서 지원하지 않음
                Err(GraphError::Validation {
                    message: "isSubgraphIsomorphic requires pattern and target graphs. Use find_all_subgraphs from Rust API.".to_string()
                })
            }

            ScalarFunction::WLHash => {
                use zeta4j_graph::algorithm::isomorphism::wl_hash;
                let hash = wl_hash(&self.engine).await?;
                Ok(Value::String(hash))
            }

            ScalarFunction::ArePotentiallyIsomorphic => {
                // 두 그래프 비교는 현재 단일 엔진 환경에서 지원하지 않음
                Err(GraphError::Validation {
                    message: "arePotentiallyIsomorphic requires two separate graphs. Use wlHash() to compare graph fingerprints.".to_string()
                })
            }

            // === Steiner Tree 알고리즘 ===
            ScalarFunction::SteinerTree => {
                // steinerTree([terminals]) - 최소 Steiner Tree (Dreyfus-Wagner)
                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "steinerTree() requires a list of terminal node IDs".to_string(),
                    });
                }
                let terminals = graph_algorithms::extract_node_id_list(&args[0])?;
                use zeta4j_graph::algorithm::steiner::steiner_tree;
                let result = steiner_tree(&self.engine, &terminals).await?;
                graph_algorithms::steiner_result_to_value(result)
            }

            ScalarFunction::SteinerTreeHeuristic => {
                // steinerTreeHeuristic([terminals]) - MST 휴리스틱 2-근사
                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "steinerTreeHeuristic() requires a list of terminal node IDs"
                            .to_string(),
                    });
                }
                let terminals = graph_algorithms::extract_node_id_list(&args[0])?;
                use zeta4j_graph::algorithm::steiner::steiner_tree_heuristic;
                let result = steiner_tree_heuristic(&self.engine, &terminals).await?;
                graph_algorithms::steiner_result_to_value(result)
            }

            ScalarFunction::SteinerCost => {
                // steinerCost([terminals]) - Steiner Tree 비용
                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "steinerCost() requires a list of terminal node IDs".to_string(),
                    });
                }
                let terminals = graph_algorithms::extract_node_id_list(&args[0])?;
                use zeta4j_graph::algorithm::steiner::steiner_cost;
                let cost = steiner_cost(&self.engine, &terminals).await?;
                Ok(Value::Float(cost))
            }

            ScalarFunction::IsSteinerTree => {
                // isSteinerTree(edges, terminals) - Steiner Tree 검증
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "isSteinerTree() requires edges and terminals lists".to_string(),
                    });
                }
                let edges = graph_algorithms::extract_weighted_edge_list(&args[0])?;
                let terminals = graph_algorithms::extract_node_id_list(&args[1])?;
                use zeta4j_graph::algorithm::steiner::is_steiner_tree;
                let result = is_steiner_tree(&edges, &terminals);
                Ok(Value::Boolean(result))
            }

            ScalarFunction::SteinerVertices => {
                // steinerVertices(edges, terminals) - Steiner 정점 추출
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "steinerVertices() requires edges and terminals lists".to_string(),
                    });
                }
                let edges = graph_algorithms::extract_weighted_edge_list(&args[0])?;
                let terminals = graph_algorithms::extract_node_id_list(&args[1])?;
                use zeta4j_graph::algorithm::steiner::find_steiner_vertices;
                let vertices = find_steiner_vertices(&edges, &terminals);
                let list: Vec<Value> = vertices.iter().map(|&v| Value::Integer(v as i64)).collect();
                Ok(Value::Array(list))
            }

            // === Tree Decomposition 알고리즘 ===
            ScalarFunction::TreeDecomposition => {
                // treeDecomposition(heuristic?) - 트리 분해 계산
                use zeta4j_graph::algorithm::tree_decomposition::{Heuristic, tree_decomposition};

                let heuristic = if !args.is_empty() {
                    match &args[0] {
                        Value::String(s) if s.to_lowercase() == "min_fill" => Heuristic::MinFill,
                        Value::String(s) if s.to_lowercase() == "minfill" => Heuristic::MinFill,
                        _ => Heuristic::MinDegree,
                    }
                } else {
                    Heuristic::MinDegree
                };

                let result = tree_decomposition(&self.engine, heuristic).await?;
                graph_algorithms::tree_decomposition_result_to_value(result)
            }

            ScalarFunction::Treewidth => {
                // treewidth() - 트리 너비 계산
                use zeta4j_graph::algorithm::tree_decomposition::treewidth;
                let width = treewidth(&self.engine).await?;
                Ok(Value::Integer(width as i64))
            }

            _ => unreachable!("Non-classic function passed to evaluate_classic_function"),
        }
    }
}
