//! 그래프 알고리즘 결과 변환 모듈
//!
//! 그래프 알고리즘 실행 결과를 Value 타입으로 변환하는 독립 함수들.
//! executor의 결과 변환 로직을 모듈화하여 코드 가독성 향상.

use indexmap::IndexMap;
use zeta4j_graph::core::{Path, Value};
use zeta4j_graph::error::Result;

// ============================================================================
// 헬퍼 함수들
// ============================================================================

/// 노드 ID 추출 헬퍼 함수
pub fn extract_node_id_list(value: &Value) -> Result<Vec<u64>> {
    use zeta4j_graph::error::GraphError;
    match value {
        Value::Array(arr) => {
            let mut ids = Vec::new();
            for v in arr {
                match v {
                    Value::Integer(i) => ids.push(*i as u64),
                    Value::Node(id) => ids.push(*id),
                    _ => {
                        return Err(GraphError::Validation {
                            message: format!("Expected integer or node, got {:?}", v),
                        });
                    }
                }
            }
            Ok(ids)
        }
        Value::Integer(i) => Ok(vec![*i as u64]),
        Value::Node(id) => Ok(vec![*id]),
        _ => Err(GraphError::Validation {
            message: format!("Expected array, integer, or node, got {:?}", value),
        }),
    }
}

/// 가중치 엣지 리스트 추출
pub fn extract_weighted_edge_list(value: &Value) -> Result<Vec<(u64, u64, f64)>> {
    use zeta4j_graph::error::GraphError;
    match value {
        Value::Array(arr) => {
            let mut edges = Vec::new();
            for v in arr {
                match v {
                    Value::Map(map) => {
                        let from = extract_node_id_from_map(map, "from")?;
                        let to = extract_node_id_from_map(map, "to")?;
                        let weight = match map.get("weight") {
                            Some(Value::Float(w)) => *w,
                            Some(Value::Integer(w)) => *w as f64,
                            _ => 1.0,
                        };
                        edges.push((from, to, weight));
                    }
                    Value::Array(edge_arr) if edge_arr.len() >= 2 => {
                        let from = match &edge_arr[0] {
                            Value::Integer(i) => *i as u64,
                            Value::Node(id) => *id,
                            _ => {
                                return Err(GraphError::Validation {
                                    message: "Invalid edge format".to_string(),
                                });
                            }
                        };
                        let to = match &edge_arr[1] {
                            Value::Integer(i) => *i as u64,
                            Value::Node(id) => *id,
                            _ => {
                                return Err(GraphError::Validation {
                                    message: "Invalid edge format".to_string(),
                                });
                            }
                        };
                        let weight = if edge_arr.len() > 2 {
                            match &edge_arr[2] {
                                Value::Float(w) => *w,
                                Value::Integer(w) => *w as f64,
                                _ => 1.0,
                            }
                        } else {
                            1.0
                        };
                        edges.push((from, to, weight));
                    }
                    _ => {
                        return Err(GraphError::Validation {
                            message: format!("Invalid edge format: {:?}", v),
                        });
                    }
                }
            }
            Ok(edges)
        }
        _ => Err(GraphError::Validation {
            message: "Expected array of edges".to_string(),
        }),
    }
}

/// 단일 노드 ID 추출
pub fn extract_single_node_id(value: &Value) -> Result<u64> {
    use zeta4j_graph::error::GraphError;
    match value {
        Value::Integer(i) => Ok(*i as u64),
        Value::Node(id) => Ok(*id),
        _ => Err(GraphError::Validation {
            message: format!("Expected integer or node, got {:?}", value),
        }),
    }
}

/// Map에서 노드 ID 추출
pub fn extract_node_id_from_map(map: &IndexMap<String, Value>, key: &str) -> Result<u64> {
    use zeta4j_graph::error::GraphError;
    match map.get(key) {
        Some(Value::Integer(i)) => Ok(*i as u64),
        Some(Value::Node(id)) => Ok(*id),
        _ => Err(GraphError::Validation {
            message: format!("Missing or invalid '{}' in map", key),
        }),
    }
}

