//! GEXF (Graph Exchange XML Format) Import/Export
//!
//! v1.56.0: GEXF 표준 그래프 교환 포맷 지원
//!
//! GEXF는 Gephi에서 개발한 XML 기반 그래프 포맷으로,
//! 동적 그래프와 시각화 속성을 지원합니다.
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::io::{GexfExporter, GexfImporter};
//!
//! // GEXF로 내보내기
//! let stats = GexfExporter::export_graph(&engine, "graph.gexf")?;
//!
//! // GEXF에서 가져오기
//! let stats = GexfImporter::import_graph(&engine, "graph.gexf")?;
//! ```
//!
//! # GEXF 1.3 사양 지원
//!
//! - 정적 그래프 (mode="static")
//! - 방향/무방향 그래프 (defaultedgetype)
//! - 노드/엣지 속성 (attributes)
//! - 다양한 데이터 타입 (integer, float, double, boolean, string)
//! - 메타데이터 (creator, description, date)

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};

use chrono::Utc;

use zeta4j_graph::core::PropertyContainer;
use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::error::Result;

/// GEXF 속성 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GexfType {
    String,
    Integer,
    Float,
    Double,
    Boolean,
    Long,
}

impl GexfType {
    /// 문자열에서 타입 파싱
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "integer" | "int" => Self::Integer,
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
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Double => "double",
            Self::Boolean => "boolean",
            Self::Long => "long",
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

/// GEXF 속성 정의
#[derive(Debug, Clone)]
pub struct GexfAttribute {
    pub id: String,
    pub title: String,
    pub attr_type: GexfType,
}

/// GEXF Export 통계
#[derive(Debug, Clone, Default)]
pub struct GexfExportStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub node_attr_count: usize,
    pub edge_attr_count: usize,
}

/// GEXF Import 통계
#[derive(Debug, Clone, Default)]
pub struct GexfImportStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub node_attr_count: usize,
    pub edge_attr_count: usize,
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
fn string_to_value(s: &str, attr_type: GexfType) -> Value {
    match attr_type {
        GexfType::Boolean => Value::Boolean(s.eq_ignore_ascii_case("true") || s == "1"),
        GexfType::Integer | GexfType::Long => s
            .parse::<i64>()
            .map(Value::Integer)
            .unwrap_or(Value::String(s.to_string())),
        GexfType::Float | GexfType::Double => s
            .parse::<f64>()
            .map(Value::Float)
            .unwrap_or(Value::String(s.to_string())),
        GexfType::String => Value::String(s.to_string()),
    }
}

/// GEXF Exporter
pub struct GexfExporter;

impl GexfExporter {
    /// 전체 그래프를 GEXF로 내보내기
    #[allow(clippy::explicit_counter_loop)]
    pub async fn export_graph(engine: &GraphEngine, path: &str) -> Result<GexfExportStats> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        // 모든 노드와 엣지 수집
        let node_ids = engine.all_node_ids().await;
        let edge_ids = engine.all_edge_ids().await;

        // 속성 수집
        let mut node_attrs: HashMap<String, GexfType> = HashMap::new();
        let mut edge_attrs: HashMap<String, GexfType> = HashMap::new();

