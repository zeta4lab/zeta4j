// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Unified Query Interface (102 Phase 2, Phase 3)
//!
//! HTTP API와 Bolt API 모두에서 사용할 수 있는 통합 쿼리 인터페이스
//!
//! v1.79.0: async 메서드로 변경 (DatabaseManager async 지원)

use indexmap::IndexMap;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;

use zeta4j_cypher_executor::ast::Query;
use zeta4j_cypher_executor::executor::{
    QueryExecutor as CypherExecutor, QueryResult as CypherResult,
};
use zeta4j_cypher_executor::parser::parse_query;
use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;

/// 연결 정보 (Phase 3: Connection Info)
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// 서버 주소
    pub server_address: String,
    /// 프로토콜 버전 (예: "Bolt/5.0", "HTTP/1.1")
    pub protocol_version: String,
    /// 연결 ID
    pub connection_id: String,
    /// 데이터베이스 이름
    pub database: String,
    /// 사용자 이름
    pub user: Option<String>,
}

impl ConnectionInfo {
    /// 새 연결 정보 생성
    pub fn new(server_address: SocketAddr, protocol: &str, connection_id: &str) -> Self {
        Self {
            server_address: server_address.to_string(),
            protocol_version: protocol.to_string(),
            connection_id: connection_id.to_string(),
            database: "zeta4j".to_string(),
            user: None,
        }
    }

    /// 데이터베이스 설정
    pub fn with_database(mut self, database: &str) -> Self {
        self.database = database.to_string();
        self
    }

    /// 사용자 설정
    pub fn with_user(mut self, user: &str) -> Self {
        self.user = Some(user.to_string());
        self
    }

    /// Bolt 연결 정보 생성
    pub fn bolt(server_address: SocketAddr, version: &str, connection_id: &str) -> Self {
        Self::new(server_address, &format!("Bolt/{}", version), connection_id)
    }

    /// HTTP 연결 정보 생성
    pub fn http(server_address: SocketAddr, connection_id: &str) -> Self {
        Self::new(server_address, "HTTP/1.1", connection_id)
    }
}

/// 통합 쿼리 결과
#[derive(Debug, Clone)]
pub struct UnifiedQueryResult {
    /// 컬럼 이름
    pub columns: Vec<String>,
    /// 결과 행
    pub rows: Vec<Vec<Value>>,
    /// 쿼리 통계
    pub stats: QueryStatistics,
    /// 쿼리 계획 (EXPLAIN 시)
    pub plan: Option<String>,
}

/// 쿼리 통계
#[derive(Debug, Clone, Default)]
pub struct QueryStatistics {
    /// 생성된 노드 수
    pub nodes_created: u64,
    /// 삭제된 노드 수
    pub nodes_deleted: u64,
    /// 생성된 관계 수
    pub relationships_created: u64,
    /// 삭제된 관계 수
    pub relationships_deleted: u64,
    /// 설정된 속성 수
    pub properties_set: u64,
    /// 추가된 레이블 수
    pub labels_added: u64,
    /// 제거된 레이블 수
    pub labels_removed: u64,
    /// 변경사항 포함 여부
    pub contains_updates: bool,
    /// 계획 시간 (ms)
    pub planning_time_ms: u64,
    /// 실행 시간 (ms)
    pub execution_time_ms: u64,
    /// 총 행 수
    pub total_rows: u64,
}

/// 스트리밍 레코드
pub type StreamRecord = Vec<Value>;

/// 스트리밍 결과 스트림
pub type RecordStream = Pin<Box<dyn Stream<Item = Result<StreamRecord, QueryError>> + Send>>;

/// 쿼리 에러
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Plan error: {0}")]
    Plan(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("Timeout")]
    Timeout,

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
}

/// 통합 쿼리 실행기 트레이트
///
/// HTTP API와 Bolt API 모두에서 사용할 수 있는 공통 인터페이스
///
/// v1.79.0: async 메서드로 변경 (DatabaseManager async 지원)
#[async_trait]
pub trait UnifiedQueryExecutor: Send + Sync {
    /// 쿼리 실행
    async fn execute(
        &self,
        query: &str,
        params: IndexMap<String, Value>,
    ) -> Result<UnifiedQueryResult, QueryError>;