// ============================================================================
// 결과 변환 함수들
// ============================================================================

/// MST 결과를 Value로 변환
pub fn mst_result_to_value(result: zeta4j_graph::algorithm::mst::MSTResult) -> Result<Value> {
    let edges: Vec<Value> = result
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

    let mut map = IndexMap::new();
    map.insert("edges".to_string(), Value::Array(edges));
    map.insert("totalWeight".to_string(), Value::Float(result.total_weight));
    map.insert(
        "nodeCount".to_string(),
        Value::Integer(result.node_count as i64),
    );
    map.insert(
        "edgeCount".to_string(),
        Value::Integer(result.edge_count() as i64),
    );
    map.insert(
        "algorithm".to_string(),
        Value::String(match result.algorithm {
            zeta4j_graph::algorithm::mst::MSTAlgorithm::Prim => "Prim".to_string(),
            zeta4j_graph::algorithm::mst::MSTAlgorithm::Kruskal => "Kruskal".to_string(),
        }),
    );

    Ok(Value::Map(map))
}

/// Topological Sort 결과를 Value로 변환
pub fn topological_result_to_value(
    result: zeta4j_graph::algorithm::topological::TopologicalResult,
) -> Result<Value> {
    use zeta4j_graph::algorithm::topological::TopologicalAlgorithm;

    let order: Vec<Value> = result
        .order
        .iter()
        .map(|&id| Value::Integer(id as i64))
        .collect();

    let mut map = IndexMap::new();
    map.insert("order".to_string(), Value::Array(order));
    map.insert("isDAG".to_string(), Value::Boolean(result.is_dag));
    map.insert(
        "algorithm".to_string(),
        Value::String(match result.algorithm {
            TopologicalAlgorithm::Kahn => "Kahn".to_string(),
            TopologicalAlgorithm::Dfs => "DFS".to_string(),
        }),
    );
    map.insert(
        "processedCount".to_string(),
        Value::Integer(result.processed_count as i64),
    );
    map.insert(
        "totalNodes".to_string(),
        Value::Integer(result.total_nodes as i64),
    );

    if let Some(levels) = result.levels {
        let level_values: Vec<Value> = levels
            .iter()
            .map(|level| {
                let nodes: Vec<Value> = level.iter().map(|&id| Value::Integer(id as i64)).collect();
                Value::Array(nodes)
            })
            .collect();
        map.insert("levels".to_string(), Value::Array(level_values));
    }

    Ok(Value::Map(map))
}

/// MaxFlow 결과를 Value로 변환
pub fn max_flow_result_to_value(
    result: zeta4j_graph::algorithm::flow::MaxFlowResult,
) -> Result<Value> {
    let mut map = IndexMap::new();
    map.insert("maxFlow".to_string(), Value::Float(result.max_flow));
    map.insert("source".to_string(), Value::Integer(result.source as i64));
    map.insert("sink".to_string(), Value::Integer(result.sink as i64));
    map.insert(
        "algorithm".to_string(),
        Value::String(match result.algorithm {
            zeta4j_graph::algorithm::flow::MaxFlowAlgorithm::EdmondsKarp => {
                "Edmonds-Karp".to_string()
            }
            zeta4j_graph::algorithm::flow::MaxFlowAlgorithm::PushRelabel => {
                "Push-Relabel".to_string()
            }
        }),
    );
    map.insert(
        "augmentingPaths".to_string(),
        Value::Integer(result.augmenting_paths as i64),
    );

    let edge_flows: Vec<Value> = result
        .edge_flows
        .iter()
        .map(|(from, to, flow, capacity)| {
            let mut edge_map = IndexMap::new();
            edge_map.insert("from".to_string(), Value::Integer(*from as i64));
            edge_map.insert("to".to_string(), Value::Integer(*to as i64));
            edge_map.insert("flow".to_string(), Value::Float(*flow));
            edge_map.insert("capacity".to_string(), Value::Float(*capacity));
            Value::Map(edge_map)
        })
        .collect();
    map.insert("edgeFlows".to_string(), Value::Array(edge_flows));

    Ok(Value::Map(map))
}

