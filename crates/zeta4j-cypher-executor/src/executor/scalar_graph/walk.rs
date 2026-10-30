//! 랜덤 워크 및 임베딩 스칼라 함수
//!
//! randomWalk, randomWalks, rwr, node2vecWalk, fastRP, hashGNN 등

use super::super::QueryExecutor;
use indexmap::IndexMap;
use zeta4j_cypher_core::ast::ScalarFunction;
use zeta4j_graph::algorithm::walk::{FastRP, FastRPConfig};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    pub(super) async fn evaluate_walk_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
    ) -> Result<Value> {
        match func {
            ScalarFunction::RandomWalk => {
                // randomWalk(startNode, length)
                // 반환: Array (노드 ID 시퀀스)
                use zeta4j_graph::algorithm::walk::{RandomWalk as RW, RandomWalkConfig};

                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "randomWalk() requires at least one argument (startNode)"
                            .to_string(),
                    });
                }

                let start_node = match &args[0] {
                    Value::Node(id) => *id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "randomWalk() first argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let walk_length = if args.len() > 1 {
                    match &args[1] {
                        Value::Integer(len) => *len as usize,
                        Value::Null => 10,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "randomWalk() second argument (length) must be Integer"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    10
                };

                let config = RandomWalkConfig::default().with_walk_length(walk_length);
                let walker = RW::with_config(&self.engine, config);
                let path = walker.walk(start_node).await?;

                let path_values: Vec<Value> =
                    path.iter().map(|&id| Value::Integer(id as i64)).collect();

                Ok(Value::Array(path_values))
            }

            ScalarFunction::RandomWalks => {
                // randomWalks(startNode, length, count)
                // 반환: Map { walks, startNode, walkCount, averageLength }
                use zeta4j_graph::algorithm::walk::{RandomWalk as RW, RandomWalkConfig};

                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "randomWalks() requires at least one argument (startNode)"
                            .to_string(),
                    });
                }

                let start_node = match &args[0] {
                    Value::Node(id) => *id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "randomWalks() first argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let walk_length = if args.len() > 1 {
                    match &args[1] {
                        Value::Integer(len) => *len as usize,
                        Value::Null => 10,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "randomWalks() second argument (length) must be Integer"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    10
                };

                let walk_count = if args.len() > 2 {
                    match &args[2] {
                        Value::Integer(cnt) => *cnt as usize,
                        Value::Null => 1,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "randomWalks() third argument (count) must be Integer"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    1
                };

                let config = RandomWalkConfig::default()
                    .with_walk_length(walk_length)
                    .with_walks_per_node(walk_count);
                let walker = RW::with_config(&self.engine, config);
                let result = walker.walks(start_node).await?;

                // 워크 경로들
                let walks_values: Vec<Value> = result
                    .walks
                    .iter()
                    .map(|walk| {
                        let path: Vec<Value> =
                            walk.iter().map(|&id| Value::Integer(id as i64)).collect();
                        Value::Array(path)
                    })
                    .collect();

                let mut map = IndexMap::new();
                map.insert("walks".to_string(), Value::Array(walks_values));
                map.insert(
                    "startNode".to_string(),
                    Value::Integer(result.start_node as i64),
                );
                map.insert(
                    "walkCount".to_string(),
                    Value::Integer(result.walk_count as i64),
                );
                map.insert(
                    "averageLength".to_string(),
                    Value::Float(result.average_length),
                );
                map.insert(
                    "maxLength".to_string(),
                    Value::Integer(result.max_length as i64),
                );

                Ok(Value::Map(map))
            }

            ScalarFunction::Rwr => {
                // rwr(startNode, restartProb?, maxIterations?)
                // 반환: Map { scores, startNode, iterations, converged }
                use zeta4j_graph::algorithm::walk::{RWRConfig, RandomWalkWithRestart};

                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "rwr() requires at least one argument (startNode)".to_string(),
                    });
                }

                let start_node = match &args[0] {
                    Value::Node(id) => *id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "rwr() first argument must be a Node or Integer".to_string(),
                        });
                    }
                };

                let restart_prob = if args.len() > 1 {
                    match &args[1] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64 / 100.0, // 정수면 퍼센트로 해석
                        Value::Null => 0.15,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "rwr() second argument (restartProb) must be Float"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    0.15
                };

                let max_iterations = if args.len() > 2 {
                    match &args[2] {
                        Value::Integer(i) => *i as usize,
                        Value::Null => 100,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "rwr() third argument (maxIterations) must be Integer"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    100
                };

                let config = RWRConfig::default()
                    .with_restart_probability(restart_prob)
                    .with_max_iterations(max_iterations);

                let rwr_algo = RandomWalkWithRestart::with_config(&self.engine, config);
                let result = rwr_algo.run(start_node).await?;

                // 점수 매핑
                let mut scores_map = IndexMap::new();
                for (&node_id, &score) in &result.scores {
                    scores_map.insert(node_id.to_string(), Value::Float(score));
                }

                // 상위 10개 노드
                let top_nodes: Vec<Value> = result
                    .top_n(10)
                    .iter()
                    .map(|&(id, score)| {
                        let mut node_map = IndexMap::new();
                        node_map.insert("nodeId".to_string(), Value::Integer(id as i64));
                        node_map.insert("score".to_string(), Value::Float(score));
                        Value::Map(node_map)
                    })
                    .collect();

                let mut map = IndexMap::new();
                map.insert("scores".to_string(), Value::Map(scores_map));
                map.insert("topNodes".to_string(), Value::Array(top_nodes));
                map.insert(
                    "startNode".to_string(),
                    Value::Integer(result.start_node as i64),
                );
                map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                map.insert("converged".to_string(), Value::Boolean(result.converged));

                Ok(Value::Map(map))
            }

            ScalarFunction::Node2VecWalk => {
                // node2vecWalk(startNode, length, p, q)
                // 반환: Array[NodeId] 또는 Map { walks, p, q, averageLength }
                use zeta4j_graph::algorithm::walk::{BiasedRandomWalk, Node2VecConfig};

                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "node2vecWalk() requires at least one argument (startNode)"
                            .to_string(),
                    });
                }

                let start_node = match &args[0] {
                    Value::Node(id) => *id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "node2vecWalk() first argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let walk_length = if args.len() > 1 {
                    match &args[1] {
                        Value::Integer(i) => (*i).max(1) as usize,
                        Value::Null => 80,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "node2vecWalk() second argument (length) must be Integer"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    80
                };

                let p = if args.len() > 2 {
                    match &args[2] {
                        Value::Float(f) => (*f).max(0.001),
                        Value::Integer(i) => (*i as f64).max(0.001),
                        Value::Null => 1.0,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "node2vecWalk() third argument (p) must be Float"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    1.0
                };

                let q = if args.len() > 3 {
                    match &args[3] {
                        Value::Float(f) => (*f).max(0.001),
                        Value::Integer(i) => (*i as f64).max(0.001),
                        Value::Null => 1.0,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "node2vecWalk() fourth argument (q) must be Float"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    1.0
                };

                let config = Node2VecConfig::default()
                    .with_walk_length(walk_length)
                    .with_p(p)
                    .with_q(q);

                let walker = BiasedRandomWalk::with_config(&self.engine, config);
                let path = walker.walk(start_node).await?;

                // 경로를 배열로 반환
                let path_values: Vec<Value> =
                    path.iter().map(|&id| Value::Integer(id as i64)).collect();

                Ok(Value::Array(path_values))
            }

            ScalarFunction::FastRP => {
                // fastRP(embeddingDimension?, iterationWeights?, seed?)
                // 반환: Map { embeddings, dimension, nodeCount }
                let embedding_dimension =
                    if !args.is_empty() {
                        match &args[0] {
                            Value::Integer(i) => *i as usize,
                            Value::Null => 128,
                            _ => return Err(GraphError::Validation {
                                message:
                                    "fastRP() first argument (embeddingDimension) must be Integer"
                                        .to_string(),
                            }),
                        }
                    } else {
                        128
                    };

                let iteration_weights = if args.len() > 1 {
                    match &args[1] {
                        Value::Array(arr) => {
                            arr.iter()
                                .map(|v| match v {
                                    Value::Float(f) => Ok(*f),
                                    Value::Integer(i) => Ok(*i as f64),
                                    _ => Err(GraphError::Validation {
                                        message: "fastRP() second argument (iterationWeights) must be an array of floats".to_string(),
                                    }),
                                })
                                .collect::<Result<Vec<f64>>>()?
                        }
                        Value::Null => vec![0.0, 1.0, 1.0],
                        _ => return Err(GraphError::Validation {
                            message: "fastRP() second argument (iterationWeights) must be Array".to_string(),
                        }),
                    }
                } else {
                    vec![0.0, 1.0, 1.0]
                };

                let seed = if args.len() > 2 {
                    match &args[2] {
                        Value::Integer(i) => Some(*i as u64),
                        Value::Null => None,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "fastRP() third argument (seed) must be Integer"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    None
                };

                let mut config = FastRPConfig::default()
                    .with_embedding_dimension(embedding_dimension)
                    .with_iteration_weights(iteration_weights);

                if let Some(s) = seed {
                    config = config.with_seed(s);
                }

                let fastrp = FastRP::with_config(&self.engine, config);
                let result = fastrp.generate(None).await?;

                // 임베딩 매핑
                let mut embeddings_map = IndexMap::new();
                for (node_id, embedding) in &result.embeddings {
                    let emb_values: Vec<Value> =
                        embedding.iter().map(|&v| Value::Float(v as f64)).collect();
                    embeddings_map.insert(node_id.to_string(), Value::Array(emb_values));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("embeddings".to_string(), Value::Map(embeddings_map));
                result_map.insert(
                    "dimension".to_string(),
                    Value::Integer(result.dimension as i64),
                );
                result_map.insert(
                    "nodeCount".to_string(),
                    Value::Integer(result.node_count as i64),
                );

                Ok(Value::Map(result_map))
            }

            ScalarFunction::HashGNN => {
                // hashGNN(embeddingDimension?, numIterations?, seed?)
                // 반환: Map { embeddings, dimension, nodeCount }
                // 현재는 FastRP와 동일한 기본 구현 (추후 MinHash 기반으로 개선 예정)
                let embedding_dimension =
                    if !args.is_empty() {
                        match &args[0] {
                            Value::Integer(i) => *i as usize,
                            Value::Null => 128,
                            _ => return Err(GraphError::Validation {
                                message:
                                    "hashGNN() first argument (embeddingDimension) must be Integer"
                                        .to_string(),
                            }),
                        }
                    } else {
                        128
                    };

                let num_iterations = if args.len() > 1 {
                    match &args[1] {
                        Value::Integer(i) => *i as usize,
                        Value::Null => 3,
                        _ => {
                            return Err(GraphError::Validation {
                                message:
                                    "hashGNN() second argument (numIterations) must be Integer"
                                        .to_string(),
                            });
                        }
                    }
                } else {
                    3
                };

                let seed = if args.len() > 2 {
                    match &args[2] {
                        Value::Integer(i) => Some(*i as u64),
                        Value::Null => None,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "hashGNN() third argument (seed) must be Integer"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    None
                };

                // iteration weights: 모든 iteration에 동일 가중치
                let iteration_weights: Vec<f64> = (0..=num_iterations).map(|_| 1.0).collect();

                let mut config = FastRPConfig::default()
                    .with_embedding_dimension(embedding_dimension)
                    .with_iteration_weights(iteration_weights);

                if let Some(s) = seed {
                    config = config.with_seed(s);
                }

                let fastrp = FastRP::with_config(&self.engine, config);
                let result = fastrp.generate(None).await?;

                // 임베딩 매핑
                let mut embeddings_map = IndexMap::new();
                for (node_id, embedding) in &result.embeddings {
                    let emb_values: Vec<Value> =
                        embedding.iter().map(|&v| Value::Float(v as f64)).collect();
                    embeddings_map.insert(node_id.to_string(), Value::Array(emb_values));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("embeddings".to_string(), Value::Map(embeddings_map));
                result_map.insert(
                    "dimension".to_string(),
                    Value::Integer(result.dimension as i64),
                );
                result_map.insert(
                    "nodeCount".to_string(),
                    Value::Integer(result.node_count as i64),
                );

                Ok(Value::Map(result_map))
            }

            _ => unreachable!("Non-walk function passed to evaluate_walk_function"),
        }
    }
}
