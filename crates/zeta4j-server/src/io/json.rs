// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! JSON Import/Export
//!
//! 그래프 데이터의 JSON 형식 내보내기/가져오기

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};

use super::format::ValueConverter;
use zeta4j_graph::core::PropertyContainer;
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::error::{GraphError, Result};

/// JSON으로 내보낼 노드 포맷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeJson {
    /// 노드 ID
    pub id: u64,
    /// 라벨 목록
    pub labels: Vec<String>,
    /// 프로퍼티
    pub properties: HashMap<String, serde_json::Value>,
}

/// JSON으로 내보낼 엣지 포맷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeJson {
    /// 엣지 ID
    pub id: u64,
    /// 소스 노드 ID (from_node)
    pub from_node: u64,
    /// 타겟 노드 ID (to_node)
    pub to_node: u64,
    /// 관계 타입
    pub relation_type: String,
    /// 프로퍼티
    pub properties: HashMap<String, serde_json::Value>,
}

/// 그래프 메타데이터
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMetadata {
    /// 라이브러리 버전
    pub version: String,
    /// 내보낸 시간 (RFC 3339)
    pub exported_at: String,
    /// 노드 수
    pub node_count: usize,
    /// 엣지 수
    pub edge_count: usize,
}

impl Default for GraphMetadata {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            exported_at: Utc::now().to_rfc3339(),
            node_count: 0,
            edge_count: 0,
        }
    }
}

/// 전체 그래프 JSON 포맷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphJson {
    /// 메타데이터
    pub metadata: GraphMetadata,
    /// 노드 목록
    pub nodes: Vec<NodeJson>,
    /// 엣지 목록
    pub edges: Vec<EdgeJson>,
}

/// 노드만 포함하는 JSON 포맷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodesJson {
    /// 노드 목록
    pub nodes: Vec<NodeJson>,
}

/// 엣지만 포함하는 JSON 포맷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgesJson {
    /// 엣지 목록
    pub edges: Vec<EdgeJson>,
}

/// JSON Exporter
pub struct JsonExporter;

impl JsonExporter {
    /// 특정 라벨의 노드를 JSON으로 내보내기
    ///
    /// # Arguments
    /// * `engine` - GraphEngine 참조
    /// * `label` - 필터할 라벨 (None이면 전체)
    /// * `path` - 출력 파일 경로
    ///
    /// # Returns
    /// 내보낸 노드 수
    pub async fn export_nodes(
        engine: &GraphEngine,
        label: Option<&str>,
        path: &str,
    ) -> Result<usize> {
        let node_ids = match label {
            Some(l) => engine.find_nodes_by_label(l).await?,
            None => engine.all_node_ids().await,
        };

        let mut nodes = Vec::new();
        for node_id in node_ids {
            if let Ok(Some(node)) = engine.get_node(node_id).await {
                let props: HashMap<String, serde_json::Value> = node
                    .properties()
                    .iter()
                    .map(|(k, v)| (k.clone(), ValueConverter::to_json(v)))
                    .collect();

                nodes.push(NodeJson {
                    id: node.id(),
                    labels: node.labels().iter().cloned().collect(),
                    properties: props,
                });
            }
        }

        let count = nodes.len();
        let output = NodesJson { nodes };

        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &output)?;

        Ok(count)
    }

    /// 특정 타입의 엣지를 JSON으로 내보내기
    pub async fn export_edges(
        engine: &GraphEngine,
        rel_type: Option<&str>,
        path: &str,
    ) -> Result<usize> {
        let edge_ids = engine.all_edge_ids().await;

        let mut edges = Vec::new();
        for edge_id in edge_ids {
            if let Ok(Some(edge)) = engine.get_edge(edge_id).await {
                // 타입 필터링
                if let Some(rt) = rel_type
                    && edge.relation_type() != rt
                {
                    continue;
                }

                let props: HashMap<String, serde_json::Value> = edge
                    .properties()
                    .iter()
                    .map(|(k, v)| (k.clone(), ValueConverter::to_json(v)))
                    .collect();

                edges.push(EdgeJson {
                    id: edge.id(),
                    from_node: edge.from_node(),
                    to_node: edge.to_node(),
                    relation_type: edge.relation_type().to_string(),
                    properties: props,
                });
            }
        }

        let count = edges.len();
        let output = EdgesJson { edges };

        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &output)?;

        Ok(count)
    }

    /// 전체 그래프를 JSON으로 내보내기
    pub async fn export_graph(engine: &GraphEngine, path: &str) -> Result<GraphMetadata> {
        let node_ids = engine.all_node_ids().await;
        let edge_ids = engine.all_edge_ids().await;

        let mut nodes = Vec::new();
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                let props: HashMap<String, serde_json::Value> = node
                    .properties()
                    .iter()
                    .map(|(k, v)| (k.clone(), ValueConverter::to_json(v)))
                    .collect();

                nodes.push(NodeJson {
                    id: node.id(),
                    labels: node.labels().iter().cloned().collect(),
                    properties: props,
                });
            }
        }

        let mut edges = Vec::new();
        for edge_id in &edge_ids {
            if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
                let props: HashMap<String, serde_json::Value> = edge
                    .properties()
                    .iter()
                    .map(|(k, v)| (k.clone(), ValueConverter::to_json(v)))
                    .collect();

                edges.push(EdgeJson {
                    id: edge.id(),
                    from_node: edge.from_node(),
                    to_node: edge.to_node(),
                    relation_type: edge.relation_type().to_string(),
                    properties: props,
                });
            }
        }

        let metadata = GraphMetadata {
            version: env!("CARGO_PKG_VERSION").to_string(),
            exported_at: Utc::now().to_rfc3339(),
            node_count: nodes.len(),
            edge_count: edges.len(),
        };

        let output = GraphJson {
            metadata: metadata.clone(),
            nodes,
            edges,
        };

        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &output)?;
        writer.flush()?;

        Ok(metadata)
    }
}

