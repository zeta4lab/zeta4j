//! CSV Import/Export
//!
//! 그래프 데이터의 CSV 형식 내보내기/가져오기

use indexmap::IndexMap;

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter};

use super::format::ValueConverter;
use zeta4j_graph::core::PropertyContainer;
use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::error::{GraphError, Result};

/// CSV 옵션
#[derive(Debug, Clone)]
pub struct CsvOptions {
    /// 구분자 (기본: ',')
    pub delimiter: u8,
    /// 헤더 포함 여부 (기본: true)
    pub has_header: bool,
    /// 인용 문자 (기본: '"')
    pub quote: u8,
    /// 라벨 구분자 (기본: ';')
    pub label_delimiter: char,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: b',',
            has_header: true,
            quote: b'"',
            label_delimiter: ';',
        }
    }
}

/// CSV Exporter
pub struct CsvExporter;

impl CsvExporter {
    /// 노드를 CSV로 내보내기
    ///
    /// CSV 포맷:
    /// ```csv
    /// id,labels,prop1,prop2,...
    /// 1,"Person;Employee",Alice,30
    /// ```
    pub async fn export_nodes(
        engine: &GraphEngine,
        label: Option<&str>,
        path: &str,
        options: CsvOptions,
    ) -> Result<usize> {
        let node_ids = match label {
            Some(l) => engine.find_nodes_by_label(l).await?,
            None => engine.all_node_ids().await,
        };

        // 모든 프로퍼티 키 수집
        let mut all_props: HashSet<String> = HashSet::new();
        let mut nodes_data: Vec<(u64, Vec<String>, IndexMap<String, Value>)> = Vec::new();

        for node_id in node_ids {
            if let Ok(Some(node)) = engine.get_node(node_id).await {
                let labels: Vec<String> = node.labels().iter().cloned().collect();
                let props: IndexMap<String, Value> = node
                    .properties()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

                for key in props.keys() {
                    all_props.insert(key.clone());
                }

                nodes_data.push((node.id(), labels, props));
            }
        }

        // 정렬된 프로퍼티 키 목록
        let mut prop_keys: Vec<String> = all_props.into_iter().collect();
        prop_keys.sort();

        // CSV 파일 작성
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        let mut csv_writer = csv::WriterBuilder::new()
            .delimiter(options.delimiter)
            .quote(options.quote)
            .from_writer(writer);

        // 헤더 작성
        if options.has_header {
            let mut header = vec!["id".to_string(), "labels".to_string()];
            header.extend(prop_keys.iter().cloned());
            csv_writer
                .write_record(&header)
                .map_err(|e| GraphError::Storage {
                    message: format!("Failed to write CSV header: {}", e),
                })?;
        }

        // 데이터 작성
        for (id, labels, props) in &nodes_data {
            let labels_str = labels.join(&options.label_delimiter.to_string());
            let mut record = vec![id.to_string(), labels_str];

            for key in &prop_keys {
                let value = props
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
        }

        csv_writer.flush().map_err(|e| GraphError::Storage {
            message: format!("Failed to flush CSV: {}", e),
        })?;

        Ok(nodes_data.len())
    }

    /// 엣지를 CSV로 내보내기
    ///
    /// CSV 포맷:
    /// ```csv
    /// id,from_node,to_node,relation_type,prop1,prop2,...
    /// 1,1,2,KNOWS,2020
    /// ```
    #[expect(clippy::type_complexity)]
    pub async fn export_edges(
        engine: &GraphEngine,
        rel_type: Option<&str>,
        path: &str,
        options: CsvOptions,
    ) -> Result<usize> {
        let edge_ids = engine.all_edge_ids().await;

        // 모든 프로퍼티 키 수집
        let mut all_props: HashSet<String> = HashSet::new();
        let mut edges_data: Vec<(u64, u64, u64, String, IndexMap<String, Value>)> = Vec::new();

        for edge_id in edge_ids {
            if let Ok(Some(edge)) = engine.get_edge(edge_id).await {
                // 타입 필터링
                if let Some(rt) = rel_type
                    && edge.relation_type() != rt
                {
                    continue;
                }

                let props: IndexMap<String, Value> = edge
                    .properties()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();

                for key in props.keys() {
                    all_props.insert(key.clone());
                }

                edges_data.push((
                    edge.id(),
                    edge.from_node(),
                    edge.to_node(),
                    edge.relation_type().to_string(),
                    props,
                ));
            }
        }

        // 정렬된 프로퍼티 키 목록
        let mut prop_keys: Vec<String> = all_props.into_iter().collect();
        prop_keys.sort();

        // CSV 파일 작성
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        let mut csv_writer = csv::WriterBuilder::new()
            .delimiter(options.delimiter)
            .quote(options.quote)
            .from_writer(writer);

        // 헤더 작성
        if options.has_header {
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

        // 데이터 작성
        for (id, from_node, to_node, edge_type, props) in &edges_data {
            let mut record = vec![
                id.to_string(),
                from_node.to_string(),
                to_node.to_string(),
                edge_type.clone(),
            ];

            for key in &prop_keys {
                let value = props
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
        }

        csv_writer.flush().map_err(|e| GraphError::Storage {
            message: format!("Failed to flush CSV: {}", e),
        })?;

        Ok(edges_data.len())
    }
}

/// CSV Importer
pub struct CsvImporter;

impl CsvImporter {
    /// CSV 파일에서 노드 가져오기
    ///
    /// 필수 열: labels
    /// 선택 열: id (무시됨, 자동 생성), 기타 프로퍼티
    pub async fn import_nodes(
        engine: &GraphEngine,
        path: &str,
        options: CsvOptions,
    ) -> Result<usize> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut csv_reader = csv::ReaderBuilder::new()
            .delimiter(options.delimiter)
            .has_headers(options.has_header)
            .quote(options.quote)
            .from_reader(reader);

        let headers: Vec<String> = if options.has_header {
            csv_reader
                .headers()
                .map_err(|e| GraphError::Storage {
                    message: format!("Failed to read CSV headers: {}", e),
                })?
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        };

        // labels 열 찾기
        let labels_idx =
            headers
                .iter()
                .position(|h| h == "labels")
                .ok_or_else(|| GraphError::Validation {
                    message: "CSV must have 'labels' column".to_string(),
                })?;

        let mut count = 0;
        for result in csv_reader.records() {
            let record = result.map_err(|e| GraphError::Storage {
                message: format!("Failed to read CSV record: {}", e),
            })?;

            // 라벨 파싱
            let labels_str = record.get(labels_idx).unwrap_or("");
            let labels: Vec<String> = if labels_str.is_empty() {
                Vec::new()
            } else {
                labels_str
                    .split(options.label_delimiter)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            };

            // 노드 생성
            let node_id = engine.create_node(labels).await?;

            // 프로퍼티 설정
            engine
                .update_node(node_id, |node| {
                    for (i, header) in headers.iter().enumerate() {
                        if header == "id" || header == "labels" {
                            continue;
                        }
                        if let Some(value_str) = record.get(i)
                            && !value_str.is_empty()
                        {
                            let value = ValueConverter::infer_from_string(value_str);
                            node.set_property(header.clone(), value);
                        }
                    }
                })
                .await?;

            count += 1;
        }

        Ok(count)
    }

    /// CSV 파일에서 엣지 가져오기
    ///
    /// 필수 열: from_node, to_node, relation_type
    /// 선택 열: id (무시됨), 기타 프로퍼티
    pub async fn import_edges(
        engine: &GraphEngine,
        path: &str,
        options: CsvOptions,
    ) -> Result<usize> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut csv_reader = csv::ReaderBuilder::new()
            .delimiter(options.delimiter)
            .has_headers(options.has_header)
            .quote(options.quote)
            .from_reader(reader);

        let headers: Vec<String> = if options.has_header {
            csv_reader
                .headers()
                .map_err(|e| GraphError::Storage {
                    message: format!("Failed to read CSV headers: {}", e),
                })?
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        };

        // 필수 열 찾기
        let from_node_idx = headers
            .iter()
            .position(|h| h == "from_node")
            .ok_or_else(|| GraphError::Validation {
                message: "CSV must have 'from_node' column".to_string(),
            })?;
        let to_node_idx =
            headers
                .iter()
                .position(|h| h == "to_node")
                .ok_or_else(|| GraphError::Validation {
                    message: "CSV must have 'to_node' column".to_string(),
                })?;
        let type_idx = headers
            .iter()
            .position(|h| h == "relation_type")
            .ok_or_else(|| GraphError::Validation {
                message: "CSV must have 'relation_type' column".to_string(),
            })?;

        let mut count = 0;
        for result in csv_reader.records() {
            let record = result.map_err(|e| GraphError::Storage {
                message: format!("Failed to read CSV record: {}", e),
            })?;

            let from_node: u64 = record
                .get(from_node_idx)
                .ok_or_else(|| GraphError::Validation {
                    message: "Missing from_node".to_string(),
                })?
                .parse()
                .map_err(|_| GraphError::Validation {
                    message: "Invalid from_node ID".to_string(),
                })?;

            let to_node: u64 = record
                .get(to_node_idx)
                .ok_or_else(|| GraphError::Validation {
                    message: "Missing to_node".to_string(),
                })?
                .parse()
                .map_err(|_| GraphError::Validation {
                    message: "Invalid to_node ID".to_string(),
                })?;

            let relation_type = record
                .get(type_idx)
                .ok_or_else(|| GraphError::Validation {
                    message: "Missing relation_type".to_string(),
                })?
                .to_string();

            // 프로퍼티 수집
            let mut props: IndexMap<String, Value> = IndexMap::new();
            for (i, header) in headers.iter().enumerate() {
                if header == "id"
                    || header == "from_node"
                    || header == "to_node"
                    || header == "relation_type"
                {
                    continue;
                }
                if let Some(value_str) = record.get(i)
                    && !value_str.is_empty()
                {
                    let value = ValueConverter::infer_from_string(value_str);
                    props.insert(header.clone(), value);
                }
            }

            // 엣지 생성
            let edge_id = engine
                .create_edge(relation_type, from_node, to_node)
                .await?;

            // 프로퍼티 설정
            if !props.is_empty() {
                engine
                    .update_edge(edge_id, |edge| {
                        for (key, value) in props.iter() {
                            edge.set_property(key.clone(), value.clone());
                        }
                    })
                    .await?;
            }
            count += 1;
        }

        Ok(count)
    }
}
