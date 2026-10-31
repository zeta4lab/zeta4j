// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Import/Export (Streaming) 메서드들.

use std::pin::Pin;

use tokio::sync::mpsc;
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use std::collections::HashSet;

use zeta4j_graph::core::{PropertyContainer, Value};
use zeta4j_graph::engine::GraphEngine;

use zeta4j_cli_grpc::admin::{ExportChunk, ExportRequest, ImportChunk, ImportResponse};

use super::AdminGrpcService;
use super::helpers::{json_to_value, parse_csv_value, value_to_json};
use crate::server::grpc::{SecurityGenerationGuard, access_status};

/// ExportStream 타입 별칭
pub type ExportStream = Pin<Box<dyn Stream<Item = Result<ExportChunk, Status>> + Send>>;

impl AdminGrpcService {
    /// 데이터 가져오기 (클라이언트 스트리밍)
    ///
    /// CSV/JSON 데이터를 파싱하여 노드 또는 엣지를 생성합니다.
    ///
    /// 각 입력 record를 canonical engine에 적용하고 실패 수를 응답에 반영합니다.
    pub async fn import_impl(
        &self,
        request: Request<Streaming<ImportChunk>>,
    ) -> Result<Response<ImportResponse>, Status> {
        let generation_guard = request
            .extensions()
            .get::<SecurityGenerationGuard>()
            .cloned()
            .ok_or_else(|| access_status(crate::server::access::PublicError::PolicyInvariant))?;
        generation_guard.validate().map_err(access_status)?;
        let mut stream = request.into_inner();
        let start = std::time::Instant::now();

        // 모든 청크 수집
        let mut all_data = Vec::new();
        let mut format = String::new();
        let mut data_type = String::new();
        let mut label: Option<String> = None;
        let mut delimiter = b',';

        while let Some(chunk) = stream.message().await? {
            generation_guard.validate().map_err(access_status)?;
            if format.is_empty() {
                format = chunk.format.clone();
                data_type = chunk.data_type.clone();
                label = chunk.label.clone();
                let bytes = chunk.delimiter.as_bytes();
                if bytes.len() != 1 || !bytes[0].is_ascii() {
                    return Err(Status::invalid_argument(
                        "delimiter must be exactly one ASCII character",
                    ));
                }
                delimiter = bytes[0];
            } else if chunk.format != format
                || chunk.data_type != data_type
                || chunk.label != label
                || chunk.delimiter.as_bytes() != [delimiter]
            {
                return Err(Status::invalid_argument(
                    "import metadata changed between stream chunks",
                ));
            }
            all_data.extend(chunk.data);
            if chunk.is_last {
                break;
            }
        }
        generation_guard.validate().map_err(access_status)?;

        // DB 매니저 확인
        let db_manager = self
            .state
            .db_manager()
            .ok_or_else(|| Status::unavailable("Database manager not available"))?;

        // 엔진 확인 (기본 데이터베이스)
        let engine = db_manager
            .get_engine("zeta4j")
            .await
            .ok_or_else(|| Status::not_found("Default database 'zeta4j' not found"))?;

        let content = std::str::from_utf8(&all_data).map_err(|error| {
            Status::invalid_argument(format!("import input is not UTF-8: {error}"))
        })?;
        if content.trim().is_empty() {
            return Err(Status::invalid_argument("import input is empty"));
        }
        let default_label = label.as_deref().unwrap_or("ImportedNode");

        if data_type != "nodes" {
            return Err(Status::invalid_argument(
                "admin import currently accepts data_type 'nodes' only",
            ));
        }

        let records = parse_import_records(&format, content, delimiter, default_label)?;
        let records_imported = u64::try_from(records.len())
            .map_err(|_| Status::resource_exhausted("import record count exceeds u64"))?;
        import_nodes_atomically(&engine, records).await?;

        tracing::info!(
            "Import completed: {} imported in {}ms",
            records_imported,
            start.elapsed().as_millis()
        );

        Ok(Response::new(ImportResponse {
            success: true,
            message: format!("Import completed: {records_imported} imported"),
            records_imported,
            records_skipped: 0,
            errors: 0,
            duration_ms: start.elapsed().as_millis() as u64,
        }))
    }