/// MinCut 결과를 Value로 변환
pub fn min_cut_result_to_value(
    result: zeta4j_graph::algorithm::flow::MinCutResult,
) -> Result<Value> {
    let mut map = IndexMap::new();
    map.insert("cutValue".to_string(), Value::Float(result.cut_value));
    map.insert("source".to_string(), Value::Integer(result.source as i64));
    map.insert("sink".to_string(), Value::Integer(result.sink as i64));

    let source_side: Vec<Value> = result
        .source_side
        .iter()
        .map(|&id| Value::Integer(id as i64))
        .collect();
    map.insert("sourceSide".to_string(), Value::Array(source_side));

    let sink_side: Vec<Value> = result
        .sink_side
        .iter()
        .map(|&id| Value::Integer(id as i64))
        .collect();
    map.insert("sinkSide".to_string(), Value::Array(sink_side));

    let cut_edges: Vec<Value> = result
        .cut_edges
        .iter()
        .map(|(from, to, capacity)| {
            let mut edge_map = IndexMap::new();
            edge_map.insert("from".to_string(), Value::Integer(*from as i64));
            edge_map.insert("to".to_string(), Value::Integer(*to as i64));
            edge_map.insert("capacity".to_string(), Value::Float(*capacity));
            Value::Map(edge_map)
        })
        .collect();
    map.insert("cutEdges".to_string(), Value::Array(cut_edges));

    Ok(Value::Map(map))
}

/// BellmanFord 결과를 Value로 변환
pub fn bellman_ford_result_to_value(
    result: zeta4j_graph::algorithm::path::BellmanFordResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    if result.has_negative_cycle {
        map.insert("hasNegativeCycle".to_string(), Value::Boolean(true));
        if let Some(cycle) = result.negative_cycle {
            map.insert(
                "negativeCycle".to_string(),
                Value::Array(
                    cycle
                        .into_iter()
                        .map(|n| Value::Integer(n as i64))
                        .collect(),
                ),
            );
        }
        map.insert("path".to_string(), Value::Null);
        map.insert("cost".to_string(), Value::Null);
    } else if result.nodes.is_empty() {
        map.insert("hasNegativeCycle".to_string(), Value::Boolean(false));
        map.insert("path".to_string(), Value::Null);
        map.insert("cost".to_string(), Value::Null);
    } else {
        map.insert("hasNegativeCycle".to_string(), Value::Boolean(false));
        map.insert(
            "path".to_string(),
            Value::Path(Path {
                nodes: result.nodes,
                edges: result.edges,
            }),
        );
        map.insert("cost".to_string(), Value::Float(result.total_cost));
    }

    map.insert("source".to_string(), Value::Integer(result.source as i64));
    map.insert("target".to_string(), Value::Integer(result.target as i64));

    Ok(Value::Map(map))
}

/// AStar 결과를 Value로 변환
pub fn astar_result_to_value(result: zeta4j_graph::algorithm::path::AStarResult) -> Result<Value> {
    let mut map = IndexMap::new();

    if result.nodes.is_empty() {
        map.insert("path".to_string(), Value::Null);
        map.insert("cost".to_string(), Value::Null);
    } else {
        map.insert(
            "path".to_string(),
            Value::Path(Path {
                nodes: result.nodes,
                edges: result.edges,
            }),
        );
        map.insert("cost".to_string(), Value::Float(result.total_cost));
    }

    map.insert("source".to_string(), Value::Integer(result.source as i64));
    map.insert("target".to_string(), Value::Integer(result.target as i64));
    map.insert(
        "nodesExplored".to_string(),
        Value::Integer(result.nodes_explored as i64),
    );

    Ok(Value::Map(map))
}

/// FloydWarshall 결과를 Value로 변환
pub fn floyd_warshall_result_to_value(
    result: zeta4j_graph::algorithm::path::FloydWarshallResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert(
        "hasNegativeCycle".to_string(),
        Value::Boolean(result.has_negative_cycle),
    );
    map.insert(
        "nodeCount".to_string(),
        Value::Integer(result.nodes.len() as i64),
    );
    map.insert("diameter".to_string(), Value::Float(result.diameter()));

    let mut distances_map = IndexMap::new();
    for ((from, to), dist) in &result.distances {
        if dist.is_finite() {
            let key = format!("{}->{}", from, to);
            distances_map.insert(key, Value::Float(*dist));
        }
    }
    map.insert("distances".to_string(), Value::Map(distances_map));

    Ok(Value::Map(map))
}