    /// 스트리밍 쿼리 실행
    async fn execute_streaming(
        &self,
        query: &str,
        params: IndexMap<String, Value>,
    ) -> Result<StreamingQueryResult, QueryError>;

    /// 트랜잭션 시작
    fn begin_transaction(&self) -> Result<u64, QueryError>;

    /// 트랜잭션 내 쿼리 실행
    async fn execute_in_transaction(
        &self,
        tx_id: u64,
        query: &str,
        params: IndexMap<String, Value>,
    ) -> Result<UnifiedQueryResult, QueryError>;

    /// 트랜잭션 커밋
    fn commit_transaction(&self, tx_id: u64) -> Result<String, QueryError>;

    /// 트랜잭션 롤백
    fn rollback_transaction(&self, tx_id: u64) -> Result<(), QueryError>;
}

/// 스트리밍 쿼리 결과
pub struct StreamingQueryResult {
    /// 컬럼 이름
    pub columns: Vec<String>,
    /// 레코드 스트림
    pub records: RecordStream,
    /// 쿼리 ID
    pub query_id: String,
}

/// 기본 쿼리 실행기 구현
pub struct DefaultQueryExecutor {
    engine: Arc<GraphEngine>,
}

impl DefaultQueryExecutor {
    /// 새 실행기 생성
    pub fn new(engine: Arc<GraphEngine>) -> Self {
        Self { engine }
    }

    /// 쿼리 파싱
    fn parse_query(&self, query: &str) -> Result<Query, QueryError> {
        parse_query(query)
            .map(|(_, q)| q)
            .map_err(|e| QueryError::Parse(format!("{:?}", e)))
    }

    /// 쿼리에서 컬럼 추출
    fn extract_columns(&self, query: &Query) -> Vec<String> {
        use zeta4j_cypher_executor::ast::{Expression, QueryBody, ReturnItem, Statement};

        fn extract_from_body(body: &QueryBody) -> Vec<String> {
            match body {
                QueryBody::Single(single) => {
                    for stmt in &single.statements {
                        if let Statement::Return(ret) = stmt {
                            return ret
                                .items
                                .iter()
                                .map(|item| match item {
                                    ReturnItem::Star => "*".to_string(),
                                    ReturnItem::Expression { expr, alias } => {
                                        if let Some(a) = alias {
                                            a.clone()
                                        } else {
                                            match expr {
                                                Expression::Variable(name) => name.clone(),
                                                Expression::PropertyAccess(var, prop) => {
                                                    format!("{}.{}", var, prop)
                                                }
                                                _ => "column".to_string(),
                                            }
                                        }
                                    }
                                })
                                .collect();
                        }
                    }
                    vec![]
                }
                QueryBody::Union { left, .. } => extract_from_body(left),
            }
        }

        extract_from_body(&query.body)
    }
}

#[async_trait]
impl UnifiedQueryExecutor for DefaultQueryExecutor {
    async fn execute(
        &self,
        query: &str,
        params: IndexMap<String, Value>,
    ) -> Result<UnifiedQueryResult, QueryError> {
        // 쿼리 파싱
        let parsed = self.parse_query(query)?;
        let _columns = self.extract_columns(&parsed);

        // 쿼리 실행
        let executor = CypherExecutor::new(self.engine.clone());
        let result = executor
            .execute_with_result_and_params(parsed, params)
            .await
            .map_err(|e| QueryError::Execution(e.to_string()))?;

        // 결과 변환
        match result {
            CypherResult::Rows { columns, rows, .. } => Ok(UnifiedQueryResult {
                columns,
                rows,
                stats: QueryStatistics::default(),
                plan: None,
            }),
            CypherResult::Plan(plan) => Ok(UnifiedQueryResult {
                columns: vec![],
                rows: vec![],
                stats: QueryStatistics::default(),
                plan: Some(plan.to_tree_string()),
            }),
            CypherResult::Analyze {
                plan,
                stats,
                columns,
                rows,
                ..
            } => Ok(UnifiedQueryResult {
                columns,
                rows,
                stats: QueryStatistics {
                    planning_time_ms: stats.planning_time.as_millis() as u64,
                    execution_time_ms: stats.execution_time.as_millis() as u64,
                    total_rows: stats.total_rows,
                    ..Default::default()
                },
                plan: Some(plan.to_tree_string()),
            }),
        }
    }