    /// 데이터 내보내기 (서버 스트리밍)
    ///
    /// 노드 또는 엣지를 CSV/JSON 형식으로 내보냅니다.
    ///
    /// 각 record를 canonical engine에서 읽어 스트리밍 응답으로 반환합니다.
    pub async fn export_impl(
        &self,
        request: Request<ExportRequest>,
    ) -> Result<Response<ExportStream>, Status> {
        let req = request.into_inner();
        if !matches!(req.format.as_str(), "csv" | "json" | "ndjson") {
            return Err(Status::invalid_argument(format!(
                "Unsupported format '{}'. Supported: csv, json, ndjson",
                req.format
            )));
        }
        if !matches!(req.data_type.as_str(), "nodes" | "edges") {
            return Err(Status::invalid_argument(format!(
                "Unsupported data type '{}'. Supported: nodes, edges",
                req.data_type
            )));
        }
        let delimiter_text = req.delimiter.as_deref().unwrap_or(",");
        let delimiter_bytes = delimiter_text.as_bytes();
        if delimiter_bytes.len() != 1 || !delimiter_bytes[0].is_ascii() {
            return Err(Status::invalid_argument(
                "delimiter must be exactly one ASCII character",
            ));
        }
        let delimiter = delimiter_bytes[0];

        let db_manager = self
            .state
            .db_manager()
            .ok_or_else(|| Status::unavailable("Database manager not available"))?;
        let engine = db_manager
            .get_engine("zeta4j")
            .await
            .ok_or_else(|| Status::not_found("Default database 'zeta4j' not found"))?;
        let (tx, rx) = mpsc::channel(100);
        let format = req.format.clone();
        let data_type = req.data_type.clone();
        let label = req.label.clone();

        tokio::spawn(async move {
            // 시작 메시지
            let _ = tx
                .send(Ok(ExportChunk {
                    data: Vec::new(),
                    records_exported: 0,
                    is_last: false,
                    message: Some(format!(
                        "Starting export of {} ({} format)",
                        data_type, format
                    )),
                }))
                .await;

            let mut records_exported = 0u64;
            let mut output = String::new();
            let mut json_records = Vec::new();

            match data_type.as_str() {
                "nodes" | "" => {
                    // 스냅샷 시점의 노드 ID 목록 획득
                    let node_ids = engine.all_node_ids().await;

                    if format == "csv" {
                        let mut records = vec![vec![
                            "id".to_string(),
                            "labels".to_string(),
                            "properties".to_string(),
                        ]];

                        for node_id in node_ids {
                            if let Ok(Some(node)) = engine.get_node(node_id).await {
                                if label
                                    .as_ref()
                                    .is_some_and(|label| !node.labels().contains(label))
                                {
                                    continue;
                                }
                                let labels: Vec<String> = node.labels().iter().cloned().collect();
                                let labels = labels.join(":");
                                let props = node.properties().as_map();
                                let props_json = match serde_json::to_string(
                                    &props
                                        .iter()
                                        .map(|(k, v)| (k.clone(), value_to_json(v)))
                                        .collect::<serde_json::Map<String, serde_json::Value>>(),
                                ) {
                                    Ok(value) => value,
                                    Err(error) => {
                                        let _ = tx
                                            .send(Err(Status::internal(format!(
                                                "Failed to serialize node {node_id}: {error}"
                                            ))))
                                            .await;
                                        return;
                                    }
                                };

                                records.push(vec![node_id.to_string(), labels, props_json]);
                                records_exported += 1;
                            }
                        }
                        match serialize_csv(records, delimiter) {
                            Ok(csv) => output = csv,
                            Err(error) => {
                                let _ = tx.send(Err(error)).await;
                                return;
                            }
                        }
                    } else if format == "json" || format == "ndjson" {
                        for node_id in node_ids {
                            if let Ok(Some(node)) = engine.get_node(node_id).await {
                                if label
                                    .as_ref()
                                    .is_some_and(|label| !node.labels().contains(label))
                                {
                                    continue;
                                }
                                let mut obj = serde_json::Map::new();
                                obj.insert(
                                    "id".to_string(),
                                    serde_json::Value::Number(node_id.into()),
                                );
                                obj.insert(
                                    "labels".to_string(),
                                    serde_json::Value::Array(
                                        node.labels()
                                            .iter()
                                            .map(|l| serde_json::Value::String(l.clone()))
                                            .collect(),
                                    ),
                                );

                                let props_map: serde_json::Map<String, serde_json::Value> = node
                                    .properties()
                                    .as_map()
                                    .iter()
                                    .map(|(k, v)| (k.clone(), value_to_json(v)))
                                    .collect();
                                obj.insert(
                                    "properties".to_string(),
                                    serde_json::Value::Object(props_map),
                                );

                                if format == "json" {
                                    json_records.push(serde_json::Value::Object(obj));
                                } else {
                                    let line = match serde_json::to_string(&obj) {
                                        Ok(line) => line,
                                        Err(error) => {
                                            let _ = tx
                                                .send(Err(Status::internal(format!(
                                                    "Failed to serialize node {node_id}: {error}"
                                                ))))
                                                .await;
                                            return;
                                        }
                                    };
                                    output.push_str(&line);
                                    output.push('\n');
                                }
                                records_exported += 1;
                            }
                        }
                    }
                }
                "edges" => {
                    let edge_ids = engine.all_edge_ids().await;

                    if format == "csv" {
                        let mut records = vec![vec![
                            "id".to_string(),
                            "type".to_string(),
                            "from_node".to_string(),
                            "to_node".to_string(),
                            "properties".to_string(),
                        ]];

                        for edge_id in edge_ids {
                            if let Ok(Some(edge)) = engine.get_edge(edge_id).await {
                                if label
                                    .as_ref()
                                    .is_some_and(|label| edge.relation_type() != label)
                                {
                                    continue;
                                }
                                let props = edge.properties().as_map();
                                let props_json = match serde_json::to_string(
                                    &props
                                        .iter()
                                        .map(|(k, v)| (k.clone(), value_to_json(v)))
                                        .collect::<serde_json::Map<String, serde_json::Value>>(),
                                ) {
                                    Ok(value) => value,
                                    Err(error) => {
                                        let _ = tx
                                            .send(Err(Status::internal(format!(
                                                "Failed to serialize edge {edge_id}: {error}"
                                            ))))
                                            .await;
                                        return;
                                    }
                                };

                                records.push(vec![
                                    edge_id.to_string(),
                                    edge.relation_type().to_string(),
                                    edge.from_node().to_string(),
                                    edge.to_node().to_string(),
                                    props_json,
                                ]);
                                records_exported += 1;
                            }
                        }
                        match serialize_csv(records, delimiter) {
                            Ok(csv) => output = csv,
                            Err(error) => {
                                let _ = tx.send(Err(error)).await;
                                return;
                            }
                        }
                    } else if format == "json" || format == "ndjson" {
                        for edge_id in edge_ids {
                            if let Ok(Some(edge)) = engine.get_edge(edge_id).await {
                                if label
                                    .as_ref()
                                    .is_some_and(|label| edge.relation_type() != label)
                                {
                                    continue;
                                }
                                let mut obj = serde_json::Map::new();
                                obj.insert(
                                    "id".to_string(),
                                    serde_json::Value::Number(edge_id.into()),
                                );
                                obj.insert(
                                    "type".to_string(),
                                    serde_json::Value::String(edge.relation_type().to_string()),
                                );
                                obj.insert(
                                    "from_node".to_string(),
                                    serde_json::Value::Number(edge.from_node().into()),
                                );
                                obj.insert(
                                    "to_node".to_string(),
                                    serde_json::Value::Number(edge.to_node().into()),
                                );

                                let props_map: serde_json::Map<String, serde_json::Value> = edge
                                    .properties()
                                    .as_map()
                                    .iter()
                                    .map(|(k, v)| (k.clone(), value_to_json(v)))
                                    .collect();
                                obj.insert(
                                    "properties".to_string(),
                                    serde_json::Value::Object(props_map),
                                );

                                if format == "json" {
                                    json_records.push(serde_json::Value::Object(obj));
                                } else {
                                    let line = match serde_json::to_string(&obj) {
                                        Ok(line) => line,
                                        Err(error) => {
                                            let _ = tx
                                                .send(Err(Status::internal(format!(
                                                    "Failed to serialize edge {edge_id}: {error}"
                                                ))))
                                                .await;
                                            return;
                                        }
                                    };
                                    output.push_str(&line);
                                    output.push('\n');
                                }
                                records_exported += 1;
                            }
                        }
                    }
                }
                _ => {}
            }

            if format == "json" {
                match serde_json::to_string_pretty(&json_records) {
                    Ok(json) => {
                        output = json;
                        output.push('\n');
                    }
                    Err(error) => {
                        let _ = tx
                            .send(Err(Status::internal(format!(
                                "Failed to serialize export: {error}"
                            ))))
                            .await;
                        return;
                    }
                }
            }

            tracing::info!("Export completed: {} records", records_exported);

            // 데이터 전송
            let _ = tx
                .send(Ok(ExportChunk {
                    data: output.into_bytes(),
                    records_exported,
                    is_last: true,
                    message: Some(format!("Export completed: {} records", records_exported)),
                }))
                .await;
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

struct ImportedNode {
    labels: Vec<String>,
    properties: Vec<(String, Value)>,
}

fn parse_import_records(
    format: &str,
    content: &str,
    delimiter: u8,
    default_label: &str,
) -> Result<Vec<ImportedNode>, Status> {
    match format {
        "csv" => parse_csv_nodes(content.as_bytes(), delimiter, default_label),
        "json" => parse_json_nodes(content, default_label),
        "ndjson" => parse_ndjson_nodes(content, default_label),
        _ => Err(Status::invalid_argument(format!(
            "Unsupported format: {format}. Supported: csv, json, ndjson"
        ))),
    }
}

fn parse_csv_nodes(
    content: &[u8],
    delimiter: u8,
    default_label: &str,
) -> Result<Vec<ImportedNode>, Status> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .trim(csv::Trim::All)
        .from_reader(content);
    let headers = reader
        .headers()
        .map_err(|error| Status::invalid_argument(format!("Invalid CSV header: {error}")))?
        .clone();
    if headers.is_empty() || headers.iter().any(str::is_empty) {
        return Err(Status::invalid_argument(
            "CSV header names must not be empty",
        ));
    }
    let mut unique_headers = HashSet::new();
    if headers
        .iter()
        .any(|header| !unique_headers.insert(header.to_owned()))
    {
        return Err(Status::invalid_argument("CSV header names must be unique"));
    }

    let labels_index = headers.iter().position(|header| header == "labels");
    let properties_index = headers.iter().position(|header| header == "properties");
    let mut nodes = Vec::new();
    for (index, result) in reader.records().enumerate() {
        let record = result.map_err(|error| {
            Status::invalid_argument(format!("Invalid CSV record {}: {error}", index + 2))
        })?;
        let labels = labels_index
            .and_then(|position| record.get(position))
            .map(parse_labels)
            .filter(|labels| !labels.is_empty())
            .unwrap_or_else(|| vec![default_label.to_owned()]);
        let properties = if let Some(position) = properties_index {
            let text = record.get(position).unwrap_or_default();
            let value = serde_json::from_str::<serde_json::Value>(text).map_err(|error| {
                Status::invalid_argument(format!(
                    "Invalid JSON properties in CSV record {}: {error}",
                    index + 2
                ))
            })?;
            json_properties(&value, &format!("CSV record {} properties", index + 2))?
        } else {
            headers
                .iter()
                .enumerate()
                .filter(|(_, header)| *header != "id" && *header != "labels")
                .filter_map(|(position, header)| {
                    record.get(position).and_then(|value| {
                        (!value.is_empty()).then(|| (header.to_owned(), parse_csv_value(value)))
                    })
                })
                .collect()
        };
        nodes.push(ImportedNode { labels, properties });
    }
    Ok(nodes)
}

fn parse_json_nodes(content: &str, default_label: &str) -> Result<Vec<ImportedNode>, Status> {
    let value = serde_json::from_str::<serde_json::Value>(content)
        .map_err(|error| Status::invalid_argument(format!("Invalid JSON: {error}")))?;
    let values = match value {
        serde_json::Value::Array(values) => values,
        value @ serde_json::Value::Object(_) => vec![value],
        _ => {
            return Err(Status::invalid_argument(
                "JSON import expects an object or an array of objects",
            ));
        }
    };
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            parse_json_node(value, default_label, &format!("JSON record {}", index + 1))
        })
        .collect()
}

fn parse_ndjson_nodes(content: &str, default_label: &str) -> Result<Vec<ImportedNode>, Status> {
    content
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            let value = serde_json::from_str::<serde_json::Value>(line).map_err(|error| {
                Status::invalid_argument(format!("Invalid NDJSON line {}: {error}", index + 1))
            })?;
            parse_json_node(&value, default_label, &format!("NDJSON line {}", index + 1))
        })
        .collect()
}

