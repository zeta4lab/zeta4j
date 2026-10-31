// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 대용량 파일 스트리밍 Import/Export
//!
//! 메모리 효율적인 청크 단위 처리로 대용량 파일을 처리합니다.
//!
//! # 주요 기능
//! - 스트리밍 CSV Export: 노드/엣지를 한 번에 하나씩 쓰기
//! - NDJSON Import/Export: Newline Delimited JSON 형식 지원
//! - 진행 상황 콜백: 대용량 파일 처리 진행률 추적
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::io::streaming::{StreamingOptions, StreamingCsvExporter, NdjsonExporter, NdjsonImporter};
//!
//! // 진행 상황 콜백 설정
//! let options = StreamingOptions::new()
//!     .with_progress_callback(|processed, total| {
//!         println!("Progress: {}/{}", processed, total);
//!     })
//!     .with_chunk_size(1000);
//!
//! // 스트리밍 CSV Export (메모리 O(1))
//! let count = StreamingCsvExporter::export_nodes(&engine, None, "nodes.csv", options)?;
//!
//! // NDJSON Export (한 줄에 하나의 JSON 객체)
//! let count = NdjsonExporter::export_nodes(&engine, None, "nodes.ndjson", options)?;
//!
//! // NDJSON Import (스트리밍)
//! let count = NdjsonImporter::import_nodes(&engine, "nodes.ndjson", options)?;
//! ```

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::csv::CsvOptions;
use super::format::ValueConverter;
use zeta4j_graph::core::PropertyContainer;
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::error::{GraphError, Result};

/// 진행 상황 콜백 타입
/// (처리된 수, 전체 수) -> ()
pub type ProgressCallback = Arc<dyn Fn(usize, Option<usize>) + Send + Sync>;

/// 스트리밍 옵션
#[derive(Clone)]
pub struct StreamingOptions {
    /// 청크 크기 (기본: 1000)
    pub chunk_size: usize,
    /// 진행 상황 콜백 (매 chunk_size 마다 호출)
    pub progress_callback: Option<ProgressCallback>,
    /// CSV 옵션
    pub csv_options: CsvOptions,
    /// 버퍼 크기 (기본: 64KB)
    pub buffer_size: usize,
}

impl Default for StreamingOptions {
    fn default() -> Self {
        Self {
            chunk_size: 1000,
            progress_callback: None,
            csv_options: CsvOptions::default(),
            buffer_size: 64 * 1024,
        }
    }
}

impl StreamingOptions {
    /// 새 스트리밍 옵션 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 청크 크기 설정
    pub fn with_chunk_size(mut self, size: usize) -> Self {
        self.chunk_size = size.max(1);
        self
    }

    /// 진행 상황 콜백 설정
    pub fn with_progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize, Option<usize>) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// CSV 옵션 설정
    pub fn with_csv_options(mut self, options: CsvOptions) -> Self {
        self.csv_options = options;
        self
    }

    /// 버퍼 크기 설정
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size.max(1024);
        self
    }

    /// 진행 상황 보고
    fn report_progress(&self, processed: usize, total: Option<usize>) {
        if let Some(ref callback) = self.progress_callback {
            callback(processed, total);
        }
    }
}

/// 스트리밍 CSV Exporter
///
/// 메모리 효율적인 CSV 내보내기. 노드/엣지를 한 번에 하나씩 쓰기하여
/// O(1) 메모리로 대용량 데이터를 처리합니다.
pub struct StreamingCsvExporter;

impl StreamingCsvExporter {
    /// 노드를 스트리밍 방식으로 CSV로 내보내기
    ///
    /// 2-패스 알고리즘:
    /// 1. 첫 번째 패스: 모든 프로퍼티 키 수집 (헤더용)
    /// 2. 두 번째 패스: 노드 데이터 쓰기
    pub async fn export_nodes(
        engine: &GraphEngine,
        label: Option<&str>,
        path: &str,
        options: StreamingOptions,
    ) -> Result<usize> {
        let node_ids: Vec<u64> = match label {
            Some(l) => engine.find_nodes_by_label(l).await?,
            None => engine.all_node_ids().await,
        };

        let total = node_ids.len();
        if total == 0 {
            return Ok(0);
        }

        // 1단계: 모든 프로퍼티 키 수집 (첫 번째 패스)
        let mut all_props: HashSet<String> = HashSet::new();
        for (i, node_id) in node_ids.iter().enumerate() {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                for key in node.properties().keys() {
                    all_props.insert(key.clone());
                }
            }
            // 첫 번째 패스 진행 상황 보고 (50%로 표시)
            if (i + 1) % options.chunk_size == 0 {
                options.report_progress(i.div_ceil(2), Some(total));
            }
        }

