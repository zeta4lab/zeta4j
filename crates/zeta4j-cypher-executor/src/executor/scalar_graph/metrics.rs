//! 그래프 메트릭 및 구조 분석 스칼라 함수
//!
//! density, triangleCount, clusteringCoefficient, diameter, radius,
//! eccentricity, articulationPoints, bridges, biconnectedComponents 등

use super::super::QueryExecutor;
use indexmap::IndexMap;
use zeta4j_cypher_core::ast::ScalarFunction;
use zeta4j_graph::algorithm::metrics::{
    ClusteringCoefficient, Density, DistanceMetrics, TriangleCount,
};
use zeta4j_graph::algorithm::structure::{
    ArticulationPoints, BiconnectedComponents, Bridges, GraphStructure, GraphStructureConfig,
};
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    pub(super) async fn evaluate_metrics_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
    ) -> Result<Value> {
        match func {
            ScalarFunction::GraphDensity => {
                // density()
                // 반환: Float (밀도 0.0 ~ 1.0)
                let density = Density::new(&self.engine).calculate().await?;
                Ok(Value::Float(density))
            }

            ScalarFunction::GraphTriangleCount => {
                // triangleCount() - 전체 삼각형 개수
                // triangleCount(node) - 노드별 삼각형 개수
                let tc = TriangleCount::new(&self.engine);

                if args.is_empty() {
                    // 전체 삼각형 개수
                    let count = tc.total_count().await?;
                    Ok(Value::Integer(count as i64))
                } else {
                    // 노드별 삼각형 개수
                    let node_id = match &args[0] {
                        Value::Node(node_id) => *node_id,
                        Value::Integer(id) => *id as u64,
                        _ => {
                            return Err(GraphError::Validation {
                                message: "triangleCount() argument must be a Node or Integer"
                                    .to_string(),
                            });
                        }
                    };
                    let count = tc.node_count(node_id).await?;
                    Ok(Value::Integer(count as i64))
                }
            }

            ScalarFunction::GraphClusteringCoefficient => {
                // clusteringCoefficient() - 글로벌 클러스터링 계수
                // clusteringCoefficient(node) - 로컬 클러스터링 계수
                let cc = ClusteringCoefficient::new(&self.engine);

                if args.is_empty() {
                    // 글로벌 클러스터링 계수
                    let coeff = cc.global().await?;
                    Ok(Value::Float(coeff))
                } else {
                    // 로컬 클러스터링 계수
                    let node_id = match &args[0] {
                        Value::Node(node_id) => *node_id,
                        Value::Integer(id) => *id as u64,
                        _ => {
                            return Err(GraphError::Validation {
                                message:
                                    "clusteringCoefficient() argument must be a Node or Integer"
                                        .to_string(),
                            });
                        }
                    };
                    let coeff = cc.local(node_id).await?;
                    Ok(Value::Float(coeff))
                }
            }

            ScalarFunction::AverageClusteringCoefficient => {
                // averageClusteringCoefficient()
                // 반환: Float (평균 클러스터링 계수)
                let cc = ClusteringCoefficient::new(&self.engine);
                let avg = cc.average().await?;
                Ok(Value::Float(avg))
            }

            // === 거리 메트릭 함수 ===
            ScalarFunction::GraphDiameter => {
                // diameter()
                // 반환: Integer (그래프 직경) 또는 Null
                let dm = DistanceMetrics::new(&self.engine);
                match dm.diameter().await? {
                    Some(d) => Ok(Value::Integer(d as i64)),
                    None => Ok(Value::Null),
                }
            }

            ScalarFunction::GraphRadius => {
                // radius()
                // 반환: Integer (그래프 반경) 또는 Null
                let dm = DistanceMetrics::new(&self.engine);
                match dm.radius().await? {
                    Some(r) => Ok(Value::Integer(r as i64)),
                    None => Ok(Value::Null),
                }
            }

            ScalarFunction::GraphEccentricity => {
                // eccentricity(node)
                // 반환: Integer (노드 이심률) 또는 Null
                if args.is_empty() {
                    return Err(GraphError::Validation {
                        message: "eccentricity() requires a node argument".to_string(),
                    });
                }

                let node_id = match &args[0] {
                    Value::Node(node_id) => *node_id,
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "eccentricity() argument must be a Node or Integer"
                                .to_string(),
                        });
                    }
                };

                let dm = DistanceMetrics::new(&self.engine);
                match dm.eccentricity(node_id).await? {
                    Some(e) => Ok(Value::Integer(e as i64)),
                    None => Ok(Value::Null),
                }
            }

            ScalarFunction::AveragePathLength => {
                // averagePathLength()
                // 반환: Float (평균 경로 길이) 또는 Null
                let dm = DistanceMetrics::new(&self.engine);
                match dm.average_path_length().await? {
                    Some(avg) => Ok(Value::Float(avg)),
                    None => Ok(Value::Null),
                }
            }

            // === 그래프 구조 분석 함수 ===
            ScalarFunction::ArticulationPoints => {
                // articulationPoints()
                // 반환: Array (단절점 노드 ID 목록)
                let ap = ArticulationPoints::new(&self.engine);
                let result = ap.find().await?;
                let points: Vec<Value> = result
                    .points
                    .iter()
                    .map(|&id| Value::Integer(id as i64))
                    .collect();
                Ok(Value::Array(points))
            }

            ScalarFunction::GraphBridges => {
                // bridges()
                // 반환: Array of Arrays (단절선 [(from, to), ...])
                let br = Bridges::new(&self.engine);
                let result = br.find().await?;
                let bridges: Vec<Value> = result
                    .bridges
                    .iter()
                    .map(|&(from, to)| {
                        Value::Array(vec![Value::Integer(from as i64), Value::Integer(to as i64)])
                    })
                    .collect();
                Ok(Value::Array(bridges))
            }

            ScalarFunction::IsArticulationPoint => {
                // isArticulationPoint(nodeId)
                // 반환: Boolean
                super::validate_arg_count("isArticulationPoint", args, 1)?;
                let node_id = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "isArticulationPoint() argument must be an integer"
                                .to_string(),
                        });
                    }
                };
                let ap = ArticulationPoints::new(&self.engine);
                let is_ap = ap.is_articulation_point(node_id).await?;
                Ok(Value::Boolean(is_ap))
            }

            ScalarFunction::IsBridge => {
                // isBridge(fromNodeId, toNodeId)
                // 반환: Boolean
                super::validate_arg_count("isBridge", args, 2)?;
                let from_id = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "isBridge() first argument must be an integer".to_string(),
                        });
                    }
                };
                let to_id = match &args[1] {
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "isBridge() second argument must be an integer".to_string(),
                        });
                    }
                };
                let br = Bridges::new(&self.engine);
                let is_bridge = br.is_bridge(from_id, to_id).await?;
                Ok(Value::Boolean(is_bridge))
            }

            ScalarFunction::BiconnectedComponents => {
                // biconnectedComponents()
                // 반환: Array of Arrays (각 컴포넌트의 노드 목록)
                let bc = BiconnectedComponents::new(&self.engine);
                let result = bc.find().await?;
                let components: Vec<Value> = result
                    .component_nodes
                    .iter()
                    .map(|nodes| {
                        let node_list: Vec<Value> =
                            nodes.iter().map(|&id| Value::Integer(id as i64)).collect();
                        Value::Array(node_list)
                    })
                    .collect();
                Ok(Value::Array(components))
            }

            ScalarFunction::BlockCutTree => {
                // blockCutTree()
                // 반환: Map { nodes: [...], edges: [...], cutVertices: [...] }
                let bc = BiconnectedComponents::new(&self.engine);
                let result = bc.build_block_cut_tree().await?;

                let mut tree_map = IndexMap::new();

                // 노드 목록 (Block 또는 CutVertex)
                let nodes: Vec<Value> = result
                    .nodes
                    .iter()
                    .map(|node| match node {
                        zeta4j_graph::algorithm::structure::BlockCutNodeType::Block(idx) => {
                            let mut m = IndexMap::new();
                            m.insert("type".to_string(), Value::String("block".to_string()));
                            m.insert("index".to_string(), Value::Integer(*idx as i64));
                            Value::Map(m)
                        }
                        zeta4j_graph::algorithm::structure::BlockCutNodeType::CutVertex(id) => {
                            let mut m = IndexMap::new();
                            m.insert("type".to_string(), Value::String("cutVertex".to_string()));
                            m.insert("nodeId".to_string(), Value::Integer(*id as i64));
                            Value::Map(m)
                        }
                    })
                    .collect();
                tree_map.insert("nodes".to_string(), Value::Array(nodes));

                // 엣지 목록
                let edges: Vec<Value> = result
                    .edges
                    .iter()
                    .map(|&(from, to)| {
                        Value::Array(vec![Value::Integer(from as i64), Value::Integer(to as i64)])
                    })
                    .collect();
                tree_map.insert("edges".to_string(), Value::Array(edges));

                // 단절점 목록
                let cut_vertices: Vec<Value> = result
                    .cut_vertices
                    .iter()
                    .map(|&id| Value::Integer(id as i64))
                    .collect();
                tree_map.insert("cutVertices".to_string(), Value::Array(cut_vertices));

                // 컴포넌트 개수
                tree_map.insert(
                    "componentCount".to_string(),
                    Value::Integer(result.components.count as i64),
                );

                Ok(Value::Map(tree_map))
            }

            ScalarFunction::GraphStructure => {
                // graphStructure()
                // 반환: Map { articulationPoints: [...], bridges: [...],
                //             biconnectedCount: n, isBiconnected: bool, isBridgeFree: bool }
                let gs = GraphStructure::with_config(&self.engine, GraphStructureConfig::default());
                let result = gs.analyze().await?;

                let mut map = IndexMap::new();

                // 단절점
                let ap: Vec<Value> = result
                    .get_critical_nodes()
                    .iter()
                    .map(|&id| Value::Integer(id as i64))
                    .collect();
                map.insert("articulationPoints".to_string(), Value::Array(ap));

                // 단절선
                let bridges: Vec<Value> = result
                    .get_critical_edges()
                    .iter()
                    .map(|&(from, to)| {
                        Value::Array(vec![Value::Integer(from as i64), Value::Integer(to as i64)])
                    })
                    .collect();
                map.insert("bridges".to_string(), Value::Array(bridges));

                // 이중 연결 성분 개수
                map.insert(
                    "biconnectedCount".to_string(),
                    Value::Integer(result.biconnected_count() as i64),
                );

                // 구조적 특성
                map.insert(
                    "isBiconnected".to_string(),
                    Value::Boolean(result.is_biconnected),
                );
                map.insert(
                    "isBridgeFree".to_string(),
                    Value::Boolean(result.is_bridge_free),
                );
                map.insert(
                    "isStructurallyRobust".to_string(),
                    Value::Boolean(result.is_structurally_robust()),
                );

                Ok(Value::Map(map))
            }

            ScalarFunction::IsStructurallyRobust => {
                // isStructurallyRobust()
                // 반환: Boolean (단절점과 단절선이 모두 없는지)
                let gs = GraphStructure::new(&self.engine);
                let is_robust = gs.is_structurally_robust().await?;
                Ok(Value::Boolean(is_robust))
            }

            _ => unreachable!("Non-metrics function passed to evaluate_metrics_function"),
        }
    }
}