fn parse_json_node(
    value: &serde_json::Value,
    default_label: &str,
    context: &str,
) -> Result<ImportedNode, Status> {
    let object = value
        .as_object()
        .ok_or_else(|| Status::invalid_argument(format!("{context} must be an object")))?;
    if let Some(properties) = object.get("properties") {
        let labels = match object.get("labels") {
            Some(serde_json::Value::Array(labels)) => labels
                .iter()
                .map(|label| {
                    label.as_str().map(str::to_owned).ok_or_else(|| {
                        Status::invalid_argument(format!(
                            "{context} labels must contain only strings"
                        ))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            Some(_) => {
                return Err(Status::invalid_argument(format!(
                    "{context} labels must be an array"
                )));
            }
            None => vec![default_label.to_owned()],
        };
        return Ok(ImportedNode {
            labels: if labels.is_empty() {
                vec![default_label.to_owned()]
            } else {
                labels
            },
            properties: json_properties(properties, &format!("{context} properties"))?,
        });
    }

    Ok(ImportedNode {
        labels: vec![default_label.to_owned()],
        properties: object
            .iter()
            .map(|(key, value)| (key.clone(), json_to_value(value)))
            .collect(),
    })
}

fn json_properties(
    value: &serde_json::Value,
    context: &str,
) -> Result<Vec<(String, Value)>, Status> {
    value
        .as_object()
        .ok_or_else(|| Status::invalid_argument(format!("{context} must be an object")))
        .map(|properties| {
            properties
                .iter()
                .map(|(key, value)| (key.clone(), json_to_value(value)))
                .collect()
        })
}

fn parse_labels(value: &str) -> Vec<String> {
    value
        .split([':', ';'])
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .collect()
}

async fn import_nodes_atomically(
    engine: &GraphEngine,
    records: Vec<ImportedNode>,
) -> Result<u64, Status> {
    let mut created = Vec::with_capacity(records.len());
    for record in records {
        match create_imported_node(engine, record.labels, record.properties).await {
            Ok(node_id) => created.push(node_id),
            Err(error) => {
                let mut rollback_errors = Vec::new();
                for node_id in created {
                    if let Err(rollback_error) = engine.delete_node(node_id).await {
                        rollback_errors.push(format!("node {node_id}: {rollback_error}"));
                    }
                }
                let suffix = if rollback_errors.is_empty() {
                    String::new()
                } else {
                    format!("; rollback failed for {}", rollback_errors.join(", "))
                };
                return Err(Status::internal(format!("Import failed: {error}{suffix}")));
            }
        }
    }
    u64::try_from(created.len())
        .map_err(|_| Status::resource_exhausted("import record count exceeds u64"))
}

fn serialize_csv(records: Vec<Vec<String>>, delimiter: u8) -> Result<String, Status> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(Vec::new());
    for record in records {
        writer
            .write_record(record)
            .map_err(|error| Status::internal(format!("Failed to serialize CSV: {error}")))?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|error| Status::internal(format!("Failed to finalize CSV: {}", error.error())))?;
    String::from_utf8(bytes)
        .map_err(|error| Status::internal(format!("CSV output was not UTF-8: {error}")))
}

async fn create_imported_node(
    engine: &GraphEngine,
    labels: Vec<String>,
    properties: Vec<(String, Value)>,
) -> Result<u64, String> {
    let node_id = engine
        .create_node(labels)
        .await
        .map_err(|error| error.to_string())?;
    if let Err(error) = engine
        .update_node(node_id, move |node| {
            for (key, value) in properties {
                node.set_property(key, value);
            }
        })
        .await
    {
        if let Err(rollback_error) = engine.delete_node(node_id).await {
            return Err(format!(
                "failed to apply imported properties: {error}; failed to roll back node {node_id}: {rollback_error}"
            ));
        }
        return Err(format!("failed to apply imported properties: {error}"));
    }
    Ok(node_id)
}
