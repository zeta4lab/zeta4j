// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Streaming Executor (M2.4)
//!
//! 스트리밍 쿼리 실행기 - HTTP 스트리밍 API와 쿼리 실행기 통합

use indexmap::IndexMap;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

use zeta4j_cypher_executor::executor::{MutationStats, QueryExecutor, QueryResult};
use zeta4j_cypher_executor::parser::parse_query;
use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::storage::memory::MemoryReservation;

use super::backpressure::{SharedBackpressure, StreamBackpressure};
use super::poll::{PollManager, SharedPollManager};
use super::types::{StreamError, StreamingConfig};

pub(crate) type SharedQueryMemory = Arc<Mutex<Option<MemoryReservation>>>;

/// 스트리밍 실행기
pub struct StreamingExecutor {
    /// 그래프 엔진
    engine: Arc<GraphEngine>,
    /// 설정
    config: StreamingConfig,
    /// 백프레셔 제어
    backpressure: SharedBackpressure,
    /// Poll 관리자
    poll_manager: SharedPollManager,
}

impl StreamingExecutor {
    /// 새 스트리밍 실행기 생성
    pub fn new(engine: Arc<GraphEngine>, config: StreamingConfig) -> Self {
        Self {
            engine,
            backpressure: Arc::new(StreamBackpressure::new(100)),
            poll_manager: Arc::new(PollManager::default()),
            config,
        }
    }

    /// 백프레셔 설정
    pub fn with_backpressure(mut self, backpressure: SharedBackpressure) -> Self {
        self.backpressure = backpressure;
        self
    }

    /// Poll 관리자 설정
    pub fn with_poll_manager(mut self, poll_manager: SharedPollManager) -> Self {
        self.poll_manager = poll_manager;
        self
    }