        let mut prop_keys: Vec<String> = all_props.into_iter().collect();
        prop_keys.sort();

        // 2단계: CSV 파일 스트리밍 쓰기
        let file = File::create(path)?;
        let writer = BufWriter::with_capacity(options.buffer_size, file);
        let mut csv_writer = csv::WriterBuilder::new()
            .delimiter(options.csv_options.delimiter)
            .quote(options.csv_options.quote)
            .from_writer(writer);

        // 헤더 작성
        if options.csv_options.has_header {
            let mut header = vec!["id".to_string(), "labels".to_string()];
            header.extend(prop_keys.iter().cloned());
            csv_writer
                .write_record(&header)
                .map_err(|e| GraphError::Storage {
                    message: format!("Failed to write CSV header: {}", e),
                })?;
        }

        // 데이터 스트리밍 쓰기 (두 번째 패스)
        let mut count = 0;
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                let labels: Vec<String> = node.labels().iter().cloned().collect();
                let labels_str = labels.join(&options.csv_options.label_delimiter.to_string());

                let mut record = vec![node.id().to_string(), labels_str];
                for key in &prop_keys {
                    let value = node
                        .properties()
                        .get(key)
                        .map(ValueConverter::to_csv_string)
                        .unwrap_or_default();
                    record.push(value);
                }

                csv_writer
                    .write_record(&record)
                    .map_err(|e| GraphError::Storage {
                        message: format!("Failed to write CSV record: {}", e),
                    })?;

                count += 1;

                // 진행 상황 보고
                if count % options.chunk_size == 0 {
                    options.report_progress(total / 2 + count / 2, Some(total));
                }
            }
        }

        csv_writer.flush().map_err(|e| GraphError::Storage {
            message: format!("Failed to flush CSV: {}", e),
        })?;

        options.report_progress(total, Some(total));
        Ok(count)
    }

    /// 엣지를 스트리밍 방식으로 CSV로 내보내기
    pub async fn export_edges(
        engine: &GraphEngine,
        rel_type: Option<&str>,
        path: &str,
        options: StreamingOptions,
    ) -> Result<usize> {
        let edge_ids = engine.all_edge_ids().await;

        // 타입 필터링된 엣지 수집
        let filtered_ids: Vec<u64> = if let Some(rt) = rel_type {
            let mut ids = Vec::new();
            for id in &edge_ids {
                if let Ok(Some(edge)) = engine.get_edge(*id).await
                    && edge.relation_type() == rt
                {
                    ids.push(*id);
                }
            }
            ids
        } else {
            edge_ids
        };

        let total = filtered_ids.len();
        if total == 0 {
            return Ok(0);
        }

        // 1단계: 모든 프로퍼티 키 수집
        let mut all_props: HashSet<String> = HashSet::new();
        for edge_id in &filtered_ids {
            if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
                for key in edge.properties().keys() {
                    all_props.insert(key.clone());
                }
            }
        }

        let mut prop_keys: Vec<String> = all_props.into_iter().collect();
        prop_keys.sort();

        // 2단계: CSV 파일 스트리밍 쓰기
        let file = File::create(path)?;
        let writer = BufWriter::with_capacity(options.buffer_size, file);
        let mut csv_writer = csv::WriterBuilder::new()
            .delimiter(options.csv_options.delimiter)
            .quote(options.csv_options.quote)
            .from_writer(writer);

        // 헤더 작성
        if options.csv_options.has_header {
            let mut header = vec![
                "id".to_string(),
                "from_node".to_string(),
                "to_node".to_string(),
                "relation_type".to_string(),
            ];
            header.extend(prop_keys.iter().cloned());
            csv_writer
                .write_record(&header)
                .map_err(|e| GraphError::Storage {
                    message: format!("Failed to write CSV header: {}", e),
                })?;
        }

        // 데이터 스트리밍 쓰기
        let mut count = 0;
        for edge_id in &filtered_ids {
            if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
                let mut record = vec![
                    edge.id().to_string(),
                    edge.from_node().to_string(),
                    edge.to_node().to_string(),
                    edge.relation_type().to_string(),
                ];

                for key in &prop_keys {
                    let value = edge
                        .properties()
                        .get(key)
                        .map(ValueConverter::to_csv_string)
                        .unwrap_or_default();
                    record.push(value);
                }

                csv_writer
                    .write_record(&record)
                    .map_err(|e| GraphError::Storage {
                        message: format!("Failed to write CSV record: {}", e),
                    })?;

                count += 1;

                if count % options.chunk_size == 0 {
                    options.report_progress(count, Some(total));
                }
            }
        }

        csv_writer.flush().map_err(|e| GraphError::Storage {
            message: format!("Failed to flush CSV: {}", e),
        })?;

        options.report_progress(total, Some(total));
        Ok(count)
    }
}

