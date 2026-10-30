//! 커뮤니티 탐지 알고리즘 스칼라 함수
//!
//! connectedComponents, stronglyConnectedComponents, louvain, modularityOptimization,
//! labelPropagation, lpaAnalyze 등

use super::super::QueryExecutor;
use indexmap::IndexMap;
use zeta4j_cypher_core::ast::ScalarFunction;
use zeta4j_graph::algorithm::community::{
    ConnectedComponents, LabelPropagation, LabelPropagationConfig, Louvain, LouvainConfig,
    ModularityConfig, ModularityOptimization, StronglyConnectedComponents,
};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    pub(super) async fn evaluate_community_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
    ) -> Result<Value> {
        match func {
            ScalarFunction::ConnectedComponents => {
                // connectedComponents()
                // 반환: Map { components: Map<NodeId, ComponentId>, componentCount: Int, componentSizes: Map }
                let wcc = ConnectedComponents::new(&self.engine);
                let result = wcc.calculate().await?;

                // 컴포넌트 매핑
                let mut components_map = IndexMap::new();
                for (node_id, component_id) in &result.components {
                    components_map
                        .insert(node_id.to_string(), Value::Integer(*component_id as i64));
                }

                // 컴포넌트 크기 매핑
                let mut sizes_map = IndexMap::new();
                for (component_id, size) in &result.component_sizes {
                    sizes_map.insert(component_id.to_string(), Value::Integer(*size as i64));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("components".to_string(), Value::Map(components_map));
                result_map.insert(
                    "componentCount".to_string(),
                    Value::Integer(result.component_count as i64),
                );
                result_map.insert("componentSizes".to_string(), Value::Map(sizes_map));

                Ok(Value::Map(result_map))
            }

            ScalarFunction::StronglyConnectedComponents => {
                // stronglyConnectedComponents()
                // 반환: Map { components: Map<NodeId, ComponentId>, componentCount: Int, componentSizes: Map }
                let scc = StronglyConnectedComponents::new(&self.engine);
                let result = scc.calculate().await?;

                // 컴포넌트 매핑
                let mut components_map = IndexMap::new();
                for (node_id, component_id) in &result.components {
                    components_map
                        .insert(node_id.to_string(), Value::Integer(*component_id as i64));
                }

                // 컴포넌트 크기 매핑
                let mut sizes_map = IndexMap::new();
                for (component_id, size) in &result.component_sizes {
                    sizes_map.insert(component_id.to_string(), Value::Integer(*size as i64));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("components".to_string(), Value::Map(components_map));
                result_map.insert(
                    "componentCount".to_string(),
                    Value::Integer(result.component_count as i64),
                );
                result_map.insert("componentSizes".to_string(), Value::Map(sizes_map));

                Ok(Value::Map(result_map))
            }

            ScalarFunction::Louvain => {
                // louvain(maxIterations?, resolution?)
                // 반환: Map { communities, communityCount, modularity, iterations, communitySizes }
                let max_iterations = if !args.is_empty() {
                    match &args[0] {
                        Value::Integer(i) => *i as usize,
                        Value::Null => 10,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "louvain() first argument (maxIterations) must be Integer"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    10
                };

                let resolution = if args.len() > 1 {
                    match &args[1] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64,
                        Value::Null => 1.0,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "louvain() second argument (resolution) must be Float"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    1.0
                };

                let config = LouvainConfig::default()
                    .with_max_iterations(max_iterations)
                    .with_resolution(resolution);

                let louvain = Louvain::with_config(&self.engine, config);
                let result = louvain.calculate(None).await?;

                // 커뮤니티 매핑
                let mut communities_map = IndexMap::new();
                for (node_id, community_id) in &result.communities {
                    communities_map
                        .insert(node_id.to_string(), Value::Integer(*community_id as i64));
                }

                // 커뮤니티 크기 매핑
                let mut sizes_map = IndexMap::new();
                for (community_id, size) in &result.community_sizes {
                    sizes_map.insert(community_id.to_string(), Value::Integer(*size as i64));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("communities".to_string(), Value::Map(communities_map));
                result_map.insert(
                    "communityCount".to_string(),
                    Value::Integer(result.community_count as i64),
                );
                result_map.insert("modularity".to_string(), Value::Float(result.modularity));
                result_map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                result_map.insert("communitySizes".to_string(), Value::Map(sizes_map));

                Ok(Value::Map(result_map))
            }

            ScalarFunction::ModularityOptimization => {
                // modularityOptimization(maxIterations?, tolerance?, resolution?)
                // 반환: Map { communities, communityCount, modularity, iterations, converged, improvement, communitySizes }
                let max_iterations = if !args.is_empty() {
                    match &args[0] {
                        Value::Integer(i) => *i as usize,
                        Value::Null => 10,
                        _ => return Err(GraphError::Validation {
                            message: "modularityOptimization() first argument (maxIterations) must be Integer".to_string(),
                        }),
                    }
                } else {
                    10
                };

                let tolerance = if args.len() > 1 {
                    match &args[1] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64,
                        Value::Null => 1e-7,
                        _ => return Err(GraphError::Validation {
                            message:
                                "modularityOptimization() second argument (tolerance) must be Float"
                                    .to_string(),
                        }),
                    }
                } else {
                    1e-7
                };

                let resolution = if args.len() > 2 {
                    match &args[2] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64,
                        Value::Null => 1.0,
                        _ => return Err(GraphError::Validation {
                            message:
                                "modularityOptimization() third argument (resolution) must be Float"
                                    .to_string(),
                        }),
                    }
                } else {
                    1.0
                };

                let config = ModularityConfig::default()
                    .with_max_iterations(max_iterations)
                    .with_tolerance(tolerance)
                    .with_resolution(resolution);

                let optimizer = ModularityOptimization::with_config(&self.engine, config);
                let result = optimizer.optimize(None).await?;

                // 커뮤니티 매핑
                let mut communities_map = IndexMap::new();
                for (node_id, community_id) in &result.communities {
                    communities_map
                        .insert(node_id.to_string(), Value::Integer(*community_id as i64));
                }

                // 커뮤니티 크기 매핑
                let mut sizes_map = IndexMap::new();
                for (community_id, size) in &result.community_sizes {
                    sizes_map.insert(community_id.to_string(), Value::Integer(*size as i64));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("communities".to_string(), Value::Map(communities_map));
                result_map.insert(
                    "communityCount".to_string(),
                    Value::Integer(result.community_count as i64),
                );
                result_map.insert("modularity".to_string(), Value::Float(result.modularity));
                result_map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                result_map.insert("converged".to_string(), Value::Boolean(result.converged));
                result_map.insert("improvement".to_string(), Value::Float(result.improvement));
                result_map.insert("communitySizes".to_string(), Value::Map(sizes_map));

                Ok(Value::Map(result_map))
            }

            ScalarFunction::LabelPropagation | ScalarFunction::Lpa => {
                // labelPropagation(maxIterations?, async?, weightProperty?, convergenceThreshold?)
                // 또는 lpa(maxIterations?, async?, weightProperty?, convergenceThreshold?)
                // 반환: Map { labels, communities, communityCount, iterations, converged, nodeCount }

                // 첫 번째 인자: maxIterations (기본값: 100)
                let max_iterations = if !args.is_empty() {
                    match &args[0] {
                        Value::Integer(i) => *i as usize,
                        Value::Null => 100,
                        _ => return Err(GraphError::Validation {
                            message:
                                "labelPropagation() first argument (maxIterations) must be Integer"
                                    .to_string(),
                        }),
                    }
                } else {
                    100
                };

                // 두 번째 인자: async 모드 (기본값: true)
                let async_update = if args.len() > 1 {
                    match &args[1] {
                        Value::Boolean(b) => *b,
                        Value::Null => true,
                        _ => {
                            return Err(GraphError::Validation {
                                message:
                                    "labelPropagation() second argument (async) must be Boolean"
                                        .to_string(),
                            });
                        }
                    }
                } else {
                    true
                };

                // 세 번째 인자: weightProperty (기본값: None)
                let weight_property = if args.len() > 2 {
                    match &args[2] {
                        Value::String(s) => Some(s.clone()),
                        Value::Null => None,
                        _ => return Err(GraphError::Validation {
                            message:
                                "labelPropagation() third argument (weightProperty) must be String"
                                    .to_string(),
                        }),
                    }
                } else {
                    None
                };

                // 네 번째 인자: convergenceThreshold (기본값: 0.0)
                let convergence_threshold = if args.len() > 3 {
                    match &args[3] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64,
                        Value::Null => 0.0,
                        _ => return Err(GraphError::Validation {
                            message: "labelPropagation() fourth argument (convergenceThreshold) must be Float".to_string(),
                        }),
                    }
                } else {
                    0.0
                };

                let mut config = LabelPropagationConfig::default()
                    .with_max_iterations(max_iterations)
                    .with_async_update(async_update)
                    .with_convergence_threshold(convergence_threshold);

                if let Some(prop) = weight_property {
                    config = config.with_weight_property(prop);
                }

                let lpa = LabelPropagation::with_config(&self.engine, config);
                let result = lpa.detect().await?;

                // 레이블 매핑
                let mut labels_map = IndexMap::new();
                for (node_id, label) in &result.labels {
                    labels_map.insert(node_id.to_string(), Value::Integer(*label as i64));
                }

                // 커뮤니티 목록 매핑
                let mut communities_map = IndexMap::new();
                for (label, nodes) in &result.communities {
                    let node_list: Vec<Value> =
                        nodes.iter().map(|&n| Value::Integer(n as i64)).collect();
                    communities_map.insert(label.to_string(), Value::Array(node_list));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("labels".to_string(), Value::Map(labels_map));
                result_map.insert("communities".to_string(), Value::Map(communities_map));
                result_map.insert(
                    "communityCount".to_string(),
                    Value::Integer(result.community_count as i64),
                );
                result_map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                result_map.insert("converged".to_string(), Value::Boolean(result.converged));
                result_map.insert(
                    "nodeCount".to_string(),
                    Value::Integer(result.node_count as i64),
                );

                Ok(Value::Map(result_map))
            }

            ScalarFunction::LpaAnalyze => {
                // lpaAnalyze(maxIterations?, async?, weightProperty?, convergenceThreshold?)
                // 반환: Map { labels, communities, communityCount, iterations, converged, nodeCount, statistics }
                use zeta4j_graph::algorithm::LabelPropagationAnalyzer;

                // 첫 번째 인자: maxIterations (기본값: 100)
                let max_iterations =
                    if !args.is_empty() {
                        match &args[0] {
                            Value::Integer(i) => *i as usize,
                            Value::Null => 100,
                            _ => return Err(GraphError::Validation {
                                message:
                                    "lpaAnalyze() first argument (maxIterations) must be Integer"
                                        .to_string(),
                            }),
                        }
                    } else {
                        100
                    };

                // 두 번째 인자: async 모드 (기본값: true)
                let async_update = if args.len() > 1 {
                    match &args[1] {
                        Value::Boolean(b) => *b,
                        Value::Null => true,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "lpaAnalyze() second argument (async) must be Boolean"
                                    .to_string(),
                            });
                        }
                    }
                } else {
                    true
                };

                // 세 번째 인자: weightProperty (기본값: None)
                let weight_property =
                    if args.len() > 2 {
                        match &args[2] {
                            Value::String(s) => Some(s.clone()),
                            Value::Null => None,
                            _ => return Err(GraphError::Validation {
                                message:
                                    "lpaAnalyze() third argument (weightProperty) must be String"
                                        .to_string(),
                            }),
                        }
                    } else {
                        None
                    };

                // 네 번째 인자: convergenceThreshold (기본값: 0.0)
                let convergence_threshold = if args.len() > 3 {
                    match &args[3] {
                        Value::Float(f) => *f,
                        Value::Integer(i) => *i as f64,
                        Value::Null => 0.0,
                        _ => return Err(GraphError::Validation {
                            message:
                                "lpaAnalyze() fourth argument (convergenceThreshold) must be Float"
                                    .to_string(),
                        }),
                    }
                } else {
                    0.0
                };

                let mut config = LabelPropagationConfig::default()
                    .with_max_iterations(max_iterations)
                    .with_async_update(async_update)
                    .with_convergence_threshold(convergence_threshold);

                if let Some(prop) = weight_property {
                    config = config.with_weight_property(prop);
                }

                let analyzer = LabelPropagationAnalyzer::with_config(&self.engine, config).await?;
                let result = analyzer.result();
                let stats = analyzer.statistics().await?;

                // 레이블 매핑
                let mut labels_map = IndexMap::new();
                for (node_id, label) in &result.labels {
                    labels_map.insert(node_id.to_string(), Value::Integer(*label as i64));
                }

                // 커뮤니티 목록 매핑
                let mut communities_map = IndexMap::new();
                for (label, nodes) in &result.communities {
                    let node_list: Vec<Value> =
                        nodes.iter().map(|&n| Value::Integer(n as i64)).collect();
                    communities_map.insert(label.to_string(), Value::Array(node_list));
                }

                // 통계 정보 매핑
                let mut stats_map = IndexMap::new();
                stats_map.insert(
                    "averageCommunitySize".to_string(),
                    Value::Float(stats.average_community_size),
                );
                stats_map.insert(
                    "communitySizeStdDev".to_string(),
                    Value::Float(stats.community_size_std_dev),
                );
                stats_map.insert(
                    "singletonCount".to_string(),
                    Value::Integer(stats.singleton_count as i64),
                );
                stats_map.insert("coverage".to_string(), Value::Float(stats.coverage));
                stats_map.insert(
                    "averageCommunityStrength".to_string(),
                    Value::Float(stats.average_community_strength),
                );

                if let Some((label, size)) = stats.largest_community {
                    let mut largest_map = IndexMap::new();
                    largest_map.insert("label".to_string(), Value::Integer(label as i64));
                    largest_map.insert("size".to_string(), Value::Integer(size as i64));
                    stats_map.insert("largestCommunity".to_string(), Value::Map(largest_map));
                }

                if let Some((label, size)) = stats.smallest_community {
                    let mut smallest_map = IndexMap::new();
                    smallest_map.insert("label".to_string(), Value::Integer(label as i64));
                    smallest_map.insert("size".to_string(), Value::Integer(size as i64));
                    stats_map.insert("smallestCommunity".to_string(), Value::Map(smallest_map));
                }

                let mut result_map = IndexMap::new();
                result_map.insert("labels".to_string(), Value::Map(labels_map));
                result_map.insert("communities".to_string(), Value::Map(communities_map));
                result_map.insert(
                    "communityCount".to_string(),
                    Value::Integer(result.community_count as i64),
                );
                result_map.insert(
                    "iterations".to_string(),
                    Value::Integer(result.iterations as i64),
                );
                result_map.insert("converged".to_string(), Value::Boolean(result.converged));
                result_map.insert(
                    "nodeCount".to_string(),
                    Value::Integer(result.node_count as i64),
                );
                result_map.insert("statistics".to_string(), Value::Map(stats_map));

                Ok(Value::Map(result_map))
            }

            _ => unreachable!("Non-community function passed to evaluate_community_function"),
        }
    }
}
