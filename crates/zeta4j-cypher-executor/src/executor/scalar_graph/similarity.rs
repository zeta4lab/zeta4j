// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 유사도 및 링크 예측 스칼라 함수
//!
//! jaccard, overlap, cosine, commonNeighbors, adamicAdar,
//! preferentialAttachment, resourceAllocation, totalNeighbors 등

use super::super::QueryExecutor;
use zeta4j_cypher_core::ast::ScalarFunction;
use zeta4j_graph::algorithm::link_prediction::{
    AdamicAdar, CommonNeighbors, PreferentialAttachment, ResourceAllocation, TotalNeighbors,
};
use zeta4j_graph::algorithm::similarity::{
    CosineSimilarity, JaccardSimilarity, OverlapCoefficient,
};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    pub(super) async fn evaluate_similarity_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
    ) -> Result<Value> {
        match func {
            ScalarFunction::Jaccard => {
                // jaccard(node1, node2)
                // 반환: Float (0.0 ~ 1.0)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "jaccard() requires two node arguments".to_string(),
                    });
                }

                let node1_id = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "jaccard() first argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let node2_id = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "jaccard() second argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let jaccard = JaccardSimilarity::new(&self.engine);
                let similarity = jaccard.similarity(node1_id, node2_id).await?;

                Ok(Value::Float(similarity))
            }

            ScalarFunction::Overlap => {
                // overlap(node1, node2)
                // 반환: Float (0.0 ~ 1.0)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "overlap() requires two node arguments".to_string(),
                    });
                }

                let node1_id = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "overlap() first argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let node2_id = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "overlap() second argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let overlap = OverlapCoefficient::new(&self.engine);
                let coefficient = overlap.coefficient(node1_id, node2_id).await?;

                Ok(Value::Float(coefficient))
            }

            ScalarFunction::Cosine => {
                // cosine(node1, node2)
                // 반환: Float (0.0 ~ 1.0)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "cosine() requires two node arguments".to_string(),
                    });
                }

                let node1_id = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "cosine() first argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let node2_id = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "cosine() second argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let cosine = CosineSimilarity::new(&self.engine);
                let similarity = cosine.similarity_neighbors(node1_id, node2_id).await?;

                Ok(Value::Float(similarity))
            }

            // === 링크 예측 함수 ===
            ScalarFunction::CommonNeighbors => {
                // commonNeighbors(node1, node2)
                // 반환: Float (공통 이웃 수)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "commonNeighbors() requires two node arguments".to_string(),
                    });
                }

                let node1_id = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "commonNeighbors() first argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let node2_id = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "commonNeighbors() second argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let cn = CommonNeighbors::new(&self.engine);
                let score = cn.score(node1_id, node2_id).await?;

                Ok(Value::Float(score))
            }

            ScalarFunction::AdamicAdar => {
                // adamicAdar(node1, node2)
                // 반환: Float (Adamic-Adar 점수)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "adamicAdar() requires two node arguments".to_string(),
                    });
                }

                let node1_id = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "adamicAdar() first argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let node2_id = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "adamicAdar() second argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let aa = AdamicAdar::new(&self.engine);
                let score = aa.score(node1_id, node2_id).await?;

                Ok(Value::Float(score))
            }

            ScalarFunction::PreferentialAttachment => {
                // preferentialAttachment(node1, node2)
                // 반환: Float (PA 점수)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "preferentialAttachment() requires two node arguments".to_string(),
                    });
                }

                let node1_id =
                    match &args[0] {
                        Value::Node(node_id) => *node_id,
                        Value::Integer(id) => *id as u64,
                        _ => return Err(GraphError::Validation {
                            message:
                                "preferentialAttachment() first argument must be a Node or Integer"
                                    .to_string(),
                        }),
                    };

                let node2_id =
                    match &args[1] {
                        Value::Node(node_id) => *node_id,
                        Value::Integer(id) => *id as u64,
                        _ => return Err(GraphError::Validation {
                            message:
                                "preferentialAttachment() second argument must be a Node or Integer"
                                    .to_string(),
                        }),
                    };

                let pa = PreferentialAttachment::new(&self.engine);
                let score = pa.score(node1_id, node2_id).await?;

                Ok(Value::Float(score))
            }

            ScalarFunction::ResourceAllocation => {
                // resourceAllocation(node1, node2)
                // 반환: Float (RA 점수)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "resourceAllocation() requires two node arguments".to_string(),
                    });
                }

                let node1_id = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message:
                                "resourceAllocation() first argument must be a Node or Integer"
                                    .to_string(),
                        });
                    }
                };

                let node2_id = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message:
                                "resourceAllocation() second argument must be a Node or Integer"
                                    .to_string(),
                        });
                    }
                };

                let ra = ResourceAllocation::new(&self.engine);
                let score = ra.score(node1_id, node2_id).await?;

                Ok(Value::Float(score))
            }

            ScalarFunction::TotalNeighbors => {
                // totalNeighbors(node1, node2)
                // 반환: Float (TN 점수)
                if args.len() < 2 {
                    return Err(GraphError::Validation {
                        message: "totalNeighbors() requires two node arguments".to_string(),
                    });
                }

                let node1_id = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "totalNeighbors() first argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let node2_id = match &args[1] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "totalNeighbors() second argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let tn = TotalNeighbors::new(&self.engine);
                let score = tn.score(node1_id, node2_id).await?;

                Ok(Value::Float(score))
            }

            _ => unreachable!("Non-similarity function passed to evaluate_similarity_function"),
        }
    }
}