/// NDJSON (Newline Delimited JSON) 노드 형식
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NdjsonNode {
    /// 노드 ID (가져오기 시 무시됨)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// 라벨 목록
    pub labels: Vec<String>,
    /// 프로퍼티
    pub properties: HashMap<String, serde_json::Value>,
}

/// NDJSON 엣지 형식
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NdjsonEdge {
    /// 엣지 ID (가져오기 시 무시됨)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// 소스 노드 ID
    pub from_node: u64,
    /// 타겟 노드 ID
    pub to_node: u64,
    /// 관계 타입
    pub relation_type: String,
    /// 프로퍼티
    pub properties: HashMap<String, serde_json::Value>,
}

/// NDJSON Exporter
///
/// Newline Delimited JSON 형식으로 내보내기.
/// 각 줄에 하나의 JSON 객체를 쓰므로 스트리밍 가져오기가 가능합니다.
pub struct NdjsonExporter;

impl NdjsonExporter {
    /// 노드를 NDJSON으로 내보내기
    pub async fn export_nodes(
        engine: &GraphEngine,
        label: Option<&str>,
        path: &str,
        options: StreamingOptions,
    ) -> Result<usize> {
        let node_ids: Vec<u64> = match label {
            Some(l) => engine.find_nodes_by_label(l).await?,
            None => engine.all_node_ids().await,
        };

        let total = node_ids.len();
        if total == 0 {
            return Ok(0);
        }

        let file = File::create(path)?;
        let mut writer = BufWriter::with_capacity(options.buffer_size, file);

        let mut count = 0;
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                let props: HashMap<String, serde_json::Value> = node
                    .properties()
                    .iter()
                    .map(|(k, v)| (k.clone(), ValueConverter::to_json(v)))
                    .collect();

                let ndjson_node = NdjsonNode {
                    id: Some(node.id()),
                    labels: node.labels().iter().cloned().collect(),
                    properties: props,
                };

                let json_line =
                    serde_json::to_string(&ndjson_node).map_err(|e| GraphError::Storage {
                        message: format!("Failed to serialize node: {}", e),
                    })?;

                writeln!(writer, "{}", json_line).map_err(|e| GraphError::Storage {
                    message: format!("Failed to write NDJSON line: {}", e),
                })?;

                count += 1;

                if count % options.chunk_size == 0 {
                    options.report_progress(count, Some(total));
                }
            }
        }

        writer.flush().map_err(|e| GraphError::Storage {
            message: format!("Failed to flush NDJSON: {}", e),
        })?;

        options.report_progress(total, Some(total));
        Ok(count)
    }

    /// 엣지를 NDJSON으로 내보내기
    pub async fn export_edges(
        engine: &GraphEngine,
        rel_type: Option<&str>,
        path: &str,
        options: StreamingOptions,
    ) -> Result<usize> {
        let edge_ids = engine.all_edge_ids().await;
        let total = edge_ids.len();

        if total == 0 {
            return Ok(0);
        }

        let file = File::create(path)?;
        let mut writer = BufWriter::with_capacity(options.buffer_size, file);

        let mut count = 0;
        for edge_id in &edge_ids {
            if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
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

                let ndjson_edge = NdjsonEdge {
                    id: Some(edge.id()),
                    from_node: edge.from_node(),
                    to_node: edge.to_node(),
                    relation_type: edge.relation_type().to_string(),
                    properties: props,
                };

                let json_line =
                    serde_json::to_string(&ndjson_edge).map_err(|e| GraphError::Storage {
                        message: format!("Failed to serialize edge: {}", e),
                    })?;

                writeln!(writer, "{}", json_line).map_err(|e| GraphError::Storage {
                    message: format!("Failed to write NDJSON line: {}", e),
                })?;

                count += 1;

                if count % options.chunk_size == 0 {
                    options.report_progress(count, None);
                }
            }
        }

        writer.flush().map_err(|e| GraphError::Storage {
            message: format!("Failed to flush NDJSON: {}", e),
        })?;

        options.report_progress(count, None);
        Ok(count)
    }
}

/// NDJSON Importer
///
/// 스트리밍 방식으로 NDJSON 파일을 가져옵니다.
/// 한 줄씩 읽어 처리하므로 메모리 사용량이 O(1)입니다.
pub struct NdjsonImporter;