        // 노드 속성 수집
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                for (key, value) in node.properties().iter() {
                    node_attrs
                        .entry(key.clone())
                        .or_insert_with(|| GexfType::from_value(value));
                }
            }
        }

        // 엣지 속성 수집
        for edge_id in &edge_ids {
            if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
                for (key, value) in edge.properties().iter() {
                    edge_attrs
                        .entry(key.clone())
                        .or_insert_with(|| GexfType::from_value(value));
                }
            }
        }

        // XML 헤더
        writeln!(writer, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(writer, r#"<gexf xmlns="http://gexf.net/1.3""#)?;
        writeln!(
            writer,
            r#"      xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance""#
        )?;
        writeln!(
            writer,
            r#"      xsi:schemaLocation="http://gexf.net/1.3 http://gexf.net/1.3/gexf.xsd""#
        )?;
        writeln!(writer, r#"      version="1.3">"#)?;

        // 메타데이터
        writeln!(
            writer,
            r#"  <meta lastmodifieddate="{}">"#,
            Utc::now().format("%Y-%m-%d")
        )?;
        writeln!(writer, r#"    <creator>Zeta4j Graph Database</creator>"#)?;
        writeln!(
            writer,
            r#"    <description>Exported graph data</description>"#
        )?;
        writeln!(writer, r#"  </meta>"#)?;

        // 그래프 시작
        writeln!(
            writer,
            r#"  <graph mode="static" defaultedgetype="directed">"#
        )?;

        // 노드 속성 정의
        if !node_attrs.is_empty() {
            writeln!(writer, r#"    <attributes class="node">"#)?;
            let mut attr_id = 0;
            let mut node_attr_map: HashMap<String, String> = HashMap::new();
            for (attr_name, attr_type) in &node_attrs {
                let id = format!("{}", attr_id);
                writeln!(
                    writer,
                    r#"      <attribute id="{}" title="{}" type="{}"/>"#,
                    id,
                    escape_xml(attr_name),
                    attr_type.as_str()
                )?;
                node_attr_map.insert(attr_name.clone(), id);
                attr_id += 1;
            }
            writeln!(writer, r#"    </attributes>"#)?;
        }

        // 엣지 속성 정의
        if !edge_attrs.is_empty() {
            writeln!(writer, r#"    <attributes class="edge">"#)?;
            let mut attr_id = 0;
            let mut edge_attr_map: HashMap<String, String> = HashMap::new();
            for (attr_name, attr_type) in &edge_attrs {
                let id = format!("{}", attr_id);
                writeln!(
                    writer,
                    r#"      <attribute id="{}" title="{}" type="{}"/>"#,
                    id,
                    escape_xml(attr_name),
                    attr_type.as_str()
                )?;
                edge_attr_map.insert(attr_name.clone(), id);
                attr_id += 1;
            }
            writeln!(writer, r#"    </attributes>"#)?;
        }

        // 노드 속성 맵 재생성 (정의 후 사용)
        let mut node_attr_map: HashMap<String, String> = HashMap::new();
        let mut attr_id = 0;
        for attr_name in node_attrs.keys() {
            node_attr_map.insert(attr_name.clone(), format!("{}", attr_id));
            attr_id += 1;
        }

        // 엣지 속성 맵 재생성
        let mut edge_attr_map: HashMap<String, String> = HashMap::new();
        attr_id = 0;
        for attr_name in edge_attrs.keys() {
            edge_attr_map.insert(attr_name.clone(), format!("{}", attr_id));
            attr_id += 1;
        }

        // 노드 출력
        writeln!(writer, r#"    <nodes>"#)?;
        let mut node_count = 0;
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                // 라벨 생성 (첫 번째 라벨 사용)
                let labels: Vec<&str> = node.labels().iter().map(|s| s.as_str()).collect();
                let label = if labels.is_empty() {
                    format!("n{}", node_id)
                } else {
                    labels.join(";")
                };

                let has_props = !node.properties().is_empty();

                if has_props {
                    writeln!(
                        writer,
                        r#"      <node id="{}" label="{}">"#,
                        node_id,
                        escape_xml(&label)
                    )?;
                    writeln!(writer, r#"        <attvalues>"#)?;
                    for (key, value) in node.properties().iter() {
                        if let Some(attr_id) = node_attr_map.get(key) {
                            writeln!(
                                writer,
                                r#"          <attvalue for="{}" value="{}"/>"#,
                                attr_id,
                                escape_xml(&value_to_string(value))
                            )?;
                        }
                    }
                    writeln!(writer, r#"        </attvalues>"#)?;
                    writeln!(writer, r#"      </node>"#)?;
                } else {
                    writeln!(
                        writer,
                        r#"      <node id="{}" label="{}"/>"#,
                        node_id,
                        escape_xml(&label)
                    )?;
                }

                node_count += 1;
            }
        }
        writeln!(writer, r#"    </nodes>"#)?;

        // 엣지 출력
        writeln!(writer, r#"    <edges>"#)?;
        let mut edge_count = 0;
        for edge_id in &edge_ids {
            if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
                let has_props = !edge.properties().is_empty();

                if has_props {
                    writeln!(
                        writer,
                        r#"      <edge id="{}" source="{}" target="{}" label="{}">"#,
                        edge_id,
                        edge.from_node(),
                        edge.to_node(),
                        escape_xml(edge.relation_type())
                    )?;
                    writeln!(writer, r#"        <attvalues>"#)?;
                    for (key, value) in edge.properties().iter() {
                        if let Some(attr_id) = edge_attr_map.get(key) {
                            writeln!(
                                writer,
                                r#"          <attvalue for="{}" value="{}"/>"#,
                                attr_id,
                                escape_xml(&value_to_string(value))
                            )?;
                        }
                    }
                    writeln!(writer, r#"        </attvalues>"#)?;
                    writeln!(writer, r#"      </edge>"#)?;
                } else {
                    writeln!(
                        writer,
                        r#"      <edge id="{}" source="{}" target="{}" label="{}"/>"#,
                        edge_id,
                        edge.from_node(),
                        edge.to_node(),
                        escape_xml(edge.relation_type())
                    )?;
                }

                edge_count += 1;
            }
        }
        writeln!(writer, r#"    </edges>"#)?;

        // 그래프 종료
        writeln!(writer, r#"  </graph>"#)?;
        writeln!(writer, r#"</gexf>"#)?;

        writer.flush()?;

        Ok(GexfExportStats {
            node_count,
            edge_count,
            node_attr_count: node_attrs.len(),
            edge_attr_count: edge_attrs.len(),
        })
    }

    /// 특정 라벨의 노드만 GEXF로 내보내기
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

        // 속성 수집
        let mut node_attrs: HashMap<String, GexfType> = HashMap::new();
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                for (key, value) in node.properties().iter() {
                    node_attrs
                        .entry(key.clone())
                        .or_insert_with(|| GexfType::from_value(value));
                }
            }
        }

        // XML 헤더
        writeln!(writer, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(
            writer,
            r#"<gexf xmlns="http://gexf.net/1.3" version="1.3">"#
        )?;
        writeln!(
            writer,
            r#"  <meta lastmodifieddate="{}">"#,
            Utc::now().format("%Y-%m-%d")
        )?;
        writeln!(writer, r#"    <creator>Zeta4j</creator>"#)?;
        writeln!(writer, r#"  </meta>"#)?;
        writeln!(
            writer,
            r#"  <graph mode="static" defaultedgetype="directed">"#
        )?;

        // 속성 정의
        if !node_attrs.is_empty() {
            writeln!(writer, r#"    <attributes class="node">"#)?;
            let mut attr_id = 0;
            for (attr_name, attr_type) in &node_attrs {
                writeln!(
                    writer,
                    r#"      <attribute id="{}" title="{}" type="{}"/>"#,
                    attr_id,
                    escape_xml(attr_name),
                    attr_type.as_str()
                )?;
                attr_id += 1;
            }
            writeln!(writer, r#"    </attributes>"#)?;
        }

        // 속성 맵 재생성
        let mut node_attr_map: HashMap<String, String> = HashMap::new();
        let mut attr_id = 0;
        for attr_name in node_attrs.keys() {
            node_attr_map.insert(attr_name.clone(), format!("{}", attr_id));
            attr_id += 1;
        }

        // 노드 출력
        writeln!(writer, r#"    <nodes>"#)?;
        let mut count = 0;
        for node_id in &node_ids {
            if let Ok(Some(node)) = engine.get_node(*node_id).await {
                let labels: Vec<&str> = node.labels().iter().map(|s| s.as_str()).collect();
                let label_str = if labels.is_empty() {
                    format!("n{}", node_id)
                } else {
                    labels.join(";")
                };

                let has_props = !node.properties().is_empty();

                if has_props {
                    writeln!(
                        writer,
                        r#"      <node id="{}" label="{}">"#,
                        node_id,
                        escape_xml(&label_str)
                    )?;
                    writeln!(writer, r#"        <attvalues>"#)?;
                    for (key, value) in node.properties().iter() {
                        if let Some(attr_id) = node_attr_map.get(key) {
                            writeln!(
                                writer,
                                r#"          <attvalue for="{}" value="{}"/>"#,
                                attr_id,
                                escape_xml(&value_to_string(value))
                            )?;
                        }
                    }
                    writeln!(writer, r#"        </attvalues>"#)?;
                    writeln!(writer, r#"      </node>"#)?;
                } else {
                    writeln!(
                        writer,
                        r#"      <node id="{}" label="{}"/>"#,
                        node_id,
                        escape_xml(&label_str)
                    )?;
                }

                count += 1;
            }
        }
        writeln!(writer, r#"    </nodes>"#)?;
        writeln!(writer, r#"    <edges/>"#)?;
        writeln!(writer, r#"  </graph>"#)?;
        writeln!(writer, r#"</gexf>"#)?;
        writer.flush()?;

        Ok(count)
    }
}

/// GEXF Importer
pub struct GexfImporter;

/// 파서 상태
#[derive(Debug, Clone, PartialEq)]
enum ParseState {
    Root,
    InGexf,
    InGraph,
    InNodeAttributes,
    InEdgeAttributes,
    InNodes,
    InNode(String, String), // node id, label
    InNodeAttValues,
    InEdges,
    InEdge(String, String, String, String), // edge id, source, target, label
    InEdgeAttValues,
}

impl GexfImporter {
    /// GEXF 파일에서 전체 그래프 가져오기
    pub async fn import_graph(engine: &GraphEngine, path: &str) -> Result<GexfImportStats> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        // 속성 정의 저장
        let mut node_attrs: HashMap<String, GexfAttribute> = HashMap::new();
        let mut edge_attrs: HashMap<String, GexfAttribute> = HashMap::new();

        // 노드/엣지 데이터
        let mut current_attvalues: HashMap<String, String> = HashMap::new();

        // ID 매핑 (GEXF ID → Engine ID)
        let mut node_id_map: HashMap<String, u64> = HashMap::new();

        let mut state = ParseState::Root;
        let mut current_node: Option<(String, String)> = None; // (id, label)
        let mut current_edge: Option<(String, String, String, String)> = None; // (id, source, target, label)

        let mut node_count = 0;
        let mut edge_count = 0;

        for line_result in reader.lines() {
            let line = line_result?;
            let trimmed = line.trim();

            // 상태 전환
            if trimmed.starts_with("<gexf") {
                state = ParseState::InGexf;
            } else if trimmed.starts_with("<graph ") {
                state = ParseState::InGraph;
            } else if trimmed.starts_with("<attributes ") && trimmed.contains("class=\"node\"") {
                state = ParseState::InNodeAttributes;
            } else if trimmed.starts_with("<attributes ") && trimmed.contains("class=\"edge\"") {
                state = ParseState::InEdgeAttributes;
            } else if trimmed == "</attributes>" {
                state = ParseState::InGraph;
            } else if trimmed.starts_with("<attribute ") {
                if let Some(attr) = Self::parse_attribute(trimmed) {
                    match state {
                        ParseState::InNodeAttributes => {
                            node_attrs.insert(attr.id.clone(), attr);
                        }
                        ParseState::InEdgeAttributes => {
                            edge_attrs.insert(attr.id.clone(), attr);
                        }
                        _ => {}
                    }
                }
            } else if trimmed == "<nodes>" {
                state = ParseState::InNodes;
            } else if trimmed == "</nodes>" {
                state = ParseState::InGraph;
            } else if trimmed.starts_with("<node ") {
                let id = Self::extract_attr(trimmed, "id").unwrap_or_default();
                let label =
                    Self::extract_attr(trimmed, "label").unwrap_or_else(|| format!("n{}", id));
                current_node = Some((id.clone(), label.clone()));
                current_attvalues.clear();

                // 자체 종료 태그인 경우
                if trimmed.ends_with("/>") {
                    // 노드 생성
                    let labels: Vec<String> = label
                        .split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let engine_id = engine.create_node(labels).await?;
                    node_id_map.insert(id, engine_id);
                    node_count += 1;
                    current_node = None;
                } else {
                    state = ParseState::InNode(id, label);
                }
            } else if trimmed == "</node>" {
                if let Some((id, label)) = current_node.take() {
                    // 노드 생성
                    let labels: Vec<String> = label
                        .split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let engine_id = engine.create_node(labels).await?;
                    node_id_map.insert(id, engine_id);

                    // 속성 설정
                    if !current_attvalues.is_empty() {
                        let props_to_set: Vec<(String, Value)> = current_attvalues
                            .iter()
                            .filter_map(|(attr_id, value)| {
                                node_attrs.get(attr_id).map(|attr| {
                                    (attr.title.clone(), string_to_value(value, attr.attr_type))
                                })
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
                    }

                    node_count += 1;
                }
                state = ParseState::InNodes;
            } else if trimmed == "<attvalues>" {
                match state {
                    ParseState::InNode(_, _) => state = ParseState::InNodeAttValues,
                    ParseState::InEdge(_, _, _, _) => state = ParseState::InEdgeAttValues,
                    _ => {}
                }
            } else if trimmed == "</attvalues>" {
                match state {
                    ParseState::InNodeAttValues => {
                        if let Some((ref id, ref label)) = current_node {
                            state = ParseState::InNode(id.clone(), label.clone());
                        }
                    }
                    ParseState::InEdgeAttValues => {
                        if let Some((ref id, ref src, ref tgt, ref lbl)) = current_edge {
                            state = ParseState::InEdge(
                                id.clone(),
                                src.clone(),
                                tgt.clone(),
                                lbl.clone(),
                            );
                        }
                    }
                    _ => {}
                }
            } else if trimmed.starts_with("<attvalue ") {
                if let (Some(for_id), Some(value)) = (
                    Self::extract_attr(trimmed, "for"),
                    Self::extract_attr(trimmed, "value"),
                ) {
                    current_attvalues.insert(for_id, value);
                }
            } else if trimmed == "<edges>" || trimmed == "<edges/>" {
                state = ParseState::InEdges;
            } else if trimmed == "</edges>" {
                state = ParseState::InGraph;
            } else if trimmed.starts_with("<edge ") {
                let id = Self::extract_attr(trimmed, "id").unwrap_or_default();
                let source = Self::extract_attr(trimmed, "source").unwrap_or_default();
                let target = Self::extract_attr(trimmed, "target").unwrap_or_default();
                let label =
                    Self::extract_attr(trimmed, "label").unwrap_or_else(|| "RELATED".to_string());
                current_edge = Some((id.clone(), source.clone(), target.clone(), label.clone()));
                current_attvalues.clear();

                // 자체 종료 태그인 경우
                if trimmed.ends_with("/>") {
                    if let (Some(&from_id), Some(&to_id)) =
                        (node_id_map.get(&source), node_id_map.get(&target))
                    {
                        engine.create_edge(label, from_id, to_id).await?;
                        edge_count += 1;
                    }
                    current_edge = None;
                } else {
                    state = ParseState::InEdge(id, source, target, label);
                }
            } else if trimmed == "</edge>" {
                if let Some((_, source, target, label)) = current_edge.take()
                    && let (Some(&from_id), Some(&to_id)) =
                        (node_id_map.get(&source), node_id_map.get(&target))
                {
                    let edge_id = engine.create_edge(label, from_id, to_id).await?;

                    // 속성 설정
                    if !current_attvalues.is_empty() {
                        let props_to_set: Vec<(String, Value)> = current_attvalues
                            .iter()
                            .filter_map(|(attr_id, value)| {
                                edge_attrs.get(attr_id).map(|attr| {
                                    (attr.title.clone(), string_to_value(value, attr.attr_type))
                                })
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
                    }

                    edge_count += 1;
                }
                state = ParseState::InEdges;
            }
        }

        Ok(GexfImportStats {
            node_count,
            edge_count,
            node_attr_count: node_attrs.len(),
            edge_attr_count: edge_attrs.len(),
        })
    }

    /// attribute 요소 파싱
    fn parse_attribute(line: &str) -> Option<GexfAttribute> {
        let id = Self::extract_attr(line, "id")?;
        let title = Self::extract_attr(line, "title").unwrap_or_else(|| id.clone());
        let attr_type = Self::extract_attr(line, "type")
            .map(|s| GexfType::parse(&s))
            .unwrap_or(GexfType::String);

        Some(GexfAttribute {
            id,
            title,
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
