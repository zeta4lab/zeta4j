// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 경로 알고리즘 스칼라 함수
//!
//! shortestPath, dijkstra, bellmanFord, astar, floydWarshall 등

use super::super::{QueryExecutor, graph_algorithms};
use indexmap::IndexMap;
use zeta4j_cypher_core::ast::ScalarFunction;
use zeta4j_graph::algorithm::path::{BfsShortestPath, Dijkstra, DijkstraConfig};
use zeta4j_graph::core::{Path, Value};
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    pub(super) async fn evaluate_path_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
    ) -> Result<Value> {
        match func {
            ScalarFunction::ShortestPath => {
                // shortestPath(startNodeId, endNodeId, maxDepth?, relTypes?)
                // 반환: Path 또는 null
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message:
                            "shortestPath() requires at least 2 arguments (startNodeId, endNodeId)"
                                .to_string(),
                    });
                }

                let start_id = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(GraphError::Validation {
                            message: format!(
                                "shortestPath() first argument must be Integer (node ID), got {}",
                                args[0].type_name()
                            ),
                        });
                    }
                };

                let end_id = match &args[1] {
                    Value::Integer(id) => *id as u64,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(GraphError::Validation {
                            message: format!(
                                "shortestPath() second argument must be Integer (node ID), got {}",
                                args[1].type_name()
                            ),
                        });
                    }
                };

                let max_depth = if args.len() > 2 {
                    match &args[2] {
                        Value::Integer(d) => Some(*d as usize),
                        Value::Null => None,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "shortestPath() third argument (maxDepth) must be Integer"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    None
                };

                // 관계 타입 필터 (선택적)
                let rel_types: Option<Vec<String>> = if args.len() > 3 {
                    match &args[3] {
                        Value::Array(arr) => {
                            let types: Result<Vec<String>> = arr
                                .iter()
                                .map(|v| match v {
                                    Value::String(s) => Ok(s.clone()),
                                    _ => Err(GraphError::Validation {
                                        message:
                                            "shortestPath() relTypes must be an array of strings"
                                                .to_string(),
                                    }),
                                })
                                .collect();
                            Some(types?)
                        }
                        Value::Null => None,
                        _ => {
                            return Err(GraphError::Validation {
                                message:
                                    "shortestPath() fourth argument (relTypes) must be an Array"
                                        .to_string(),
                            });
                        }
                    }
                } else {
                    None
                };

                let bfs = BfsShortestPath::new(&self.engine);
                let rel_refs: Option<&[String]> = rel_types.as_deref();

                match bfs
                    .find(start_id, end_id, max_depth, rel_refs, false)
                    .await?
                {
                    Some(result) => {
                        // Path 값으로 변환
                        Ok(Value::Path(Path {
                            nodes: result.nodes,
                            edges: result.edges,
                        }))
                    }
                    None => Ok(Value::Null),
                }
            }

            ScalarFunction::Dijkstra => {
                // dijkstra(startNodeId, endNodeId, weightProperty?, maxCost?)
                // 반환: Map { path: Path, cost: Float } 또는 null
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message:
                            "dijkstra() requires at least 2 arguments (startNodeId, endNodeId)"
                                .to_string(),
                    });
                }

                let start_id = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(GraphError::Validation {
                            message: format!(
                                "dijkstra() first argument must be Integer (node ID), got {}",
                                args[0].type_name()
                            ),
                        });
                    }
                };

                let end_id = match &args[1] {
                    Value::Integer(id) => *id as u64,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(GraphError::Validation {
                            message: format!(
                                "dijkstra() second argument must be Integer (node ID), got {}",
                                args[1].type_name()
                            ),
                        });
                    }
                };

                let weight_property = if args.len() > 2 {
                    match &args[2] {
                        Value::String(s) => s.clone(),
                        Value::Null => "weight".to_string(),
                        _ => {
                            return Err(GraphError::Validation {
                                message:
                                    "dijkstra() third argument (weightProperty) must be String"
                                        .to_string(),
                            });
                        }
                    }
                } else {
                    "weight".to_string()
                };

                let max_cost =
                    if args.len() > 3 {
                        match &args[3] {
                            Value::Float(c) => Some(*c),
                            Value::Integer(c) => Some(*c as f64),
                            Value::Null => None,
                            _ => return Err(GraphError::Validation {
                                message:
                                    "dijkstra() fourth argument (maxCost) must be Float or Integer"
                                        .to_string(),
                            }),
                        }
                    } else {
                        None
                    };

                let mut config = DijkstraConfig::default().with_weight_property(&weight_property);
                if let Some(cost) = max_cost {
                    config = config.with_max_cost(cost);
                }
                let dijkstra = Dijkstra::new(&self.engine, config);

                match dijkstra.find(start_id, end_id, None, false).await? {
                    Some(result) => {
                        // Map으로 결과 반환 { path: Path, cost: Float }
                        let mut map = IndexMap::new();
                        map.insert(
                            "path".to_string(),
                            Value::Path(Path {
                                nodes: result.nodes,
                                edges: result.edges,
                            }),
                        );
                        map.insert("cost".to_string(), Value::Float(result.total_cost));
                        Ok(Value::Map(map))
                    }
                    None => Ok(Value::Null),
                }
            }

            ScalarFunction::BellmanFord => {
                // bellmanFord(source, target, weightProp?)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "bellmanFord() requires at least 2 arguments (source, target)"
                            .to_string(),
                    });
                }

                let source = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(GraphError::Validation {
                            message: "bellmanFord() first argument must be Integer".to_string(),
                        });
                    }
                };

                let target = match &args[1] {
                    Value::Integer(id) => *id as u64,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(GraphError::Validation {
                            message: "bellmanFord() second argument must be Integer".to_string(),
                        });
                    }
                };

                let weight_prop = if args.len() > 2 {
                    match &args[2] {
                        Value::String(s) => s.clone(),
                        Value::Null => "weight".to_string(),
                        _ => {
                            return Err(GraphError::Validation {
                                message: "bellmanFord() third argument must be String".to_string(),
                            });
                        }
                    }
                } else {
                    "weight".to_string()
                };

                use zeta4j_graph::algorithm::path::{BellmanFord, BellmanFordConfig};
                let config = BellmanFordConfig::default().with_weight_property(&weight_prop);
                let bf = BellmanFord::with_config(&self.engine, config);
                let result = bf.find_path(source, target).await?;

                graph_algorithms::bellman_ford_result_to_value(result)
            }

            ScalarFunction::AStar => {
                // astar(source, target, weightProp?, xProp?, yProp?)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "astar() requires at least 2 arguments (source, target)"
                            .to_string(),
                    });
                }

                let source = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(GraphError::Validation {
                            message: "astar() first argument must be Integer".to_string(),
                        });
                    }
                };

                let target = match &args[1] {
                    Value::Integer(id) => *id as u64,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(GraphError::Validation {
                            message: "astar() second argument must be Integer".to_string(),
                        });
                    }
                };

                let weight_prop = if args.len() > 2 {
                    match &args[2] {
                        Value::String(s) => s.clone(),
                        Value::Null => "weight".to_string(),
                        _ => "weight".to_string(),
                    }
                } else {
                    "weight".to_string()
                };

                use zeta4j_graph::algorithm::path::{AStar, AStarConfig};
                let mut config = AStarConfig::default().with_weight_property(&weight_prop);

                // x, y 프로퍼티가 있으면 유클리드 휴리스틱 사용
                if args.len() > 4
                    && let (Value::String(x_prop), Value::String(y_prop)) = (&args[3], &args[4])
                {
                    config = config.with_euclidean(x_prop, y_prop);
                }

                let astar = AStar::with_config(&self.engine, config);
                let result = astar.find_path(source, target).await?;

                graph_algorithms::astar_result_to_value(result)
            }

            ScalarFunction::FloydWarshall => {
                // floydWarshall(weightProp?)
                let weight_prop = if !args.is_empty() {
                    match &args[0] {
                        Value::String(s) => s.clone(),
                        Value::Null => "weight".to_string(),
                        _ => "weight".to_string(),
                    }
                } else {
                    "weight".to_string()
                };

                use zeta4j_graph::algorithm::path::{FloydWarshall, FloydWarshallConfig};
                let config = FloydWarshallConfig::default().with_weight_property(&weight_prop);
                let fw = FloydWarshall::with_config(&self.engine, config);
                let result = fw.compute().await?;

                graph_algorithms::floyd_warshall_result_to_value(result)
            }

            ScalarFunction::AllPairsDistance => {
                // allPairsDistance(weightProp?)
                let weight_prop = if !args.is_empty() {
                    match &args[0] {
                        Value::String(s) => s.clone(),
                        Value::Null => "weight".to_string(),
                        _ => "weight".to_string(),
                    }
                } else {
                    "weight".to_string()
                };

                use zeta4j_graph::algorithm::path::all_pairs_distances_weighted;
                let distances = all_pairs_distances_weighted(&self.engine, &weight_prop).await?;

                // Map으로 변환: "from->to" -> distance
                let mut map = IndexMap::new();
                for ((from, to), dist) in distances {
                    if dist.is_finite() {
                        let key = format!("{}->{}", from, to);
                        map.insert(key, Value::Float(dist));
                    }
                }

                Ok(Value::Map(map))
            }

            ScalarFunction::HasNegativeCycle => {
                // hasNegativeCycle(source, weightProp?)
                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "hasNegativeCycle() requires at least 1 argument (source)"
                            .to_string(),
                    });
                }

                let source = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    Value::Null => return Ok(Value::Null),
                    _ => {
                        return Err(GraphError::Validation {
                            message: "hasNegativeCycle() first argument must be Integer"
                                .to_string(),
                        });
                    }
                };

                let weight_prop = if args.len() > 1 {
                    match &args[1] {
                        Value::String(s) => s.clone(),
                        Value::Null => "weight".to_string(),
                        _ => "weight".to_string(),
                    }
                } else {
                    "weight".to_string()
                };

                use zeta4j_graph::algorithm::path::{BellmanFord, BellmanFordConfig};
                let config = BellmanFordConfig::default().with_weight_property(&weight_prop);
                let bf = BellmanFord::with_config(&self.engine, config);

                match bf.detect_negative_cycle(source).await? {
                    Some(cycle) => {
                        let mut map = IndexMap::new();
                        map.insert("hasNegativeCycle".to_string(), Value::Boolean(true));
                        map.insert(
                            "cycle".to_string(),
                            Value::Array(
                                cycle
                                    .into_iter()
                                    .map(|n| Value::Integer(n as i64))
                                    .collect(),
                            ),
                        );
                        Ok(Value::Map(map))
                    }
                    None => {
                        let mut map = IndexMap::new();
                        map.insert("hasNegativeCycle".to_string(), Value::Boolean(false));
                        map.insert("cycle".to_string(), Value::Null);
                        Ok(Value::Map(map))
                    }
                }
            }

            _ => unreachable!("Non-path function passed to evaluate_path_function"),
        }
    }
}