    async fn execute_streaming(
        &self,
        query: &str,
        params: IndexMap<String, Value>,
    ) -> Result<StreamingQueryResult, QueryError> {
        // 쿼리 파싱
        let parsed = self.parse_query(query)?;
        let columns = self.extract_columns(&parsed);

        // 쿼리 실행
        let executor = CypherExecutor::new(self.engine.clone());
        let result = executor
            .execute_with_result_and_params(parsed, params)
            .await
            .map_err(|e| QueryError::Execution(e.to_string()))?;

        // 스트림 생성
        let query_id = uuid::Uuid::now_v7().to_string();

        let rows = match result {
            CypherResult::Rows { rows, .. } => rows,
            CypherResult::Plan(_) => vec![],
            CypherResult::Analyze { rows, .. } => rows,
        };

        // 결과를 스트림으로 변환
        let stream = futures::stream::iter(rows.into_iter().map(Ok));

        Ok(StreamingQueryResult {
            columns,
            records: Box::pin(stream),
            query_id,
        })
    }

    fn begin_transaction(&self) -> Result<u64, QueryError> {
        // 기본 구현: GraphEngine의 트랜잭션 매니저 사용
        self.engine
            .begin_transaction()
            .map_err(|e| QueryError::Execution(e.to_string()))
    }

    async fn execute_in_transaction(
        &self,
        _tx_id: u64,
        query: &str,
        params: IndexMap<String, Value>,
    ) -> Result<UnifiedQueryResult, QueryError> {
        // 현재는 기본 execute와 동일 (추후 트랜잭션 컨텍스트 지원)
        self.execute(query, params).await
    }

    fn commit_transaction(&self, tx_id: u64) -> Result<String, QueryError> {
        self.engine
            .commit_transaction(tx_id)
            .map_err(|e| QueryError::Execution(e.to_string()))?;

        Ok(format!("zeta4j:bookmark:v1:tx{}", tx_id))
    }

    fn rollback_transaction(&self, tx_id: u64) -> Result<(), QueryError> {
        self.engine
            .rollback_transaction(tx_id)
            .map_err(|e| QueryError::Execution(e.to_string()))
    }
}

/// 공유 쿼리 실행기
pub type SharedQueryExecutor = Arc<dyn UnifiedQueryExecutor>;

/// 쿼리 실행기 생성 헬퍼
pub fn create_query_executor(engine: Arc<GraphEngine>) -> SharedQueryExecutor {
    Arc::new(DefaultQueryExecutor::new(engine))
}

/// 결과 변환 유틸리티
pub mod convert {
    use super::*;
    use crate::server::bolt::packstream::PackStreamValue;

    /// Value를 PackStreamValue로 변환 (Bolt용)
    pub fn value_to_packstream(value: &Value) -> PackStreamValue {
        match value {
            Value::Null => PackStreamValue::Null,
            Value::Boolean(b) => PackStreamValue::Boolean(*b),
            Value::Integer(i) => PackStreamValue::Integer(*i),
            Value::Float(f) => PackStreamValue::Float(*f),
            Value::String(s) => PackStreamValue::String(s.clone()),
            Value::Array(list) => {
                PackStreamValue::List(list.iter().map(value_to_packstream).collect())
            }
            Value::Map(map) => PackStreamValue::Map(
                map.iter()
                    .map(|(k, v)| (k.clone(), value_to_packstream(v)))
                    .collect(),
            ),
            Value::Node(id) => {
                let mut props = HashMap::new();
                props.insert("_id".to_string(), PackStreamValue::Integer(*id as i64));
                PackStreamValue::Map(props)
            }
            Value::Edge(id) => {
                let mut props = HashMap::new();
                props.insert("_id".to_string(), PackStreamValue::Integer(*id as i64));
                PackStreamValue::Map(props)
            }
            Value::Path(_) => PackStreamValue::Null,
            Value::DateTime(dt) => PackStreamValue::String(dt.to_rfc3339()),
            Value::Date(d) => PackStreamValue::String(d.to_string()),
            Value::Time(t) => PackStreamValue::String(t.to_string()),
            Value::LocalDateTime(ldt) => PackStreamValue::String(ldt.to_string()),
            Value::Duration(d) => PackStreamValue::String(format!("{:?}", d)),
            Value::Vector(v) => {
                use crate::server::bolt::packstream::PackStreamVector;
                let ps_vec = PackStreamVector::new(v.as_slice().to_vec());
                ps_vec.to_value()
            }
        }
    }

