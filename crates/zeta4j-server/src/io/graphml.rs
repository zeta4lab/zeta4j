//! GraphML Import/Export
//!
//! v0.51.0: GraphML 표준 그래프 교환 포맷 지원
//!
//! GraphML은 XML 기반 그래프 교환 포맷으로 Neo4j, Gephi, yEd 등과 호환됩니다.
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::io::{GraphMLExporter, GraphMLImporter};
//!
//! // GraphML로 내보내기
//! let stats = GraphMLExporter::export_graph(&engine, "graph.graphml").await?;
//!
//! // GraphML에서 가져오기
//! let stats = GraphMLImporter::import_graph(&engine, "graph.graphml").await?;
//! ```

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

use zeta4j_graph::core::PropertyContainer;
use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::error::Result;

/// GraphML 속성 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphMLType {
    String,
    Int,
    Long,
    Float,
    Double,
    Boolean,
}

impl GraphMLType {
    /// 문자열에서 타입 파싱
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "int" | "integer" => Self::Int,
            "long" => Self::Long,
            "float" => Self::Float,
            "double" => Self::Double,
            "boolean" | "bool" => Self::Boolean,
            _ => Self::String,
        }
    }

    /// 타입을 문자열로
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int => "int",
            Self::Long => "long",
            Self::Float => "float",
            Self::Double => "double",
            Self::Boolean => "boolean",
        }
    }

    /// Value에서 타입 추론
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::Integer(_) => Self::Long,
            Value::Float(_) => Self::Double,
            Value::Boolean(_) => Self::Boolean,
            _ => Self::String,
        }
    }
}

/// 키 대상 (노드 또는 엣지)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyTarget {
    Node,
    Edge,
    All,
}

impl KeyTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Edge => "edge",
            Self::All => "all",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "node" => Self::Node,
            "edge" => Self::Edge,
            _ => Self::All,
        }
    }
}

/// GraphML 키 정의
#[derive(Debug, Clone)]
pub struct GraphMLKey {
    pub id: String,
    pub for_type: KeyTarget,
    pub attr_name: String,
    pub attr_type: GraphMLType,
}

/// Export 통계
#[derive(Debug, Clone, Default)]
pub struct ExportStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub key_count: usize,
}

/// Import 통계
#[derive(Debug, Clone, Default)]
pub struct ImportStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub key_count: usize,
}

/// XML 특수문자 이스케이프
fn escape_xml(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\'' => result.push_str("&apos;"),
            _ => result.push(c),
        }
    }
    result
}

/// XML 특수문자 언이스케이프
fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Value를 문자열로 변환
fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(value_to_string).collect();
            format!("[{}]", items.join(","))
        }
        Value::Map(map) => {
            let items: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}:{}", k, value_to_string(v)))
                .collect();
            format!("{{{}}}", items.join(","))
        }
        Value::Node(n) => format!("Node({})", n),
        Value::Edge(e) => format!("Edge({})", e),
        Value::Path(p) => format!("Path(len={})", p.edges.len()),
        Value::DateTime(dt) => dt.to_rfc3339(),
        Value::Date(d) => d.to_string(),
        Value::Time(t) => t.to_string(),
        Value::LocalDateTime(ldt) => ldt.to_string(),
        Value::Duration(dur) => format!("P{}M{}DT{}S", dur.months, dur.days, dur.seconds),
        Value::Vector(v) => format!("Vector(dims={})", v.dimensions()),
    }
}

/// 문자열을 Value로 변환
fn string_to_value(s: &str, attr_type: GraphMLType) -> Value {
    match attr_type {
        GraphMLType::Boolean => Value::Boolean(s.eq_ignore_ascii_case("true") || s == "1"),
        GraphMLType::Int | GraphMLType::Long => s
            .parse::<i64>()
            .map(Value::Integer)
            .unwrap_or(Value::String(s.to_string())),
        GraphMLType::Float | GraphMLType::Double => s
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or(Value::String(s.to_string())),
        GraphMLType::String => Value::String(s.to_string()),
    }
}

