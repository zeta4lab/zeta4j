// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! K-Core 분석 스칼라 함수
//!
//! kCoreNodes, coreNumber, coreNumbers, degeneracy,
//! kShellNodes, shellDecomposition, kCoreAnalysis 등

use super::super::QueryExecutor;
use indexmap::IndexMap;
use zeta4j_cypher_core::ast::ScalarFunction;
use zeta4j_graph::algorithm::kcore::KCore;
use zeta4j_graph::core::Value;
use zeta4j_graph::error::{GraphError, Result};

impl QueryExecutor {
    pub(super) async fn evaluate_kcore_function(
        &self,
        func: &ScalarFunction,
        args: &[Value],
    ) -> Result<Value> {
        match func {
            ScalarFunction::KCoreNodes => {
                // kCore(k)
                // 반환: Array (k-core에 속하는 노드 목록)
                super::validate_arg_count("kCore", args, 1)?;
                let k = match &args[0] {
                    Value::Integer(k) => *k as usize,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "kCore() argument must be an integer".to_string(),
                        });
                    }
                };
                let kcore = KCore::new(&self.engine);
                let result = kcore.find_k_core(k).await?;
                let nodes: Vec<Value> = result
                    .nodes
                    .iter()
                    .map(|&id| Value::Integer(id as i64))
                    .collect();
                Ok(Value::Array(nodes))
            }

            ScalarFunction::CoreNumber => {
                // coreNumber(nodeId)
                // 반환: Integer (노드의 core number)
                super::validate_arg_count("coreNumber", args, 1)?;
                let node_id = match &args[0] {
                    Value::Integer(id) => *id as u64,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "coreNumber() argument must be an integer".to_string(),
                        });
                    }
                };
                let kcore = KCore::new(&self.engine);
                let core = kcore.node_core_number(node_id).await?;
                Ok(Value::Integer(core as i64))
            }

            ScalarFunction::CoreNumbers => {
                // coreNumbers()
                // 반환: Map (nodeId -> coreNumber)
                let kcore = KCore::new(&self.engine);
                let result = kcore.compute_core_numbers().await?;
                let map: IndexMap<String, Value> = result
                    .core_numbers
                    .iter()
                    .map(|(&node, &core)| (node.to_string(), Value::Integer(core as i64)))
                    .collect();
                Ok(Value::Map(map))
            }

            ScalarFunction::GraphDegeneracy => {
                // degeneracy()
                // 반환: Integer (그래프 퇴화도)
                let kcore = KCore::new(&self.engine);
                let deg = kcore.degeneracy().await?;
                Ok(Value::Integer(deg as i64))
            }

            ScalarFunction::KShellNodes => {
                // kShell(k)
                // 반환: Array (k-shell 노드 ID 목록)
                super::validate_arg_count("kShell", args, 1)?;
                let k = match &args[0] {
                    Value::Integer(k) => *k as usize,
                    _ => {
                        return Err(GraphError::Validation {
                            message: "kShell: k must be an integer".to_string(),
                        });
                    }
                };

                let kcore = KCore::new(&self.engine);
                let result = kcore.find_k_shell(k).await?;

                let nodes: Vec<Value> = result
                    .nodes
                    .iter()
                    .map(|&id| Value::Integer(id as i64))
                    .collect();

                Ok(Value::Array(nodes))
            }

            ScalarFunction::ShellDecomposition => {
                // shellDecomposition()
                // 반환: Map (shell -> 노드 목록)
                let kcore = KCore::new(&self.engine);
                let result = kcore.decompose_shells().await?;

                let mut map = IndexMap::new();
                for (shell, nodes) in result.shells {
                    let node_values: Vec<Value> =
                        nodes.iter().map(|&id| Value::Integer(id as i64)).collect();
                    map.insert(shell.to_string(), Value::Array(node_values));
                }

                Ok(Value::Map(map))
            }

            ScalarFunction::KCoreAnalysis => {
                // kCoreAnalysis()
                // 반환: Map (통합 분석 결과)
                let kcore = KCore::new(&self.engine);
                let result = kcore.analyze().await?;

                let mut map = IndexMap::new();

                // degeneracy
                map.insert(
                    "degeneracy".to_string(),
                    Value::Integer(result.degeneracy as i64),
                );

                // nodeCount
                map.insert(
                    "nodeCount".to_string(),
                    Value::Integer(result.node_count as i64),
                );

                // shellCount
                map.insert(
                    "shellCount".to_string(),
                    Value::Integer(result.shell_count as i64),
                );

                // mainCore (노드 목록)
                let main_core_nodes: Vec<Value> = result
                    .main_core
                    .nodes
                    .iter()
                    .map(|&id| Value::Integer(id as i64))
                    .collect();
                map.insert("mainCore".to_string(), Value::Array(main_core_nodes));

                // mainCoreSize
                map.insert(
                    "mainCoreSize".to_string(),
                    Value::Integer(result.main_core.node_count as i64),
                );

                // shells (shell -> 노드 목록)
                let mut shells_map = IndexMap::new();
                for (shell, nodes) in result.shells.shells {
                    let node_values: Vec<Value> =
                        nodes.iter().map(|&id| Value::Integer(id as i64)).collect();
                    shells_map.insert(shell.to_string(), Value::Array(node_values));
                }
                map.insert("shells".to_string(), Value::Map(shells_map));

                Ok(Value::Map(map))
            }

            _ => unreachable!("Non-kcore function passed to evaluate_kcore_function"),
        }
    }
}