    /// Value를 JSON으로 변환 (HTTP용)
    pub fn value_to_json(value: &Value) -> serde_json::Value {
        match value {
            Value::Null => serde_json::Value::Null,
            Value::Boolean(b) => serde_json::Value::Bool(*b),
            Value::Integer(i) => serde_json::json!(*i),
            Value::Float(f) => serde_json::json!(*f),
            Value::String(s) => serde_json::Value::String(s.clone()),
            Value::Array(list) => {
                let items: Vec<_> = list.iter().map(value_to_json).collect();
                serde_json::Value::Array(items)
            }
            Value::Map(map) => {
                let obj: serde_json::Map<String, serde_json::Value> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), value_to_json(v)))
                    .collect();
                serde_json::Value::Object(obj)
            }
            Value::Node(node_id) => {
                serde_json::json!({
                    "_type": "node",
                    "id": node_id
                })
            }
            Value::Edge(edge_id) => {
                serde_json::json!({
                    "_type": "edge",
                    "id": edge_id
                })
            }
            Value::Path(path) => {
                serde_json::json!({
                    "_type": "path",
                    "nodes": path.nodes,
                    "edges": path.edges
                })
            }
            Value::Duration(d) => {
                serde_json::json!({
                    "_type": "duration",
                    "months": d.months,
                    "days": d.days,
                    "seconds": d.seconds,
                    "nanoseconds": d.nanoseconds
                })
            }
            Value::Date(d) => {
                serde_json::json!({
                    "_type": "date",
                    "value": d.to_string()
                })
            }
            Value::Time(t) => {
                serde_json::json!({
                    "_type": "time",
                    "value": t.to_string()
                })
            }
            Value::DateTime(dt) => {
                serde_json::json!({
                    "_type": "datetime",
                    "value": dt.to_rfc3339()
                })
            }
            Value::LocalDateTime(ldt) => {
                serde_json::json!({
                    "_type": "localdatetime",
                    "value": ldt.to_string()
                })
            }
            Value::Vector(v) => {
                serde_json::json!({
                    "_type": "vector",
                    "dimensions": v.dimensions(),
                    "data": v.as_slice()
                })
            }
        }
    }

    /// PackStreamValue를 Value로 변환
    pub fn packstream_to_value(value: &PackStreamValue) -> Value {
        match value {
            PackStreamValue::Null => Value::Null,
            PackStreamValue::Boolean(b) => Value::Boolean(*b),
            PackStreamValue::Integer(i) => Value::Integer(*i),
            PackStreamValue::Float(f) => Value::Float(*f),
            PackStreamValue::String(s) => Value::String(s.clone()),
            PackStreamValue::List(list) => {
                Value::Array(list.iter().map(packstream_to_value).collect())
            }
            PackStreamValue::Map(map) => Value::Map(
                map.iter()
                    .map(|(k, v)| (k.clone(), packstream_to_value(v)))
                    .collect(),
            ),
            PackStreamValue::Bytes(bytes) => Value::String(format!("{:?}", bytes)),
            PackStreamValue::Structure(_) => Value::Null,
        }
    }

    /// JSON을 Value로 변환
    pub fn json_to_value(json: &serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Null
                }
            }
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Array(arr) => {
                let items: Vec<_> = arr.iter().map(json_to_value).collect();
                Value::Array(items)
            }
            serde_json::Value::Object(obj) => {
                let map: IndexMap<String, Value> = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), json_to_value(v)))
                    .collect();
                Value::Map(map)
            }
        }
    }
}