/// GraphML Exporter
pub struct GraphMLExporter;

impl GraphMLExporter {
    /// 전체 그래프를 GraphML로 내보내기
    pub async fn export_graph(engine: &GraphEngine, path: &str) -> Result<ExportStats> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // 모든 노드와 엣지 수집
        let node_ids = engine.all_node_ids().await;
        let edge_ids = engine.all_edge_ids().await;

        // 속성 키 수집
        let mut node_keys: HashMap<String, GraphMLType> = HashMap::new();
        let mut edge_keys: HashMap<String, GraphMLType> = HashMap::new();

        // 노드 속성 키 수집
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                for (key, value) in node.properties().iter() {
                    node_keys
                        .entry(key.clone())
                        .or_insert_with(|| GraphMLType::from_value(value));
                }
            }
        }

        // 엣지 속성 키 수집
        for edge_id in &edge_ids {
            if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
                for (key, value) in edge.properties().iter() {
                    edge_keys
                        .entry(key.clone())
                        .or_insert_with(|| GraphMLType::from_value(value));
                }
            }
        }

        // XML 헤더
        writeln!(writer, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(
            writer,
            r#"<graphml xmlns="http://graphml.graphdrawing.org/xmlns""#
        )?;
        writeln!(
            writer,
            r#"         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance""#
        )?;
        writeln!(
            writer,
            r#"         xsi:schemaLocation="http://graphml.graphdrawing.org/xmlns http://graphml.graphdrawing.org/xmlns/1.0/graphml.xsd">"#
        )?;

        // 노드 라벨 키 (특수)
        writeln!(
            writer,
            r#"  <key id="label" for="node" attr.name="label" attr.type="string"/>"#
        )?;

        // 노드 속성 키
        let mut key_id = 0;
        let mut node_key_map: HashMap<String, String> = HashMap::new();
        for (attr_name, attr_type) in &node_keys {
            let id = format!("n{}", key_id);
            writeln!(
                writer,
                r#"  <key id="{}" for="node" attr.name="{}" attr.type="{}"/>"#,
                id,
                escape_xml(attr_name),
                attr_type.as_str()
            )?;
            node_key_map.insert(attr_name.clone(), id);
            key_id += 1;
        }

        // 엣지 관계 타입 키 (특수)
        writeln!(
            writer,
            r#"  <key id="reltype" for="edge" attr.name="reltype" attr.type="string"/>"#
        )?;

        // 엣지 속성 키
        let mut edge_key_map: HashMap<String, String> = HashMap::new();
        for (attr_name, attr_type) in &edge_keys {
            let id = format!("e{}", key_id);
            writeln!(
                writer,
                r#"  <key id="{}" for="edge" attr.name="{}" attr.type="{}"/>"#,
                id,
                escape_xml(attr_name),
                attr_type.as_str()
            )?;
            edge_key_map.insert(attr_name.clone(), id);
            key_id += 1;
        }

        // 그래프 시작
        writeln!(writer, r#"  <graph id="G" edgedefault="directed">"#)?;

        // 노드 출력
        let mut node_count = 0;
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                writeln!(writer, r#"    <node id="n{}">"#, node_id)?;

                // 라벨
                let labels: Vec<&str> = node.labels().iter().map(|s| s.as_str()).collect();
                if !labels.is_empty() {
                    writeln!(
                        writer,
                        r#"      <data key="label">{}</data>"#,
                        escape_xml(&labels.join(";"))
                    )?;
                }

                // 속성
                for (key, value) in node.properties().iter() {
                    if let Some(key_id) = node_key_map.get(key) {
                        writeln!(
                            writer,
                            r#"      <data key="{}">{}</data>"#,
                            key_id,
                            escape_xml(&value_to_string(value))
                        )?;
                    }
                }

                writeln!(writer, r#"    </node>"#)?;
                node_count += 1;
            }
        }

        // 엣지 출력
        let mut edge_count = 0;
        for edge_id in &edge_ids {
            if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
                writeln!(
                    writer,
                    r#"    <edge id="e{}" source="n{}" target="n{}">"#,
                    edge_id,
                    edge.from_node(),
                    edge.to_node()
                )?;

                // 관계 타입
                writeln!(
                    writer,
                    r#"      <data key="reltype">{}</data>"#,
                    escape_xml(edge.relation_type())
                )?;

                // 속성
                for (key, value) in edge.properties().iter() {
                    if let Some(key_id) = edge_key_map.get(key) {
                        writeln!(
                            writer,
                            r#"      <data key="{}">{}</data>"#,
                            key_id,
                            escape_xml(&value_to_string(value))
                        )?;
                    }
                }

                writeln!(writer, r#"    </edge>"#)?;
                edge_count += 1;
            }
        }

        // 그래프 종료
        writeln!(writer, r#"  </graph>"#)?;
        writeln!(writer, r#"</graphml>"#)?;

        writer.flush()?;

        Ok(ExportStats {
            node_count,
            edge_count,
            key_count: node_keys.len() + edge_keys.len() + 2, // +2 for label and reltype
        })
    }

    /// 특정 라벨의 노드만 GraphML로 내보내기
    #[allow(clippy::explicit_counter_loop)]
    pub async fn export_nodes(
        engine: &GraphEngine,
        label: Option<&str>,
        path: &str,
    ) -> Result<usize> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        let node_ids = match label {
            Some(l) => engine.find_nodes_by_label(l).await?,
            None => engine.all_node_ids().await,
        };

        // 속성 키 수집
        let mut node_keys: HashMap<String, GraphMLType> = HashMap::new();
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                for (key, value) in node.properties().iter() {
                    node_keys
                        .entry(key.clone())
                        .or_insert_with(|| GraphMLType::from_value(value));
                }
            }
        }

        // XML 헤더
        writeln!(writer, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(
            writer,
            r#"<graphml xmlns="http://graphml.graphdrawing.org/xmlns">"#
        )?;

        // 라벨 키
        writeln!(
            writer,
            r#"  <key id="label" for="node" attr.name="label" attr.type="string"/>"#
        )?;

        // 속성 키
        let mut key_id = 0;
        let mut node_key_map: HashMap<String, String> = HashMap::new();
        for (attr_name, attr_type) in &node_keys {
            let id = format!("n{}", key_id);
            writeln!(
                writer,
                r#"  <key id="{}" for="node" attr.name="{}" attr.type="{}"/>"#,
                id,
                escape_xml(attr_name),
                attr_type.as_str()
            )?;
            node_key_map.insert(attr_name.clone(), id);
            key_id += 1;
        }

        // 그래프 시작
        writeln!(writer, r#"  <graph id="G" edgedefault="directed">"#)?;

        // 노드 출력
        let mut count = 0;
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                writeln!(writer, r#"    <node id="n{}">"#, node_id)?;

                let labels: Vec<&str> = node.labels().iter().map(|s| s.as_str()).collect();
                if !labels.is_empty() {
                    writeln!(
                        writer,
                        r#"      <data key="label">{}</data>"#,
                        escape_xml(&labels.join(";"))
                    )?;
                }

                for (key, value) in node.properties().iter() {
                    if let Some(key_id) = node_key_map.get(key) {
                        writeln!(
                            writer,
                            r#"      <data key="{}">{}</data>"#,
                            key_id,
                            escape_xml(&value_to_string(value))
                        )?;
                    }
                }

                writeln!(writer, r#"    </node>"#)?;
                count += 1;
            }
        }

        writeln!(writer, r#"  </graph>"#)?;
        writeln!(writer, r#"</graphml>"#)?;
        writer.flush()?;

        Ok(count)
    }
}

/// GraphML Importer
pub struct GraphMLImporter;

/// 파서 상태
#[derive(Debug, Clone, PartialEq)]
enum ParseState {
    Root,
    InGraphML,
    InGraph,
    InNode(String),                 // node id
    InEdge(String, String, String), // edge id, source, target
}

impl GraphMLImporter {
    /// GraphML 파일에서 전체 그래프 가져오기
    pub async fn import_graph(engine: &GraphEngine, path: &str) -> Result<ImportStats> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        // 키 정의 저장
        let mut keys: HashMap<String, GraphMLKey> = HashMap::new();

        // 노드/엣지 데이터
        let mut current_node_data: HashMap<String, String> = HashMap::new();
        let mut current_edge_data: HashMap<String, String> = HashMap::new();

        // ID 매핑 (GraphML ID → Engine ID)
        let mut node_id_map: HashMap<String, u64> = HashMap::new();

        let mut state = ParseState::Root;

        // 멀티라인 <data> 처리용
        let mut in_data_mode = false;
        let mut current_data_key = String::new();
        let mut data_content = String::new();

        let mut node_count = 0;
        let mut edge_count = 0;

        for line_result in reader.lines() {
            let line = line_result?;
            let trimmed = line.trim();

            // key 정의 파싱
            if trimmed.starts_with("<key ") {
                if let Some(key) = Self::parse_key(trimmed) {
                    keys.insert(key.id.clone(), key);
                }
                continue;
            }

            // 상태 전환
            if trimmed.starts_with("<graphml") {
                state = ParseState::InGraphML;
            } else if trimmed.starts_with("<graph ") {
                state = ParseState::InGraph;
            } else if trimmed.starts_with("<node ") {
                if let Some(id) = Self::extract_attr(trimmed, "id") {
                    current_node_data.clear();
                    state = ParseState::InNode(id);
                }
            } else if trimmed.starts_with("</node>") {
                // 노드 생성
                if let ParseState::InNode(ref gml_id) = state {
                    let labels = current_node_data
                        .get("label")
                        .map(|s| s.split(';').map(|l| l.trim().to_string()).collect())
                        .unwrap_or_default();

                    let engine_id = engine.create_node(labels).await?;
                    node_id_map.insert(gml_id.clone(), engine_id);

                    // 속성 설정
                    let props_to_set: Vec<(String, Value)> = current_node_data
                        .iter()
                        .filter(|(k, _)| *k != "label")
                        .map(|(k, v)| {
                            let key_info = keys.get(k);
                            let attr_type = key_info
                                .map(|ki| ki.attr_type)
                                .unwrap_or(GraphMLType::String);
                            let attr_name = key_info
                                .map(|ki| ki.attr_name.clone())
                                .unwrap_or_else(|| k.clone());
                            (attr_name, string_to_value(v, attr_type))
                        })
                        .collect();

                    if !props_to_set.is_empty() {
                        engine
                            .update_node(engine_id, |node| {
                                for (attr_name, value) in props_to_set {
                                    node.set_property(attr_name, value);
                                }
                            })
                            .await?;
                    }

                    node_count += 1;
                }
                state = ParseState::InGraph;
            } else if trimmed.starts_with("<edge ") {
                let id = Self::extract_attr(trimmed, "id").unwrap_or_default();
                let source = Self::extract_attr(trimmed, "source").unwrap_or_default();
                let target = Self::extract_attr(trimmed, "target").unwrap_or_default();
                current_edge_data.clear();
                state = ParseState::InEdge(id, source, target);
            } else if trimmed.starts_with("</edge>") {
                // 엣지 생성
                if let ParseState::InEdge(_, ref source, ref target) = state {
                    let from_id = node_id_map.get(source).copied();
                    let to_id = node_id_map.get(target).copied();

                    if let (Some(from), Some(to)) = (from_id, to_id) {
                        let rel_type = current_edge_data
                            .get("reltype")
                            .cloned()
                            .unwrap_or_else(|| "RELATED".to_string());

                        let edge_id = engine.create_edge(rel_type, from, to).await?;

                        // 속성 설정
                        let props_to_set: Vec<(String, Value)> = current_edge_data
                            .iter()
                            .filter(|(k, _)| *k != "reltype")
                            .map(|(k, v)| {
                                let key_info = keys.get(k);
                                let attr_type = key_info
                                    .map(|ki| ki.attr_type)
                                    .unwrap_or(GraphMLType::String);
                                let attr_name = key_info
                                    .map(|ki| ki.attr_name.clone())
                                    .unwrap_or_else(|| k.clone());
                                (attr_name, string_to_value(v, attr_type))
                            })
                            .collect();

                        if !props_to_set.is_empty() {
                            engine
                                .update_edge(edge_id, |edge| {
                                    for (attr_name, value) in props_to_set {
                                        edge.set_property(attr_name, value);
                                    }
                                })
                                .await?;
                        }

                        edge_count += 1;
                    }
                }
                state = ParseState::InGraph;
            } else if trimmed.starts_with("<data ") {
                if let Some(data_key) = Self::extract_attr(trimmed, "key") {
                    current_data_key = data_key;
                    data_content.clear();

                    // 인라인 데이터 처리: <data key="...">value</data>
                    if let Some(start) = trimmed.find('>') {
                        if let Some(end) = trimmed.rfind("</data>") {
                            // 인라인: 시작과 종료가 같은 줄에 있음
                            let content = &trimmed[start + 1..end];
                            data_content = unescape_xml(content);

                            // 데이터 저장
                            match &state {
                                ParseState::InNode(_) => {
                                    current_node_data
                                        .insert(current_data_key.clone(), data_content.clone());
                                }
                                ParseState::InEdge(_, _, _) => {
                                    current_edge_data
                                        .insert(current_data_key.clone(), data_content.clone());
                                }
                                _ => {}
                            }
                        } else {
                            // 멀티라인 시작: <data key="...">content...
                            in_data_mode = true;
                            let content = &trimmed[start + 1..];
                            data_content = unescape_xml(content);
                        }
                    } else {
                        // <data key="..."> 만 있는 경우
                        in_data_mode = true;
                    }
                }
            } else if trimmed.contains("</data>") && in_data_mode {
                // 멀티라인 종료
                if let Some(end) = trimmed.find("</data>") {
                    let content = &trimmed[..end];
                    if !data_content.is_empty() {
                        data_content.push('\n');
                    }
                    data_content.push_str(&unescape_xml(content));
                }

                // 데이터 저장
                match &state {
                    ParseState::InNode(_) => {
                        current_node_data.insert(current_data_key.clone(), data_content.clone());
                    }
                    ParseState::InEdge(_, _, _) => {
                        current_edge_data.insert(current_data_key.clone(), data_content.clone());
                    }
                    _ => {}
                }

                in_data_mode = false;
                data_content.clear();
            } else if in_data_mode {
                // 멀티라인 내용 누적
                if !data_content.is_empty() {
                    data_content.push('\n');
                }
                data_content.push_str(&unescape_xml(trimmed));
            }
        }

        Ok(ImportStats {
            node_count,
            edge_count,
            key_count: keys.len(),
        })
    }

    /// key 요소 파싱
    fn parse_key(line: &str) -> Option<GraphMLKey> {
        let id = Self::extract_attr(line, "id")?;
        let for_type = Self::extract_attr(line, "for")
            .map(|s| KeyTarget::parse(&s))
            .unwrap_or(KeyTarget::All);
        let attr_name = Self::extract_attr(line, "attr.name").unwrap_or_else(|| id.clone());
        let attr_type = Self::extract_attr(line, "attr.type")
            .map(|s| GraphMLType::parse(&s))
            .unwrap_or(GraphMLType::String);

        Some(GraphMLKey {
            id,
            for_type,
            attr_name,
            attr_type,
        })
    }

    /// 속성 값 추출
    fn extract_attr(line: &str, attr: &str) -> Option<String> {
        let pattern = format!(r#"{}=""#, attr);
        if let Some(start) = line.find(&pattern) {
            let rest = &line[start + pattern.len()..];
            if let Some(end) = rest.find('"') {
                return Some(unescape_xml(&rest[..end]));
            }
        }
        None
    }
}