/// BipartiteResult를 Value로 변환
pub fn bipartite_result_to_value(
    result: zeta4j_graph::algorithm::matching::BipartiteResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert(
        "isBipartite".to_string(),
        Value::Boolean(result.is_bipartite),
    );

    let left: Vec<Value> = result
        .left_partition
        .iter()
        .map(|&n| Value::Integer(n as i64))
        .collect();
    map.insert("leftPartition".to_string(), Value::Array(left));

    let right: Vec<Value> = result
        .right_partition
        .iter()
        .map(|&n| Value::Integer(n as i64))
        .collect();
    map.insert("rightPartition".to_string(), Value::Array(right));

    if let Some(cycle) = result.odd_cycle {
        let cycle_values: Vec<Value> = cycle.iter().map(|&n| Value::Integer(n as i64)).collect();
        map.insert("oddCycle".to_string(), Value::Array(cycle_values));
    }

    Ok(Value::Map(map))
}

/// MatchingResult를 Value로 변환
pub fn matching_result_to_value(
    result: zeta4j_graph::algorithm::matching::MatchingResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert(
        "matchingSize".to_string(),
        Value::Integer(result.matching_size as i64),
    );
    map.insert("isPerfect".to_string(), Value::Boolean(result.is_perfect()));
    map.insert("algorithm".to_string(), Value::String(result.algorithm));

    let matches: Vec<Value> = result
        .matches
        .iter()
        .map(|&(left, right)| {
            let mut m = IndexMap::new();
            m.insert("left".to_string(), Value::Integer(left as i64));
            m.insert("right".to_string(), Value::Integer(right as i64));
            Value::Map(m)
        })
        .collect();
    map.insert("matches".to_string(), Value::Array(matches));

    let unmatched_left: Vec<Value> = result
        .unmatched_left
        .iter()
        .map(|&n| Value::Integer(n as i64))
        .collect();
    map.insert("unmatchedLeft".to_string(), Value::Array(unmatched_left));

    let unmatched_right: Vec<Value> = result
        .unmatched_right
        .iter()
        .map(|&n| Value::Integer(n as i64))
        .collect();
    map.insert("unmatchedRight".to_string(), Value::Array(unmatched_right));

    Ok(Value::Map(map))
}

/// HungarianResult를 Value로 변환
pub fn hungarian_result_to_value(
    result: zeta4j_graph::algorithm::matching::HungarianResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert(
        "matchingSize".to_string(),
        Value::Integer(result.matching_size as i64),
    );
    map.insert("totalWeight".to_string(), Value::Float(result.total_weight));
    map.insert("maximized".to_string(), Value::Boolean(result.maximized));

    let matches: Vec<Value> = result
        .matches
        .iter()
        .map(|&(left, right)| {
            let mut m = IndexMap::new();
            m.insert("left".to_string(), Value::Integer(left as i64));
            m.insert("right".to_string(), Value::Integer(right as i64));
            Value::Map(m)
        })
        .collect();
    map.insert("matches".to_string(), Value::Array(matches));

    let mut assignment_map = IndexMap::new();
    for (&from, &to) in &result.assignment {
        assignment_map.insert(from.to_string(), Value::Integer(to as i64));
    }
    map.insert("assignment".to_string(), Value::Map(assignment_map));

    Ok(Value::Map(map))
}