    /// 스트리밍 쿼리 실행
    pub async fn execute_streaming(
        &self,
        query: &str,
        parameters: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<StreamingResult, StreamError> {
        // 백프레셔 체크
        let permit = self
            .backpressure
            .try_acquire()
            .map_err(|_| StreamError::BackpressureExceeded)?;

        // 쿼리 ID 생성
        let query_id = Uuid::now_v7().to_string();

        // 쿼리 파싱
        let parsed_query = parse_query(query)
            .map_err(|e| StreamError::QueryError(format!("Parse error: {:?}", e)))?
            .1;

        // 컬럼 이름 추출 (RETURN 절에서)
        let columns = self.extract_columns_from_query(&parsed_query);

        // 파라미터 변환 (serde_json::Value -> zeta4j_graph::core::Value)
        let core_params = self.convert_parameters(parameters);

        // 채널 생성
        let (tx, rx) = mpsc::channel(self.config.buffer_size);
        let batch_size = self.config.default_batch_size;
        let memory_reservation = Arc::new(Mutex::new(None));
        let task_memory_reservation = Arc::clone(&memory_reservation);

        // 쿼리 실행 (백그라운드)
        let engine = self.engine.clone();
        let timeout = Duration::from_millis(self.config.timeout_ms);

        tokio::spawn(async move {
            let _permit = permit; // 스트리밍 완료 시까지 퍼밋 유지
            let _timeout = timeout; // 추후 타임아웃 처리에 사용

            // QueryExecutor 생성 및 실행
            let executor = QueryExecutor::new(engine);
            let result = executor
                .execute_with_result_and_params(parsed_query, core_params)
                .await;

            match result {
                Ok(QueryResult::Rows {
                    rows,
                    mutation_stats,
                    _memory_reservation,
                    ..
                }) => {
                    *task_memory_reservation.lock() = _memory_reservation;
                    // 결과를 청크 단위로 스트리밍
                    for chunk in rows.chunks(batch_size) {
                        for row in chunk {
                            let json_row = row_to_json(row);
                            if tx.send(Ok(json_row)).await.is_err() {
                                // 수신자가 닫힘 (클라이언트 연결 끊김)
                                return;
                            }
                        }
                    }
                    // mutation 통계 전송
                    if mutation_stats.has_updates() {
                        let _ = tx.send(Ok(mutation_stats_to_json(&mutation_stats))).await;
                    }
                }
                Ok(QueryResult::Plan(plan)) => {
                    // EXPLAIN 결과
                    let plan_json = serde_json::json!({
                        "type": "plan",
                        "plan": plan.to_tree_string()
                    });
                    let _ = tx.send(Ok(plan_json)).await;
                }
                Ok(QueryResult::Analyze {
                    plan,
                    stats,
                    rows,
                    mutation_stats,
                    _memory_reservation,
                    ..
                }) => {
                    *task_memory_reservation.lock() = _memory_reservation;
                    // EXPLAIN ANALYZE 결과
                    for row in rows {
                        let json_row = row_to_json(&row);
                        if tx.send(Ok(json_row)).await.is_err() {
                            return;
                        }
                    }
                    // 실행 통계 전송
                    let stats_json = serde_json::json!({
                        "type": "stats",
                        "plan": plan.to_tree_string(),
                        "planningTime": stats.planning_time.as_millis() as u64,
                        "executionTime": stats.execution_time.as_millis() as u64,
                        "totalRows": stats.total_rows
                    });
                    let _ = tx.send(Ok(stats_json)).await;
                    // mutation 통계 전송
                    if mutation_stats.has_updates() {
                        let _ = tx.send(Ok(mutation_stats_to_json(&mutation_stats))).await;
                    }
                }
                Err(e) => {
                    // 에러 전송
                    let _ = tx.send(Err(StreamError::QueryError(e.to_string()))).await;
                }
            }
            // tx가 drop되면 채널이 닫힘
        });

        Ok(StreamingResult {
            query_id,
            columns,
            receiver: rx,
            memory_reservation,
        })
    }

    /// 쿼리에서 컬럼 이름 추출
    fn extract_columns_from_query(
        &self,
        query: &zeta4j_cypher_executor::ast::Query,
    ) -> Vec<String> {
        use zeta4j_cypher_executor::ast::{Expression, QueryBody, ReturnItem, Statement};

        fn extract_from_body(body: &QueryBody) -> Vec<String> {
            match body {
                QueryBody::Single(single) => {
                    for stmt in &single.statements {
                        if let Statement::Return(ret) = stmt {
                            return ret
                                .items
                                .iter()
                                .map(|item| {
                                    match item {
                                        ReturnItem::Star => "*".to_string(),
                                        ReturnItem::Expression { expr, alias } => {
                                            // alias가 있으면 alias 사용, 없으면 표현식에서 추론
                                            if let Some(a) = alias {
                                                a.clone()
                                            } else {
                                                match expr {
                                                    Expression::Variable(name) => name.clone(),
                                                    Expression::PropertyAccess(
                                                        variable,
                                                        property,
                                                    ) => {
                                                        format!("{}.{}", variable, property)
                                                    }
                                                    _ => "column".to_string(),
                                                }
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

    /// 파라미터 변환 (serde_json -> core::Value)
    fn convert_parameters(
        &self,
        params: &serde_json::Map<String, serde_json::Value>,
    ) -> IndexMap<String, Value> {
        params
            .iter()
            .map(|(k, v)| (k.clone(), json_to_value(v)))
            .collect()
    }

    /// 취소 가능한 스트리밍 쿼리 실행
    pub async fn execute_cancellable(
        &self,
        query: &str,
        parameters: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<CancellableStreamingResult, StreamError> {
        let (cancel_tx, _cancel_rx) = mpsc::channel::<()>(1);
        let result = self.execute_streaming(query, parameters).await?;

        let query_id = result.query_id.clone();

        Ok(CancellableStreamingResult {
            query_id,
            columns: result.columns,
            receiver: result.receiver,
            cancel_sender: cancel_tx,
            _memory_reservation: result.memory_reservation,
        })
    }

    /// Poll 쿼리 시작
    pub async fn start_poll_query(
        &self,
        query: &str,
        parameters: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<String, StreamError> {
        let query_id = Uuid::now_v7().to_string();

        // 쿼리 파싱
        let parsed_query = parse_query(query)
            .map_err(|e| StreamError::QueryError(format!("Parse error: {:?}", e)))?
            .1;

        // 파라미터 변환
        let core_params = self.convert_parameters(parameters);

        // 쿼리 등록
        self.poll_manager
            .register_query(query_id.clone())
            .await
            .map_err(|e| StreamError::QueryError(e.to_string()))?;

        // 쿼리 실행 (백그라운드)
        let engine = self.engine.clone();
        let poll_manager = self.poll_manager.clone();
        let qid = query_id.clone();

        tokio::spawn(async move {
            // QueryExecutor 생성 및 실행
            let executor = QueryExecutor::new(engine);
            let result = executor
                .execute_with_result_and_params(parsed_query, core_params)
                .await;

            match result {
                Ok(QueryResult::Rows {
                    rows,
                    columns,
                    mutation_stats,
                    _memory_reservation,
                    ..
                }) => {
                    // 컬럼 정보를 먼저 설정 (race condition 방지)
                    if let Err(e) = poll_manager.set_columns(&qid, columns).await {
                        tracing::error!("Failed to set columns for query {}: {}", qid, e);
                    }

                    // 결과를 Poll 관리자에 저장
                    let mut json_rows: Vec<serde_json::Value> =
                        rows.iter().map(|row| row_to_json(row)).collect();

                    // mutation 통계 추가
                    if mutation_stats.has_updates() {
                        json_rows.push(mutation_stats_to_json(&mutation_stats));
                    }

                    if let Err(e) = poll_manager
                        .add_results_with_memory(&qid, json_rows, _memory_reservation)
                        .await
                    {
                        tracing::error!("Failed to add poll results for query {}: {}", qid, e);
                    }
                }
                Ok(QueryResult::Plan(plan)) => {
                    if let Err(e) = poll_manager
                        .set_columns(&qid, vec!["Plan".to_string()])
                        .await
                    {
                        tracing::error!("Failed to set columns for query {}: {}", qid, e);
                    }
                    let plan_json = serde_json::json!({
                        "type": "plan",
                        "plan": plan.to_tree_string()
                    });
                    if let Err(e) = poll_manager.add_results(&qid, vec![plan_json]).await {
                        tracing::error!("Failed to add plan results for query {}: {}", qid, e);
                    }
                }
                Ok(QueryResult::Analyze {
                    plan,
                    stats,
                    columns,
                    rows,
                    mutation_stats,
                    _memory_reservation,
                    ..
                }) => {
                    // 컬럼 정보를 먼저 설정
                    if let Err(e) = poll_manager.set_columns(&qid, columns).await {
                        tracing::error!("Failed to set columns for query {}: {}", qid, e);
                    }

                    let mut json_rows: Vec<serde_json::Value> =
                        rows.iter().map(|row| row_to_json(row)).collect();

                    // 실행 통계 추가
                    json_rows.push(serde_json::json!({
                        "type": "stats",
                        "plan": plan.to_tree_string(),
                        "planningTime": stats.planning_time.as_millis() as u64,
                        "executionTime": stats.execution_time.as_millis() as u64,
                        "totalRows": stats.total_rows
                    }));

                    // mutation 통계 추가
                    if mutation_stats.has_updates() {
                        json_rows.push(mutation_stats_to_json(&mutation_stats));
                    }

                    if let Err(e) = poll_manager
                        .add_results_with_memory(&qid, json_rows, _memory_reservation)
                        .await
                    {
                        tracing::error!("Failed to add analyze results for query {}: {}", qid, e);
                    }
                }
                Err(e) => {
                    // 에러 기록
                    if let Err(fail_err) = poll_manager.fail_query(&qid, e.to_string()).await {
                        tracing::error!(
                            "Failed to record query failure for {}: {} (original: {})",
                            qid,
                            fail_err,
                            e
                        );
                    }
                    return;
                }
            }

            // 쿼리 완료 표시
            if let Err(e) = poll_manager.complete_query(&qid).await {
                tracing::error!("Failed to mark query {} as complete: {}", qid, e);
            }
        });

        Ok(query_id)
    }

    /// 엔진 참조 반환
    pub fn engine(&self) -> &Arc<GraphEngine> {
        &self.engine
    }

    /// 백프레셔 참조 반환
    pub fn backpressure(&self) -> &SharedBackpressure {
        &self.backpressure
    }

    /// Poll 관리자 참조 반환
    pub fn poll_manager(&self) -> &SharedPollManager {
        &self.poll_manager
    }

    /// 설정 참조 반환
    pub fn config(&self) -> &StreamingConfig {
        &self.config
    }
}

/// 스트리밍 결과
pub struct StreamingResult {
    pub query_id: String,
    pub columns: Vec<String>,
    pub receiver: mpsc::Receiver<Result<serde_json::Value, StreamError>>,
    pub(crate) memory_reservation: SharedQueryMemory,
}

/// 취소 가능한 스트리밍 결과
pub struct CancellableStreamingResult {
    pub query_id: String,
    pub columns: Vec<String>,
    pub receiver: mpsc::Receiver<Result<serde_json::Value, StreamError>>,
    pub cancel_sender: mpsc::Sender<()>,
    pub(crate) _memory_reservation: SharedQueryMemory,
}

impl CancellableStreamingResult {
    /// 쿼리 취소
    pub async fn cancel(&self) {
        let _ = self.cancel_sender.send(()).await;
    }
}

/// 결과 행을 JSON으로 변환
fn row_to_json(row: &[zeta4j_graph::core::Value]) -> serde_json::Value {
    let values: Vec<serde_json::Value> = row.iter().map(value_to_json).collect();
    serde_json::Value::Array(values)
}

/// JSON을 Value로 변환
fn json_to_value(json: &serde_json::Value) -> zeta4j_graph::core::Value {
    match json {
        serde_json::Value::Null => zeta4j_graph::core::Value::Null,
        serde_json::Value::Bool(b) => zeta4j_graph::core::Value::Boolean(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                zeta4j_graph::core::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                zeta4j_graph::core::Value::Float(f)
            } else {
                zeta4j_graph::core::Value::Null
            }
        }
        serde_json::Value::String(s) => zeta4j_graph::core::Value::String(s.clone()),
        serde_json::Value::Array(arr) => {
            let items: Vec<_> = arr.iter().map(json_to_value).collect();
            zeta4j_graph::core::Value::Array(items)
        }
        serde_json::Value::Object(obj) => {
            let map: indexmap::IndexMap<String, zeta4j_graph::core::Value> = obj
                .iter()
                .map(|(k, v)| (k.clone(), json_to_value(v)))
                .collect();
            zeta4j_graph::core::Value::Map(map)
        }
    }
}

/// Value를 JSON으로 변환
fn value_to_json(value: &zeta4j_graph::core::Value) -> serde_json::Value {
    match value {
        zeta4j_graph::core::Value::Null => serde_json::Value::Null,
        zeta4j_graph::core::Value::Boolean(b) => serde_json::Value::Bool(*b),
        zeta4j_graph::core::Value::Integer(i) => serde_json::json!(*i),
        zeta4j_graph::core::Value::Float(f) => serde_json::json!(*f),
        zeta4j_graph::core::Value::String(s) => serde_json::Value::String(s.clone()),
        zeta4j_graph::core::Value::Array(list) => {
            let items: Vec<_> = list.iter().map(value_to_json).collect();
            serde_json::Value::Array(items)
        }
        zeta4j_graph::core::Value::Map(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect();
            serde_json::Value::Object(obj)
        }
        zeta4j_graph::core::Value::Node(node_id) => {
            serde_json::json!({
                "_type": "node",
                "id": node_id
            })
        }
        zeta4j_graph::core::Value::Edge(edge_id) => {
            serde_json::json!({
                "_type": "edge",
                "id": edge_id
            })
        }
        zeta4j_graph::core::Value::Path(path) => {
            serde_json::json!({
                "_type": "path",
                "nodes": path.nodes,
                "edges": path.edges
            })
        }
        zeta4j_graph::core::Value::Duration(d) => {
            serde_json::json!({
                "_type": "duration",
                "months": d.months,
                "days": d.days,
                "seconds": d.seconds,
                "nanoseconds": d.nanoseconds
            })
        }
        zeta4j_graph::core::Value::Date(d) => {
            serde_json::json!({
                "_type": "date",
                "value": d.to_string()
            })
        }
        zeta4j_graph::core::Value::Time(t) => {
            serde_json::json!({
                "_type": "time",
                "value": t.to_string()
            })
        }
        zeta4j_graph::core::Value::DateTime(dt) => {
            serde_json::json!({
                "_type": "datetime",
                "value": dt.to_rfc3339()
            })
        }
        zeta4j_graph::core::Value::LocalDateTime(ldt) => {
            serde_json::json!({
                "_type": "localdatetime",
                "value": ldt.to_string()
            })
        }
        zeta4j_graph::core::Value::Vector(v) => {
            serde_json::json!({
                "_type": "vector",
                "dimensions": v.dimensions(),
                "data": v.as_slice()
            })
        }
    }
}

/// 공유 스트리밍 실행기
pub type SharedStreamingExecutor = Arc<StreamingExecutor>;

/// MutationStats → JSON 변환
fn mutation_stats_to_json(stats: &MutationStats) -> serde_json::Value {
    serde_json::json!({
        "type": "mutation_stats",
        "nodes_created": stats.nodes_created,
        "nodes_deleted": stats.nodes_deleted,
        "relationships_created": stats.relationships_created,
        "relationships_deleted": stats.relationships_deleted,
        "properties_set": stats.properties_set,
        "labels_added": stats.labels_added,
        "labels_removed": stats.labels_removed,
        "indexes_added": stats.indexes_added,
        "indexes_removed": stats.indexes_removed,
        "constraints_added": stats.constraints_added,
        "constraints_removed": stats.constraints_removed,
        "contains_updates": stats.has_updates()
    })
}

/// 스트리밍 실행기 생성 헬퍼
pub fn create_streaming_executor(
    engine: Arc<GraphEngine>,
    config: StreamingConfig,
) -> SharedStreamingExecutor {
    Arc::new(StreamingExecutor::new(engine, config))
}