/// JSON Importer
pub struct JsonImporter;

impl JsonImporter {
    /// JSON 파일에서 노드 가져오기
    ///
    /// # Arguments
    /// * `engine` - GraphEngine 참조
    /// * `path` - 입력 파일 경로
    ///
    /// # Returns
    /// 가져온 노드 수
    pub async fn import_nodes(engine: &GraphEngine, path: &str) -> Result<usize> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let data: NodesJson = serde_json::from_reader(reader)?;

        let mut count = 0;
        for node_json in data.nodes {
            let node_id = engine.create_node(node_json.labels).await?;
            engine
                .update_node(node_id, |node| {
                    for (key, value) in node_json.properties.iter() {
                        node.set_property(key.clone(), ValueConverter::from_json(value));
                    }
                })
                .await?;
            count += 1;
        }

        Ok(count)
    }

    /// JSON 파일에서 엣지 가져오기
    ///
    /// # Note
    /// 엣지 가져오기 전에 노드가 먼저 존재해야 합니다.
    /// 노드 ID 매핑이 필요한 경우 import_graph()를 사용하세요.
    pub async fn import_edges(engine: &GraphEngine, path: &str) -> Result<usize> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let data: EdgesJson = serde_json::from_reader(reader)?;

        let mut count = 0;
        for edge_json in data.edges {
            let edge_id = engine
                .create_edge(
                    edge_json.relation_type,
                    edge_json.from_node,
                    edge_json.to_node,
                )
                .await?;

            // 프로퍼티 설정
            if !edge_json.properties.is_empty() {
                engine
                    .update_edge(edge_id, |edge| {
                        for (key, value) in edge_json.properties.iter() {
                            edge.set_property(key.clone(), ValueConverter::from_json(value));
                        }
                    })
                    .await?;
            }
            count += 1;
        }

        Ok(count)
    }

    /// JSON 파일에서 전체 그래프 가져오기
    ///
    /// # Note
    /// 이 함수는 원본 노드 ID를 새 노드 ID로 매핑합니다.
    pub async fn import_graph(engine: &GraphEngine, path: &str) -> Result<GraphMetadata> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let data: GraphJson = serde_json::from_reader(reader)?;

        // 원본 ID → 새 ID 매핑
        let mut id_map: HashMap<u64, u64> = HashMap::new();

        // 노드 가져오기
        for node_json in &data.nodes {
            let new_id = engine.create_node(node_json.labels.clone()).await?;
            id_map.insert(node_json.id, new_id);

            engine
                .update_node(new_id, |node| {
                    for (key, value) in node_json.properties.iter() {
                        node.set_property(key.clone(), ValueConverter::from_json(value));
                    }
                })
                .await?;
        }

        // 엣지 가져오기 (ID 매핑 적용)
        for edge_json in &data.edges {
            let from_node = id_map.get(&edge_json.from_node).copied().ok_or({
                GraphError::NodeNotFound {
                    id: edge_json.from_node,
                }
            })?;
            let to_node = id_map.get(&edge_json.to_node).copied().ok_or({
                GraphError::NodeNotFound {
                    id: edge_json.to_node,
                }
            })?;

            let edge_id = engine
                .create_edge(edge_json.relation_type.clone(), from_node, to_node)
                .await?;

            // 프로퍼티 설정
            if !edge_json.properties.is_empty() {
                engine
                    .update_edge(edge_id, |edge| {
                        for (key, value) in edge_json.properties.iter() {
                            edge.set_property(key.clone(), ValueConverter::from_json(value));
                        }
                    })
                    .await?;
            }
        }

        Ok(GraphMetadata {
            version: data.metadata.version,
            exported_at: data.metadata.exported_at,
            node_count: data.nodes.len(),
            edge_count: data.edges.len(),
        })
    }
}