impl NdjsonImporter {
    /// NDJSON 파일에서 노드 가져오기
    ///
    /// 한 줄씩 읽어 파싱하므로 대용량 파일도 메모리 효율적으로 처리합니다.
    pub async fn import_nodes(
        engine: &GraphEngine,
        path: &str,
        options: StreamingOptions,
    ) -> Result<usize> {
        let file = File::open(path)?;
        let reader = BufReader::with_capacity(options.buffer_size, file);

        let mut count = 0;
        let mut errors = 0;

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| GraphError::Storage {
                message: format!("Failed to read line {}: {}", line_num + 1, e),
            })?;

            // 빈 줄 건너뛰기
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // JSON 파싱
            let node_data: NdjsonNode = match serde_json::from_str(trimmed) {
                Ok(data) => data,
                Err(e) => {
                    errors += 1;
                    tracing::warn!("Failed to parse line {}: {}", line_num + 1, e);
                    continue;
                }
            };

            // 노드 생성
            let node_id = engine.create_node(node_data.labels).await?;

            // 프로퍼티 설정
            if !node_data.properties.is_empty() {
                engine
                    .update_node(node_id, |node| {
                        for (key, value) in node_data.properties.iter() {
                            node.set_property(key.clone(), ValueConverter::from_json(value));
                        }
                    })
                    .await?;
            }

            count += 1;

            if count % options.chunk_size == 0 {
                options.report_progress(count, None);
            }
        }

        if errors > 0 {
            tracing::warn!("NDJSON import completed with {} parse errors", errors);
        }

        options.report_progress(count, None);
        Ok(count)
    }

    /// NDJSON 파일에서 엣지 가져오기
    ///
    /// # Note
    /// from_node와 to_node ID가 이미 존재하는 노드를 가리켜야 합니다.
    pub async fn import_edges(
        engine: &GraphEngine,
        path: &str,
        options: StreamingOptions,
    ) -> Result<usize> {
        let file = File::open(path)?;
        let reader = BufReader::with_capacity(options.buffer_size, file);

        let mut count = 0;
        let mut errors = 0;

        for (line_num, line_result) in reader.lines().enumerate() {
            let line = line_result.map_err(|e| GraphError::Storage {
                message: format!("Failed to read line {}: {}", line_num + 1, e),
            })?;

            // 빈 줄 건너뛰기
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // JSON 파싱
            let edge_data: NdjsonEdge = match serde_json::from_str(trimmed) {
                Ok(data) => data,
                Err(e) => {
                    errors += 1;
                    tracing::warn!("Failed to parse line {}: {}", line_num + 1, e);
                    continue;
                }
            };

            // 엣지 생성
            let edge_id = engine
                .create_edge(
                    edge_data.relation_type,
                    edge_data.from_node,
                    edge_data.to_node,
                )
                .await?;

            // 프로퍼티 설정
            if !edge_data.properties.is_empty() {
                engine
                    .update_edge(edge_id, |edge| {
                        for (key, value) in edge_data.properties.iter() {
                            edge.set_property(key.clone(), ValueConverter::from_json(value));
                        }
                    })
                    .await?;
            }

            count += 1;

            if count % options.chunk_size == 0 {
                options.report_progress(count, None);
            }
        }

        if errors > 0 {
            tracing::warn!("NDJSON import completed with {} parse errors", errors);
        }

        options.report_progress(count, None);
        Ok(count)
    }

    /// NDJSON 파일에서 노드와 엣지를 함께 가져오기 (ID 매핑 지원)
    ///
    /// 노드 파일을 먼저 읽어 ID 매핑을 생성하고,
    /// 그 후 엣지 파일을 읽어 매핑된 ID로 엣지를 생성합니다.
    pub async fn import_graph(
        engine: &GraphEngine,
        nodes_path: &str,
        edges_path: &str,
        options: StreamingOptions,
    ) -> Result<(usize, usize)> {
        // 1. 노드 가져오기 (ID 매핑 저장)
        let mut id_map: HashMap<u64, u64> = HashMap::new();

        let nodes_file = File::open(nodes_path)?;
        let nodes_reader = BufReader::with_capacity(options.buffer_size, nodes_file);

        let mut node_count = 0;
        for (line_num, line_result) in nodes_reader.lines().enumerate() {
            let line = line_result.map_err(|e| GraphError::Storage {
                message: format!("Failed to read nodes line {}: {}", line_num + 1, e),
            })?;

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let node_data: NdjsonNode =
                serde_json::from_str(trimmed).map_err(|e| GraphError::Storage {
                    message: format!("Failed to parse nodes line {}: {}", line_num + 1, e),
                })?;

            let new_id = engine.create_node(node_data.labels).await?;

            // ID 매핑 저장
            if let Some(old_id) = node_data.id {
                id_map.insert(old_id, new_id);
            }

            // 프로퍼티 설정
            if !node_data.properties.is_empty() {
                engine
                    .update_node(new_id, |node| {
                        for (key, value) in node_data.properties.iter() {
                            node.set_property(key.clone(), ValueConverter::from_json(value));
                        }
                    })
                    .await?;
            }

            node_count += 1;

            if node_count % options.chunk_size == 0 {
                options.report_progress(node_count, None);
            }
        }

        // 2. 엣지 가져오기 (ID 매핑 적용)
        let edges_file = File::open(edges_path)?;
        let edges_reader = BufReader::with_capacity(options.buffer_size, edges_file);

        let mut edge_count = 0;
        for (line_num, line_result) in edges_reader.lines().enumerate() {
            let line = line_result.map_err(|e| GraphError::Storage {
                message: format!("Failed to read edges line {}: {}", line_num + 1, e),
            })?;

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let edge_data: NdjsonEdge =
                serde_json::from_str(trimmed).map_err(|e| GraphError::Storage {
                    message: format!("Failed to parse edges line {}: {}", line_num + 1, e),
                })?;

            // ID 매핑 적용
            let from_node = id_map.get(&edge_data.from_node).copied().ok_or({
                GraphError::NodeNotFound {
                    id: edge_data.from_node,
                }
            })?;
            let to_node = id_map.get(&edge_data.to_node).copied().ok_or({
                GraphError::NodeNotFound {
                    id: edge_data.to_node,
                }
            })?;

            let edge_id = engine
                .create_edge(edge_data.relation_type, from_node, to_node)
                .await?;

            // 프로퍼티 설정
            if !edge_data.properties.is_empty() {
                engine
                    .update_edge(edge_id, |edge| {
                        for (key, value) in edge_data.properties.iter() {
                            edge.set_property(key.clone(), ValueConverter::from_json(value));
                        }
                    })
                    .await?;
            }

            edge_count += 1;

            if edge_count % options.chunk_size == 0 {
                options.report_progress(node_count + edge_count, None);
            }
        }

        options.report_progress(node_count + edge_count, None);
        Ok((node_count, edge_count))
    }
}