/// SCCResult를 Value로 변환
pub fn scc_result_to_value(result: zeta4j_graph::algorithm::scc::SCCResult) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert("count".to_string(), Value::Integer(result.count as i64));
    map.insert(
        "isStronglyConnected".to_string(),
        Value::Boolean(result.is_strongly_connected),
    );
    map.insert(
        "nodeCount".to_string(),
        Value::Integer(result.node_count as i64),
    );
    map.insert(
        "largestSize".to_string(),
        Value::Integer(result.largest_scc_size() as i64),
    );

    let components: Vec<Value> = result
        .components
        .iter()
        .map(|component| {
            let nodes: Vec<Value> = component
                .iter()
                .map(|&n| Value::Integer(n as i64))
                .collect();
            Value::Array(nodes)
        })
        .collect();
    map.insert("components".to_string(), Value::Array(components));
    map.insert("algorithm".to_string(), Value::String(result.algorithm));

    Ok(Value::Map(map))
}

/// CondensationGraph를 Value로 변환
pub fn condensation_to_value(
    result: zeta4j_graph::algorithm::scc::CondensationGraph,
) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert(
        "sccCount".to_string(),
        Value::Integer(result.scc_count() as i64),
    );
    map.insert(
        "edgeCount".to_string(),
        Value::Integer(result.edge_count() as i64),
    );

    let sizes: Vec<Value> = result
        .scc_sizes
        .iter()
        .map(|&s| Value::Integer(s as i64))
        .collect();
    map.insert("sccSizes".to_string(), Value::Array(sizes));

    let edges: Vec<Value> = result
        .scc_edges
        .iter()
        .map(|&(from, to)| {
            let mut e = IndexMap::new();
            e.insert("from".to_string(), Value::Integer(from as i64));
            e.insert("to".to_string(), Value::Integer(to as i64));
            Value::Map(e)
        })
        .collect();
    map.insert("edges".to_string(), Value::Array(edges));

    let sources: Vec<Value> = result
        .source_sccs()
        .iter()
        .map(|&s| Value::Integer(s as i64))
        .collect();
    map.insert("sourceSCCs".to_string(), Value::Array(sources));

    let sinks: Vec<Value> = result
        .sink_sccs()
        .iter()
        .map(|&s| Value::Integer(s as i64))
        .collect();
    map.insert("sinkSCCs".to_string(), Value::Array(sinks));

    Ok(Value::Map(map))
}

/// ColoringResult를 Value로 변환
pub fn coloring_result_to_value(
    result: zeta4j_graph::algorithm::coloring::ColoringResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert(
        "numColors".to_string(),
        Value::Integer(result.num_colors as i64),
    );
    map.insert("algorithm".to_string(), Value::String(result.algorithm));

    let mut colors_map = IndexMap::new();
    for (&node, &color) in &result.colors {
        colors_map.insert(node.to_string(), Value::Integer(color as i64));
    }
    map.insert("colors".to_string(), Value::Map(colors_map));

    let color_classes: Vec<Value> = result
        .color_classes
        .iter()
        .map(|class| {
            let nodes: Vec<Value> = class.iter().map(|&n| Value::Integer(n as i64)).collect();
            Value::Array(nodes)
        })
        .collect();
    map.insert("colorClasses".to_string(), Value::Array(color_classes));

    Ok(Value::Map(map))
}

/// EulerianCheckResult를 Value로 변환
pub fn eulerian_check_result_to_value(
    result: zeta4j_graph::algorithm::eulerian::EulerianCheckResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert(
        "hasEulerianCircuit".to_string(),
        Value::Boolean(result.has_eulerian_circuit),
    );
    map.insert(
        "hasEulerianPath".to_string(),
        Value::Boolean(result.has_eulerian_path),
    );
    map.insert(
        "edgeCount".to_string(),
        Value::Integer(result.edge_count as i64),
    );
    map.insert(
        "nodeCount".to_string(),
        Value::Integer(result.node_count as i64),
    );

    if let Some(start) = result.path_start {
        map.insert("pathStart".to_string(), Value::Integer(start as i64));
    } else {
        map.insert("pathStart".to_string(), Value::Null);
    }

    if let Some(end) = result.path_end {
        map.insert("pathEnd".to_string(), Value::Integer(end as i64));
    } else {
        map.insert("pathEnd".to_string(), Value::Null);
    }

    let odd_nodes: Vec<Value> = result
        .odd_degree_nodes
        .iter()
        .map(|&n| Value::Integer(n as i64))
        .collect();
    map.insert("oddDegreeNodes".to_string(), Value::Array(odd_nodes));

    Ok(Value::Map(map))
}

