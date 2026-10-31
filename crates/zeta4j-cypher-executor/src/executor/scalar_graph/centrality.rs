// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 중심성 알고리즘 스칼라 함수
//!
//! pageRank, articleRank, betweenness, closeness, harmonic, eigenvector, katz, hits 등

use super::super::QueryExecutor;
use indexmap::IndexMap;
use zeta4j_cypher_core::ast::ScalarFunction;
use zeta4j_graph::algorithm::centrality::{
    ArticleRank, ArticleRankConfig, BetweennessCentrality, BetweennessConfig, ClosenessCentrality,
    ClosenessConfig, EigenvectorCentrality, EigenvectorConfig, HITS, HarmonicCentrality,
    KatzCentrality, KatzConfig, PageRank, PageRankConfig,
};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    pub(super) async fn evaluate_centrality_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
    ) -> Result<Value> {
        match func {
            ScalarFunction::PageRank => {
                // pageRank(dampingFactor?, maxIterations?, tolerance?)
                // 반환: Map { nodeId: score, ... }
                let mut config = PageRankConfig::default();

                if !args.is_empty() {
                    match &args[0] {
                        Value::Float(f) => config.damping_factor = *f,
                        Value::Integer(i) => config.damping_factor = *i as f64,
                        Value::Null => {} // 기본값 유지
                        _ => {
                            return Err(GraphError::Validation {
                                message: "pageRank() first argument (dampingFactor) must be Float"
                                    .to_string(),
                            });
                        }
                    }
                }

                if args.len() > 1 {
                    match &args[1] {
                        Value::Integer(i) => config.max_iterations = *i as usize,
                        Value::Null => {} // 기본값 유지
                        _ => {
                            return Err(GraphError::Validation {
                                message:
                                    "pageRank() second argument (maxIterations) must be Integer"
                                        .to_string(),
                            });
                        }
                    }
                }

                if args.len() > 2 {
                    match &args[2] {
                        Value::Float(f) => config.tolerance = *f,
                        Value::Null => {} // 기본값 유지
                        _ => {
                            return Err(GraphError::Validation {
                                message: "pageRank() third argument (tolerance) must be Float"
                                    .to_string(),
                            });
                        }
                    }
                }

                let pagerank = PageRank::with_config(&self.engine, config);
                let result = pagerank.calculate(None).await?;

                // Map으로 결과 반환 { "scores": Map, "iterations": Int, "converged": Bool }
                let mut scores_map = IndexMap::new();
                for (node_id, score) in result.scores {
                    scores_map.insert(node_id.to_string(), Value::Float(score));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("scores".to_string(), Value::Map(scores_map));
                result_map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                result_map.insert("converged".to_string(), Value::Boolean(result.converged));

                Ok(Value::Map(result_map))
            }

            ScalarFunction::ArticleRank => {
                // articleRank(dampingFactor?, maxIterations?, tolerance?)
                // 반환: Map { "scores": Map, "iterations": Int, "converged": Bool, "averageDegree": Float }
                let mut config = ArticleRankConfig::default();

                if !args.is_empty() {
                    match &args[0] {
                        Value::Float(f) => config.damping_factor = *f,
                        Value::Integer(i) => config.damping_factor = *i as f64,
                        Value::Null => {} // 기본값 유지
                        _ => {
                            return Err(GraphError::Validation {
                                message:
                                    "articleRank() first argument (dampingFactor) must be Float"
                                        .to_string(),
                            });
                        }
                    }
                }

                if args.len() > 1 {
                    match &args[1] {
                        Value::Integer(i) => config.max_iterations = *i as usize,
                        Value::Null => {} // 기본값 유지
                        _ => {
                            return Err(GraphError::Validation {
                                message:
                                    "articleRank() second argument (maxIterations) must be Integer"
                                        .to_string(),
                            });
                        }
                    }
                }

                if args.len() > 2 {
                    match &args[2] {
                        Value::Float(f) => config.tolerance = *f,
                        Value::Null => {} // 기본값 유지
                        _ => {
                            return Err(GraphError::Validation {
                                message: "articleRank() third argument (tolerance) must be Float"
                                    .to_string(),
                            });
                        }
                    }
                }

                let articlerank = ArticleRank::with_config(&self.engine, config);
                let result = articlerank.calculate(None).await?;

                // Map으로 결과 반환 { "scores": Map, "iterations": Int, "converged": Bool, "averageDegree": Float }
                let mut scores_map = IndexMap::new();
                for (node_id, score) in result.scores {
                    scores_map.insert(node_id.to_string(), Value::Float(score));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("scores".to_string(), Value::Map(scores_map));
                result_map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                result_map.insert("converged".to_string(), Value::Boolean(result.converged));
                result_map.insert(
                    "averageDegree".to_string(),
                    Value::Float(result.average_degree),
                );

                Ok(Value::Map(result_map))
            }

            ScalarFunction::Betweenness => {
                // betweenness(normalized?, directed?)
                // 반환: Map { nodeId: score, ... }
                let normalized = if !args.is_empty() {
                    match &args[0] {
                        Value::Boolean(b) => *b,
                        Value::Null => true,
                        _ => {
                            return Err(GraphError::Validation {
                                message:
                                    "betweenness() first argument (normalized) must be Boolean"
                                        .to_string(),
                            });
                        }
                    }
                } else {
                    true
                };

                let directed = if args.len() > 1 {
                    match &args[1] {
                        Value::Boolean(b) => *b,
                        Value::Null => false,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "betweenness() second argument (directed) must be Boolean"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    false
                };

                let config = BetweennessConfig::default()
                    .with_normalized(normalized)
                    .with_directed(directed);

                let betweenness = BetweennessCentrality::with_config(&self.engine, config);
                let result = betweenness.calculate(None).await?;

                // Map으로 결과 반환 { "1": score, "2": score, ... }
                let mut scores_map = IndexMap::new();
                for (node_id, score) in result.scores {
                    scores_map.insert(node_id.to_string(), Value::Float(score));
                }

                Ok(Value::Map(scores_map))
            }

            ScalarFunction::Closeness => {
                // closeness(normalized?, wassermanFaust?)
                // 반환: Map { nodeId: score, ... }
                let normalized = if !args.is_empty() {
                    match &args[0] {
                        Value::Boolean(b) => *b,
                        Value::Null => true,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "closeness() first argument (normalized) must be Boolean"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    true
                };

                let wasserman_faust =
                    if args.len() > 1 {
                        match &args[1] {
                            Value::Boolean(b) => *b,
                            Value::Null => true,
                            _ => return Err(GraphError::Validation {
                                message:
                                    "closeness() second argument (wassermanFaust) must be Boolean"
                                        .to_string(),
                            }),
                        }
                    } else {
                        true
                    };

                let config = ClosenessConfig::default()
                    .with_normalized(normalized)
                    .with_wasserman_faust(wasserman_faust);

                let closeness = ClosenessCentrality::with_config(&self.engine, config);
                let result = closeness.calculate(None).await?;

                // Map으로 결과 반환 { "1": score, "2": score, ... }
                let mut scores_map = IndexMap::new();
                for (node_id, score) in result.scores {
                    scores_map.insert(node_id.to_string(), Value::Float(score));
                }

                Ok(Value::Map(scores_map))
            }

            ScalarFunction::Harmonic => {
                // harmonic(normalized?)
                // 반환: Map { nodeId: score, ... }
                let normalized = if !args.is_empty() {
                    match &args[0] {
                        Value::Boolean(b) => *b,
                        Value::Null => true,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "harmonic() first argument (normalized) must be Boolean"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    true
                };

                let harmonic = HarmonicCentrality::new(&self.engine).with_normalized(normalized);
                let result = harmonic.calculate(None).await?;

                // Map으로 결과 반환 { "1": score, "2": score, ... }
                let mut scores_map = IndexMap::new();
                for (node_id, score) in result.scores {
                    scores_map.insert(node_id.to_string(), Value::Float(score));
                }

                Ok(Value::Map(scores_map))
            }

            ScalarFunction::Eigenvector => {
                // eigenvector(maxIterations?, tolerance?)
                // 반환: Map { scores: Map, iterations: Int, converged: Bool }
                let max_iterations =
                    if !args.is_empty() {
                        match &args[0] {
                            Value::Integer(i) => *i as usize,
                            Value::Null => 100,
                            _ => return Err(GraphError::Validation {
                                message:
                                    "eigenvector() first argument (maxIterations) must be Integer"
                                        .to_string(),
                            }),
                        }
                    } else {
                        100
                    };

                let tolerance = if args.len() > 1 {
                    match &args[1] {
                        Value::Float(f) => *f,
                        Value::Null => 1e-6,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "eigenvector() second argument (tolerance) must be Float"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    1e-6
                };

                let config = EigenvectorConfig::default()
                    .with_max_iterations(max_iterations)
                    .with_tolerance(tolerance);

                let eigenvector = EigenvectorCentrality::with_config(&self.engine, config);
                let result = eigenvector.calculate(None).await?;

                // 결과를 Map으로 반환
                let mut scores_map = IndexMap::new();
                for (node_id, score) in result.scores {
                    scores_map.insert(node_id.to_string(), Value::Float(score));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("scores".to_string(), Value::Map(scores_map));
                result_map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                result_map.insert("converged".to_string(), Value::Boolean(result.converged));

                Ok(Value::Map(result_map))
            }

            ScalarFunction::Katz => {
                // katz(alpha?, beta?)
                // 반환: Map { scores: Map, iterations: Int, converged: Bool, alpha: Float, beta: Float }
                let alpha = if !args.is_empty() {
                    match &args[0] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64,
                        Value::Null => 0.1,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "katz() first argument (alpha) must be Float".to_string(),
                            });
                        }
                    }
                } else {
                    0.1
                };

                let beta = if args.len() > 1 {
                    match &args[1] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64,
                        Value::Null => 1.0,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "katz() second argument (beta) must be Float".to_string(),
                            });
                        }
                    }
                } else {
                    1.0
                };

                let config = KatzConfig::default().with_alpha(alpha).with_beta(beta);

                let katz = KatzCentrality::with_config(&self.engine, config);
                let result = katz.calculate(None).await?;

                // 결과를 Map으로 반환
                let mut scores_map = IndexMap::new();
                for (node_id, score) in result.scores {
                    scores_map.insert(node_id.to_string(), Value::Float(score));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("scores".to_string(), Value::Map(scores_map));
                result_map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                result_map.insert("converged".to_string(), Value::Boolean(result.converged));
                result_map.insert("alpha".to_string(), Value::Float(result.alpha));
                result_map.insert("beta".to_string(), Value::Float(result.beta));

                Ok(Value::Map(result_map))
            }

            ScalarFunction::KatzScore => {
                // katzScore(nodeId, alpha?, beta?)
                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "katzScore() requires nodeId argument".to_string(),
                    });
                }

                let node_id = match &args[0] {
                    Value::Integer(i) => *i as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "katzScore() nodeId must be Integer".to_string(),
                        });
                    }
                };

                let alpha = if args.len() > 1 {
                    match &args[1] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64,
                        Value::Null => 0.1,
                        _ => 0.1,
                    }
                } else {
                    0.1
                };

                let beta = if args.len() > 2 {
                    match &args[2] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64,
                        Value::Null => 1.0,
                        _ => 1.0,
                    }
                } else {
                    1.0
                };

                let config = KatzConfig::default().with_alpha(alpha).with_beta(beta);

                let katz = KatzCentrality::with_config(&self.engine, config);
                let score = katz.calculate_single(node_id, None).await?;

                Ok(Value::Float(score))
            }

            ScalarFunction::Hits => {
                // hits()
                // 반환: Map { hubs: Map, authorities: Map, iterations: Int, converged: Bool }
                let hits = HITS::new(&self.engine);
                let result = hits.calculate(None).await?;

                // Hub 점수 맵
                let mut hubs_map = IndexMap::new();
                for (node_id, score) in result.hubs {
                    hubs_map.insert(node_id.to_string(), Value::Float(score));
                }

                // Authority 점수 맵
                let mut auth_map = IndexMap::new();
                for (node_id, score) in result.authorities {
                    auth_map.insert(node_id.to_string(), Value::Float(score));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("hubs".to_string(), Value::Map(hubs_map));
                result_map.insert("authorities".to_string(), Value::Map(auth_map));
                result_map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                result_map.insert("converged".to_string(), Value::Boolean(result.converged));

                Ok(Value::Map(result_map))
            }

            ScalarFunction::HubScore => {
                // hubScore(nodeId)
                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "hubScore() requires nodeId argument".to_string(),
                    });
                }

                let node_id = match &args[0] {
                    Value::Integer(i) => *i as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "hubScore() nodeId must be Integer".to_string(),
                        });
                    }
                };

                let hits = HITS::new(&self.engine);
                let score = hits.hub_score(node_id, None).await?;

                Ok(Value::Float(score))
            }

            ScalarFunction::AuthorityScore => {
                // authorityScore(nodeId)
                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "authorityScore() requires nodeId argument".to_string(),
                    });
                }

                let node_id = match &args[0] {
                    Value::Integer(i) => *i as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "authorityScore() nodeId must be Integer".to_string(),
                        });
                    }
                };

                let hits = HITS::new(&self.engine);
                let score = hits.authority_score(node_id, None).await?;

                Ok(Value::Float(score))
            }

            _ => unreachable!("Non-centrality function passed to evaluate_centrality_function"),
        }
    }
}