/// 스트리밍 노드 이터레이터
///
/// 대용량 NDJSON 파일에서 노드를 하나씩 읽어오는 이터레이터
pub struct StreamingNodeIterator {
    reader: std::io::Lines<BufReader<File>>,
    line_num: usize,
}

impl StreamingNodeIterator {
    /// 새 이터레이터 생성
    pub fn new(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self {
            reader: reader.lines(),
            line_num: 0,
        })
    }
}

impl Iterator for StreamingNodeIterator {
    type Item = Result<NdjsonNode>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.line_num += 1;
            match self.reader.next() {
                Some(Ok(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str(trimmed) {
                        Ok(node) => return Some(Ok(node)),
                        Err(e) => {
                            return Some(Err(GraphError::Storage {
                                message: format!("Failed to parse line {}: {}", self.line_num, e),
                            }));
                        }
                    }
                }
                Some(Err(e)) => {
                    return Some(Err(GraphError::Storage {
                        message: format!("Failed to read line {}: {}", self.line_num, e),
                    }));
                }
                None => return None,
            }
        }
    }
}

/// 스트리밍 엣지 이터레이터
pub struct StreamingEdgeIterator {
    reader: std::io::Lines<BufReader<File>>,
    line_num: usize,
}

impl StreamingEdgeIterator {
    /// 새 이터레이터 생성
    pub fn new(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self {
            reader: reader.lines(),
            line_num: 0,
        })
    }
}

impl Iterator for StreamingEdgeIterator {
    type Item = Result<NdjsonEdge>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.line_num += 1;
            match self.reader.next() {
                Some(Ok(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str(trimmed) {
                        Ok(edge) => return Some(Ok(edge)),
                        Err(e) => {
                            return Some(Err(GraphError::Storage {
                                message: format!("Failed to parse line {}: {}", self.line_num, e),
                            }));
                        }
                    }
                }
                Some(Err(e)) => {
                    return Some(Err(GraphError::Storage {
                        message: format!("Failed to read line {}: {}", self.line_num, e),
                    }));
                }
                None => return None,
            }
        }
    }
}