/// EulerianResult를 Value로 변환
pub fn eulerian_result_to_value(
    result: zeta4j_graph::algorithm::eulerian::EulerianResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert("isCircuit".to_string(), Value::Boolean(result.is_circuit));
    map.insert(
        "edgeCount".to_string(),
        Value::Integer(result.edge_count as i64),
    );

    let path: Vec<Value> = result
        .path
        .iter()
        .map(|&n| Value::Integer(n as i64))
        .collect();
    map.insert("path".to_string(), Value::Array(path));

    let edges: Vec<Value> = result
        .edges
        .iter()
        .map(|&(from, to)| {
            let mut e = IndexMap::new();
            e.insert("from".to_string(), Value::Integer(from as i64));
            e.insert("to".to_string(), Value::Integer(to as i64));
            Value::Map(e)
        })
        .collect();
    map.insert("edges".to_string(), Value::Array(edges));

    Ok(Value::Map(map))
}

/// HamiltonianResult를 Value로 변환
pub fn hamiltonian_result_to_value(
    result: zeta4j_graph::algorithm::hamiltonian::HamiltonianResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    map.insert("found".to_string(), Value::Boolean(result.found));
    map.insert("isCircuit".to_string(), Value::Boolean(result.is_circuit));
    map.insert(
        "nodesExplored".to_string(),
        Value::Integer(result.nodes_explored as i64),
    );

    let path: Vec<Value> = result
        .path
        .iter()
        .map(|&n| Value::Integer(n as i64))
        .collect();
    map.insert("path".to_string(), Value::Array(path));
    map.insert(
        "pathLength".to_string(),
        Value::Integer(result.path.len() as i64),
    );

    Ok(Value::Map(map))
}

/// Clique 결과를 Value로 변환
pub fn clique_result_to_value(
    result: zeta4j_graph::algorithm::clique::CliqueResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    let cliques: Vec<Value> = result
        .cliques
        .iter()
        .map(|clique| Value::Array(clique.iter().map(|&id| Value::Integer(id as i64)).collect()))
        .collect();
    map.insert("cliques".to_string(), Value::Array(cliques));
    map.insert(
        "cliqueCount".to_string(),
        Value::Integer(result.clique_count as i64),
    );
    map.insert(
        "maxCliqueSize".to_string(),
        Value::Integer(result.max_clique_size as i64),
    );

    Ok(Value::Map(map))
}

/// Independent Set 결과를 Value로 변환
pub fn independent_set_result_to_value(
    result: zeta4j_graph::algorithm::independent_set::IndependentSetResult,
) -> Result<Value> {
    let mut map = IndexMap::new();
    let set: Vec<Value> = result
        .set
        .iter()
        .map(|&id| Value::Integer(id as i64))
        .collect();
    map.insert("set".to_string(), Value::Array(set));
    map.insert("size".to_string(), Value::Integer(result.size as i64));
    map.insert("isMaximum".to_string(), Value::Boolean(result.is_maximum));
    Ok(Value::Map(map))
}

/// Dominating Set 결과를 Value로 변환
pub fn dominating_set_result_to_value(
    result: zeta4j_graph::algorithm::dominating_set::DominatingSetResult,
) -> Result<Value> {
    let mut map = IndexMap::new();
    let set: Vec<Value> = result
        .set
        .iter()
        .map(|&id| Value::Integer(id as i64))
        .collect();
    map.insert("set".to_string(), Value::Array(set));
    map.insert("size".to_string(), Value::Integer(result.size as i64));
    map.insert("isMinimum".to_string(), Value::Boolean(result.is_minimum));
    Ok(Value::Map(map))
}

/// Feedback Vertex Set 결과를 Value로 변환
pub fn feedback_vertex_set_result_to_value(
    result: zeta4j_graph::algorithm::feedback::FeedbackVertexSetResult,
) -> Result<Value> {
    let mut map = IndexMap::new();
    let set: Vec<Value> = result
        .set
        .iter()
        .map(|&id| Value::Integer(id as i64))
        .collect();
    map.insert("set".to_string(), Value::Array(set));
    map.insert("size".to_string(), Value::Integer(result.size as i64));
    map.insert("isMinimum".to_string(), Value::Boolean(result.is_minimum));
    Ok(Value::Map(map))
}

/// Feedback Arc Set 결과를 Value로 변환
pub fn feedback_arc_set_result_to_value(
    result: zeta4j_graph::algorithm::feedback::FeedbackArcSetResult,
) -> Result<Value> {
    let mut map = IndexMap::new();
    let edges: Vec<Value> = result
        .edges
        .iter()
        .map(|(from, to)| {
            Value::Array(vec![
                Value::Integer(*from as i64),
                Value::Integer(*to as i64),
            ])
        })
        .collect();
    map.insert("edges".to_string(), Value::Array(edges));
    map.insert("size".to_string(), Value::Integer(result.size as i64));
    map.insert("isMinimum".to_string(), Value::Boolean(result.is_minimum));
    Ok(Value::Map(map))
}

/// Steiner Tree 결과를 Value로 변환
pub fn steiner_result_to_value(
    result: zeta4j_graph::algorithm::steiner::SteinerResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    let edges: Vec<Value> = result
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
    map.insert("totalWeight".to_string(), Value::Float(result.total_weight));

    let terminals: Vec<Value> = result
        .terminals
        .iter()
        .map(|&t| Value::Integer(t as i64))
        .collect();
    map.insert("terminals".to_string(), Value::Array(terminals));

    let steiner_vertices: Vec<Value> = result
        .steiner_vertices
        .iter()
        .map(|&v| Value::Integer(v as i64))
        .collect();
    map.insert(
        "steinerVertices".to_string(),
        Value::Array(steiner_vertices),
    );

    let all_vertices: Vec<Value> = result
        .all_vertices
        .iter()
        .map(|&v| Value::Integer(v as i64))
        .collect();
    map.insert("allVertices".to_string(), Value::Array(all_vertices));

    map.insert(
        "edgeCount".to_string(),
        Value::Integer(result.edge_count() as i64),
    );
    map.insert(
        "vertexCount".to_string(),
        Value::Integer(result.vertex_count() as i64),
    );
    map.insert(
        "isHeuristic".to_string(),
        Value::Boolean(result.is_heuristic),
    );

    Ok(Value::Map(map))
}

/// Tree Decomposition 결과를 Value로 변환
pub fn tree_decomposition_result_to_value(
    result: zeta4j_graph::algorithm::tree_decomposition::TreeDecompositionResult,
) -> Result<Value> {
    let mut map = IndexMap::new();

    let bags: Vec<Value> = result
        .bags
        .iter()
        .map(|bag| {
            let mut bag_map = IndexMap::new();
            bag_map.insert("id".to_string(), Value::Integer(bag.id as i64));
            let vertices: Vec<Value> = bag
                .vertices
                .iter()
                .map(|&v| Value::Integer(v as i64))
                .collect();
            bag_map.insert("vertices".to_string(), Value::Array(vertices));
            bag_map.insert("size".to_string(), Value::Integer(bag.size() as i64));
            Value::Map(bag_map)
        })
        .collect();
    map.insert("bags".to_string(), Value::Array(bags));

    let tree_edges: Vec<Value> = result
        .tree_edges
        .iter()
        .map(|(i, j)| Value::Array(vec![Value::Integer(*i as i64), Value::Integer(*j as i64)]))
        .collect();
    map.insert("treeEdges".to_string(), Value::Array(tree_edges));

    map.insert("width".to_string(), Value::Integer(result.width as i64));
    map.insert(
        "bagCount".to_string(),
        Value::Integer(result.bag_count() as i64),
    );

    let ordering: Vec<Value> = result
        .elimination_ordering
        .iter()
        .map(|&v| Value::Integer(v as i64))
        .collect();
    map.insert("eliminationOrdering".to_string(), Value::Array(ordering));

    Ok(Value::Map(map))
}
