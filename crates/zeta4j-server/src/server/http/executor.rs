// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! HTTP Query Executor
//!
//! Cypher 쿼리 실행 및 결과 변환
//!
//! # 비동기 지원 (v1.77.0, updated v1.79.0)
//!
//! v1.79.0: QueryExecutor의 execute 메서드들이 async로 변경되어
//! spawn_blocking 없이 직접 비동기 호출합니다.
//!
//! ```ignore
//! // 비동기 쿼리 실행
//! let result = executor.execute_statement_async(&statement).await?;
//! ```

use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::Arc;

use super::cypher_types::{
    CypherError, GraphData, NodeData, QueryStats, RelationshipData, ResultDataContent, ResultRow,
    Statement, StatementResult,
};

use crate::GraphError;
use zeta4j_common::codes;
use zeta4j_common::error::ZetaError;
use zeta4j_cypher_executor::parser::split_statements;
use zeta4j_cypher_executor::procedures::{FulltextSearchProvider, VectorSearchProvider};
use zeta4j_cypher_executor::{
    AdminService, MutationStats, QueryExecutor, QueryResult, parse_query,
};
use zeta4j_graph::core::PropertyContainer;
use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;

fn cypher_error_to_zeta(error: CypherError) -> ZetaError {
    if error.code == codes::graph::MEMORY_ADMISSION_DENIED.to_string() {
        return ZetaError::memory_admission(error.message);
    }
    let code = match error.code.as_str() {
        "CY-ADMIN-001" => Some(codes::cypher::ADMIN_SERVICE_UNAVAILABLE),
        "CY-ADMIN-002" => Some(codes::cypher::ADMIN_UNSUPPORTED),
        "CY-ADMIN-003" => Some(codes::cypher::ADMIN_PRINCIPAL_REQUIRED),
        "CY-ADMIN-004" => Some(codes::cypher::ADMIN_STATE_INVARIANT),
        _ => None,
    };
    if let Some(stable_code) = code {
        ZetaError::query_execution_with_code(stable_code, error.message)
    } else {
        ZetaError::query_execution(error.message)
    }
}

// ============================================================================
// Execution Config (Phase 1: 쿼리 실행 함수 통합)
// ============================================================================

/// 쿼리 실행 설정
///
/// 여러 쿼리 실행 함수의 매개변수를 통합한 구조체입니다.
/// 빌더 패턴을 사용하여 필요한 옵션만 설정할 수 있습니다.
#[derive(Default, Clone)]
pub struct ExecutionConfig {
    /// 최대 결과 크기 (0 = 무제한)
    pub max_result_size: usize,
    /// 쿼리 타임아웃 (밀리초, 0 = 무제한)
    pub timeout_ms: u64,
    /// Fulltext 검색 Provider
    pub fulltext_provider: Option<Arc<dyn FulltextSearchProvider>>,
    /// Vector search Provider.
    pub vector_provider: Option<Arc<dyn VectorSearchProvider>>,
    /// Canonical admin service.
    pub admin_service: Option<Arc<dyn AdminService>>,
    /// Authenticated principal.
    pub current_user: Option<String>,
}

impl ExecutionConfig {
    /// 새 설정 생성 (기본값)
    pub fn new() -> Self {
        Self::default()
    }

    /// 쿼리 제한 설정
    pub fn with_query_limits(mut self, max_result_size: usize, timeout_ms: u64) -> Self {
        self.max_result_size = max_result_size;
        self.timeout_ms = timeout_ms;
        self
    }

    /// Fulltext Provider 설정
    pub fn with_fulltext(mut self, provider: Arc<dyn FulltextSearchProvider>) -> Self {
        self.fulltext_provider = Some(provider);
        self
    }

    pub fn with_vector(mut self, provider: Arc<dyn VectorSearchProvider>) -> Self {
        self.vector_provider = Some(provider);
        self
    }

    pub fn with_admin_service(mut self, service: Arc<dyn AdminService>) -> Self {
        self.admin_service = Some(service);
        self
    }

    pub fn with_current_user(mut self, username: impl Into<String>) -> Self {
        self.current_user = Some(username.into());
        self
    }
}

/// 설정 기반 쿼리 실행 (통합 함수)
///
/// 기존 6개의 execute_*_with_* 함수를 통합한 단일 진입점입니다.
///
/// # Example
///
/// ```ignore
/// let config = ExecutionConfig::new()
///     .with_query_limits(1000, 30000);
///
/// let results = execute_with_config(engine, &statements, config).await?;
/// ```
pub async fn execute_with_config(
    engine: Arc<GraphEngine>,
    statements: &[Statement],
    config: ExecutionConfig,
) -> Result<Vec<StatementResult>, ZetaError> {
    let mut executor =
        HttpQueryExecutor::new(engine).with_query_config(config.max_result_size, config.timeout_ms);

    if let Some(provider) = config.fulltext_provider {
        executor = executor.with_fulltext_provider(provider);
    }
    if let Some(provider) = config.vector_provider {
        executor = executor.with_vector_provider(provider);
    }
    if let Some(service) = config.admin_service {
        executor = executor.with_admin_service(service);
    }
    if let Some(username) = config.current_user {
        executor = executor.with_current_user(username);
    }

    executor
        .execute_statements(statements)
        .await
        .map_err(cypher_error_to_zeta)
}

/// 단일 문장 설정 기반 실행
#[allow(dead_code)]
pub async fn execute_statement_with_config(
    engine: Arc<GraphEngine>,
    statement: &Statement,
    config: ExecutionConfig,
) -> Result<StatementResult, ZetaError> {
    let mut executor =
        HttpQueryExecutor::new(engine).with_query_config(config.max_result_size, config.timeout_ms);

    if let Some(provider) = config.fulltext_provider {
        executor = executor.with_fulltext_provider(provider);
    }
    if let Some(provider) = config.vector_provider {
        executor = executor.with_vector_provider(provider);
    }
    if let Some(service) = config.admin_service {
        executor = executor.with_admin_service(service);
    }
    if let Some(username) = config.current_user {
        executor = executor.with_current_user(username);
    }

    executor
        .execute_statement(statement)
        .await
        .map_err(cypher_error_to_zeta)
}

// ============================================================================
// Transaction Execution Helpers (Phase 2)
// ============================================================================

use super::http_transaction::HttpTransactionManager;

/// 트랜잭션 내 쿼리 실행 및 통계 기록
///
/// 쿼리를 실행하고 실행 통계를 트랜잭션 매니저에 기록합니다.
/// 실패 시 트랜잭션 롤백은 호출자가 처리해야 합니다.
///
/// # Example
///
/// ```ignore
/// let config = ExecutionConfig::new()
///     .with_query_limits(1000, 30000);
///
/// let results = execute_and_record(
///     engine,
///     &tx_id,
///     &tx_manager,
///     &statements,
///     config,
/// ).await?;
/// ```
pub async fn execute_and_record(
    engine: Arc<GraphEngine>,
    tx_id: &str,
    tx_manager: &HttpTransactionManager,
    statements: &[Statement],
    config: ExecutionConfig,
) -> Result<Vec<StatementResult>, ZetaError> {
    let results = execute_with_config(engine, statements, config).await?;

    // 실행 통계 기록
    for r in &results {
        if let Some(ref stats) = r.stats
            && let Err(e) = tx_manager.record_execution(tx_id, stats)
        {
            tracing::warn!("Failed to record execution stats for tx {}: {}", tx_id, e);
        }
    }

    Ok(results)
}

/// 트랜잭션 롤백 후 에러 반환
///
/// 에러 발생 시 트랜잭션을 롤백하고 원래 에러를 반환합니다.
pub fn rollback_and_return_error(
    tx_manager: &HttpTransactionManager,
    tx_id: &str,
    error: ZetaError,
) -> ZetaError {
    if let Err(e) = tx_manager.rollback(tx_id) {
        tracing::error!(
            "Rollback failed for tx {}: {} (original error: {})",
            tx_id,
            e,
            error
        );
    }
    error
}

// ============================================================================
// Query Executor
// ============================================================================

/// HTTP 쿼리 실행기
pub struct HttpQueryExecutor {
    /// 현재 데이터베이스의 그래프 엔진
    engine: Arc<GraphEngine>,
    /// Fulltext 검색 Provider (#155)
    fulltext_provider: Option<Arc<dyn FulltextSearchProvider>>,
    vector_provider: Option<Arc<dyn VectorSearchProvider>>,
    admin_service: Option<Arc<dyn AdminService>>,
    current_user: Option<String>,
    /// 최대 결과 크기 (0 = 무제한)
    max_result_size: usize,
    /// 쿼리 타임아웃 (밀리초, 0 = 무제한)
    timeout_ms: u64,
}

impl HttpQueryExecutor {
    /// 새 실행기 생성
    pub fn new(engine: Arc<GraphEngine>) -> Self {
        Self {
            engine,
            fulltext_provider: None,
            vector_provider: None,
            admin_service: None,
            current_user: None,
            max_result_size: 0,
            timeout_ms: 0,
        }
    }

    /// 쿼리 설정 적용 (빌더 패턴)
    pub fn with_query_config(mut self, max_result_size: usize, timeout_ms: u64) -> Self {
        self.max_result_size = max_result_size;
        self.timeout_ms = timeout_ms;
        self
    }

    /// Fulltext 검색 Provider 설정 (빌더 패턴)
    ///
    /// CALL db.index.fulltext.* 프로시저에서 사용됩니다.
    pub fn with_fulltext_provider(mut self, provider: Arc<dyn FulltextSearchProvider>) -> Self {
        self.fulltext_provider = Some(provider);
        self
    }

    pub fn with_vector_provider(mut self, provider: Arc<dyn VectorSearchProvider>) -> Self {
        self.vector_provider = Some(provider);
        self
    }

    pub fn with_admin_service(mut self, service: Arc<dyn AdminService>) -> Self {
        self.admin_service = Some(service);
        self
    }

    pub fn with_current_user(mut self, username: impl Into<String>) -> Self {
        self.current_user = Some(username.into());
        self
    }

    /// 기본 엔진 반환
    pub fn engine(&self) -> Arc<GraphEngine> {
        self.engine.clone()
    }

    /// 단일 문장 실행
    ///
    /// 세미콜론으로 구분된 여러 쿼리를 순차적으로 실행하고 마지막 결과를 반환합니다.
    ///
    /// v1.79.0: 비동기 메서드로 변경 (DatabaseManager async 지원)
    /// v1.80.0: 세미콜론 구분 다중 쿼리 지원 (#156)
    pub async fn execute_statement(
        &self,
        statement: &Statement,
    ) -> Result<StatementResult, CypherError> {
        // 세미콜론으로 쿼리 분할
        let queries = split_statements(&statement.statement);

        // 단일 쿼리인 경우 기존 로직
        if queries.len() <= 1 {
            return self
                .execute_single_query(
                    queries.first().copied().unwrap_or(&statement.statement),
                    &statement.parameters,
                    &statement.result_data_contents,
                    statement.include_stats,
                )
                .await;
        }

        // 다중 쿼리: 순차 실행, 마지막 결과 반환
        let mut last_result = None;
        for query_str in queries {
            last_result = Some(
                self.execute_single_query(
                    query_str,
                    &statement.parameters,
                    &statement.result_data_contents,
                    statement.include_stats,
                )
                .await?,
            );
        }

        last_result.ok_or_else(|| CypherError::syntax_error("Empty query"))
    }

    /// 단일 쿼리 실행 (내부 헬퍼)
    async fn execute_single_query(
        &self,
        query_str: &str,
        parameters: &HashMap<String, serde_json::Value>,
        result_data_contents: &[ResultDataContent],
        include_stats: bool,
    ) -> Result<StatementResult, CypherError> {
        // 1. 쿼리 파싱
        let (_, query) =
            parse_query(query_str).map_err(|e| CypherError::syntax_error(format!("{:?}", e)))?;

        // 2. 쿼리 실행기 생성 (query 설정 적용)
        let mut executor = QueryExecutor::new(self.engine.clone())
            .with_query_config(self.max_result_size, self.timeout_ms);

        // 4. Fulltext Provider 연결 (#155)
        if let Some(provider) = &self.fulltext_provider {
            executor = executor.with_fulltext_provider(provider.clone());
        }
        if let Some(provider) = &self.vector_provider {
            executor = executor.with_vector_provider(provider.clone());
        }
        if let Some(service) = &self.admin_service {
            executor = executor.with_admin_service(service.clone());
        }
        if let Some(username) = &self.current_user {
            executor = executor.with_current_user(username.clone());
        }

        // 5. 파라미터 변환
        let params = self.convert_parameters(parameters);

        // 6. 쿼리 실행 (파라미터 주입)
        let result = executor
            .execute_with_result_and_params(query, params)
            .await
            .map_err(Self::convert_error)?;

        // 7. 결과 변환
        self.convert_result(result, result_data_contents, include_stats)
            .await
    }

    /// 여러 문장 실행 (auto-commit)
    ///
    /// v1.79.0: 비동기 메서드로 변경 (DatabaseManager async 지원)
    pub async fn execute_statements(
        &self,
        statements: &[Statement],
    ) -> Result<Vec<StatementResult>, CypherError> {
        let mut results = Vec::with_capacity(statements.len());

        for statement in statements {
            match self.execute_statement(statement).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    // 에러 발생 시 즉시 반환
                    return Err(e);
                }
            }
        }

        Ok(results)
    }

    // ========================================================================
    // Async Methods (v1.77.0, updated v1.79.0)
    // v1.79.0: execute_statement/execute_statements가 이제 async이므로
    //          spawn_blocking_query 대신 직접 호출
    // ========================================================================

    /// 단일 문장 비동기 실행
    ///
    /// v1.79.0: execute_statement가 async로 변경되어 직접 호출
    pub async fn execute_statement_async(
        &self,
        statement: &Statement,
    ) -> Result<StatementResult, CypherError> {
        self.execute_statement(statement).await
    }

    /// 지정된 엔진으로 단일 문장 비동기 실행
    pub async fn execute_statement_with_engine(
        engine: Arc<GraphEngine>,
        statement: &Statement,
    ) -> Result<StatementResult, CypherError> {
        Self::execute_statement_with_engine_and_config(engine, statement, 0, 0).await
    }

    /// 지정된 엔진과 쿼리 설정으로 단일 문장 비동기 실행
    pub async fn execute_statement_with_engine_and_config(
        engine: Arc<GraphEngine>,
        statement: &Statement,
        max_result_size: usize,
        timeout_ms: u64,
    ) -> Result<StatementResult, CypherError> {
        let executor =
            HttpQueryExecutor::new(engine).with_query_config(max_result_size, timeout_ms);
        executor.execute_statement(statement).await
    }

    /// 여러 문장 비동기 실행 (auto-commit)
    ///
    /// v1.79.0: execute_statements가 async로 변경되어 직접 호출
    pub async fn execute_statements_async(
        &self,
        statements: &[Statement],
    ) -> Result<Vec<StatementResult>, CypherError> {
        self.execute_statements(statements).await
    }

    /// 지정된 엔진으로 여러 문장 비동기 실행
    ///
    /// USE 문이나 URL path의 db_name으로 다른 데이터베이스 엔진을 사용할 때 호출
    pub async fn execute_statements_with_engine(
        engine: Arc<GraphEngine>,
        statements: &[Statement],
    ) -> Result<Vec<StatementResult>, CypherError> {
        Self::execute_statements_with_engine_and_config(engine, statements, 0, 0).await
    }

    /// 지정된 엔진과 쿼리 설정으로 여러 문장 비동기 실행
    pub async fn execute_statements_with_engine_and_config(
        engine: Arc<GraphEngine>,
        statements: &[Statement],
        max_result_size: usize,
        timeout_ms: u64,
    ) -> Result<Vec<StatementResult>, CypherError> {
        let executor =
            HttpQueryExecutor::new(engine).with_query_config(max_result_size, timeout_ms);
        executor.execute_statements(statements).await
    }

    /// 모든 프로바이더를 포함하여 여러 문장 비동기 실행 (#155)
    ///
    /// Fulltext/Vector Provider와 admin service를 모두 연결합니다.
    pub async fn execute_statements_with_all_providers(
        engine: Arc<GraphEngine>,
        fulltext_provider: Option<Arc<dyn FulltextSearchProvider>>,
        vector_provider: Option<Arc<dyn VectorSearchProvider>>,
        admin_service: Option<Arc<dyn AdminService>>,
        current_user: Option<String>,
        statements: &[Statement],
        max_result_size: usize,
        timeout_ms: u64,
    ) -> Result<Vec<StatementResult>, CypherError> {
        let mut executor =
            HttpQueryExecutor::new(engine).with_query_config(max_result_size, timeout_ms);

        if let Some(provider) = fulltext_provider {
            executor = executor.with_fulltext_provider(provider);
        }
        if let Some(provider) = vector_provider {
            executor = executor.with_vector_provider(provider);
        }
        if let Some(service) = admin_service {
            executor = executor.with_admin_service(service);
        }
        if let Some(username) = current_user {
            executor = executor.with_current_user(username);
        }

        executor.execute_statements(statements).await
    }

    /// JSON 파라미터를 Value로 변환
    fn convert_parameters(
        &self,
        params: &HashMap<String, serde_json::Value>,
    ) -> IndexMap<String, Value> {
        params
            .iter()
            .map(|(k, v)| (k.clone(), Self::json_to_value(v)))
            .collect()
    }

    /// JSON Value를 core::Value로 변환
    fn json_to_value(json: &serde_json::Value) -> Value {
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
                Value::Array(arr.iter().map(Self::json_to_value).collect())
            }
            serde_json::Value::Object(map) => {
                let props: IndexMap<String, Value> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::json_to_value(v)))
                    .collect();
                Value::Map(props)
            }
        }
    }

    /// core::Value를 JSON Value로 변환
    #[async_recursion::async_recursion]
    async fn value_to_json(&self, value: &Value) -> serde_json::Value {
        match value {
            Value::Null => serde_json::Value::Null,
            Value::Boolean(b) => serde_json::Value::Bool(*b),
            Value::Integer(i) => serde_json::json!(*i),
            Value::Float(f) => serde_json::json!(*f),
            Value::String(s) => serde_json::Value::String(s.clone()),
            Value::Array(arr) => {
                let mut result = Vec::with_capacity(arr.len());
                for v in arr {
                    result.push(self.value_to_json(v).await);
                }
                serde_json::Value::Array(result)
            }
            Value::Map(map) => {
                let mut obj = serde_json::Map::new();
                for (k, v) in map.iter() {
                    obj.insert(k.clone(), self.value_to_json(v).await);
                }
                serde_json::Value::Object(obj)
            }
            Value::Node(node_id) => {
                // Neo4j 표준: row에는 프로퍼티만 포함 (메타는 meta 필드에)
                match self.engine.get_node(*node_id).await {
                    Ok(Some(node)) => {
                        let mut obj = serde_json::Map::new();
                        for (k, v) in node.properties().iter() {
                            obj.insert(k.clone(), self.value_to_json(v).await);
                        }
                        serde_json::Value::Object(obj)
                    }
                    Ok(None) => {
                        tracing::debug!("Node {} not found during serialization", node_id);
                        serde_json::Value::Object(serde_json::Map::new())
                    }
                    Err(e) => {
                        tracing::warn!("Failed to retrieve node {}: {}", node_id, e);
                        serde_json::Value::Object(serde_json::Map::new())
                    }
                }
            }
            Value::Edge(edge_id) => {
                // Neo4j 표준: row에는 프로퍼티만 포함 (메타는 meta 필드에)
                match self.engine.get_edge(*edge_id).await {
                    Ok(Some(edge)) => {
                        let mut obj = serde_json::Map::new();
                        for (k, v) in edge.properties().iter() {
                            obj.insert(k.clone(), self.value_to_json(v).await);
                        }
                        serde_json::Value::Object(obj)
                    }
                    Ok(None) => {
                        tracing::debug!("Edge {} not found during serialization", edge_id);
                        serde_json::Value::Object(serde_json::Map::new())
                    }
                    Err(e) => {
                        tracing::warn!("Failed to retrieve edge {}: {}", edge_id, e);
                        serde_json::Value::Object(serde_json::Map::new())
                    }
                }
            }
            Value::Path(path) => {
                // Neo4j 표준: 노드-관계 교대 배열 [n0, r0, n1, r1, n2, ...]
                let mut elements = Vec::with_capacity(path.nodes.len() + path.edges.len());
                for (i, node_id) in path.nodes.iter().enumerate() {
                    elements.push(self.value_to_json(&Value::Node(*node_id)).await);
                    if i < path.edges.len() {
                        elements.push(self.value_to_json(&Value::Edge(path.edges[i])).await);
                    }
                }
                serde_json::Value::Array(elements)
            }
            Value::Date(d) => serde_json::Value::String(d.to_string()),
            Value::Time(t) => serde_json::Value::String(t.to_string()),
            Value::DateTime(dt) => serde_json::Value::String(dt.to_rfc3339()),
            Value::LocalDateTime(ldt) => serde_json::Value::String(ldt.to_string()),
            Value::Duration(d) => serde_json::json!({
                "months": d.months,
                "days": d.days,
                "seconds": d.seconds,
                "nanoseconds": d.nanoseconds,
            }),
            Value::Vector(v) => serde_json::json!({
                "type": "vector",
                "dimensions": v.dimensions(),
                "data": v.as_slice()
            }),
        }
    }

    /// 실행 결과를 StatementResult로 변환
    async fn convert_result(
        &self,
        result: QueryResult,
        formats: &[ResultDataContent],
        include_stats: bool,
    ) -> Result<StatementResult, CypherError> {
        match result {
            QueryResult::Rows {
                columns,
                rows,
                mutation_stats,
                _memory_reservation: memory_reservation,
            } => {
                let result = self
                    .convert_rows(columns, rows, formats, include_stats, mutation_stats)
                    .await?;
                Ok(result.with_memory_reservation(memory_reservation))
            }
            QueryResult::Plan(plan) => {
                // EXPLAIN 결과: 실제 쿼리 플랜 반환
                let mut result = StatementResult::new(vec!["Plan".to_string()]);
                result = result.with_row(ResultRow::row(vec![serde_json::json!(
                    plan.to_tree_string()
                )]));
                Ok(result)
            }
            QueryResult::Analyze {
                plan,
                stats,
                columns,
                rows,
                mutation_stats,
                _memory_reservation: memory_reservation,
            } => {
                // EXPLAIN ANALYZE 결과: 행 데이터 + 실행 통계
                let mut result = self
                    .convert_rows(columns, rows, formats, include_stats, mutation_stats)
                    .await?;
                // 실행 프로파일링 정보 추가
                let operators: Vec<serde_json::Value> = stats
                    .operators
                    .iter()
                    .map(|op| {
                        serde_json::json!({
                            "operator": op.operator,
                            "description": op.description,
                            "estimatedRows": op.estimated_rows,
                            "actualRows": op.actual_rows,
                            "actualTimeMs": op.actual_time_ms(),
                            "loops": op.loops
                        })
                    })
                    .collect();
                result = result.with_row(ResultRow::row(vec![serde_json::json!({
                    "plan": plan.to_tree_string(),
                    "planningTime": stats.planning_time.as_millis() as u64,
                    "executionTime": stats.execution_time.as_millis() as u64,
                    "totalRows": stats.total_rows,
                    "operators": operators
                })]));
                Ok(result.with_memory_reservation(memory_reservation))
            }
        }
    }

    /// 행 결과 변환
    async fn convert_rows(
        &self,
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
        formats: &[ResultDataContent],
        include_stats: bool,
        mutation_stats: MutationStats,
    ) -> Result<StatementResult, CypherError> {
        // 추론된 컬럼 이름 사용 (빈 경우 기본 이름 생성)
        let columns = if columns.is_empty() && !rows.is_empty() {
            // 컬럼 이름이 없고 결과가 있는 경우 기본 이름 사용
            (0..rows[0].len())
                .map(|i| format!("column_{}", i))
                .collect()
        } else {
            columns
        };

        let mut statement_result = StatementResult::new(columns);

        // 포맷 결정 (기본: Row)
        let use_row = formats.is_empty() || formats.contains(&ResultDataContent::Row);
        let use_graph = formats.contains(&ResultDataContent::Graph);

        // 각 행 변환
        for row in &rows {
            let result_row = self.convert_row(row, use_row, use_graph).await;
            statement_result = statement_result.with_row(result_row);
        }

        // 통계 추가
        if include_stats {
            let stats = QueryStats::from(&mutation_stats);
            statement_result = statement_result.with_stats(stats);
        }

        Ok(statement_result)
    }

    /// 단일 행 변환
    async fn convert_row(&self, row: &[Value], use_row: bool, use_graph: bool) -> ResultRow {
        let row_data = if use_row {
            let mut result = Vec::with_capacity(row.len());
            for v in row {
                result.push(self.value_to_json(v).await);
            }
            Some(result)
        } else {
            None
        };

        // Neo4j 호환: meta 필드 생성 (각 값에 대해 null, 배열 요소도 개별 카운트)
        let meta_data = if use_row {
            Some(self.generate_meta_for_row(row))
        } else {
            None
        };

        let graph_data = if use_graph {
            Some(self.extract_graph_data(row).await)
        } else {
            None
        };

        ResultRow {
            row: row_data,
            meta: meta_data,
            graph: graph_data,
        }
    }

    /// Neo4j 호환 meta 필드 생성
    ///
    /// 배열 요소를 재귀적으로 카운트하여 null 생성
    /// 참고: Neo4j는 저장된 프로퍼티 배열과 계산된 배열을 다르게 처리하지만,
    /// 이 구분은 HTTP 응답 레벨에서 불가능. 대부분의 케이스에서 동작하는 재귀 방식 사용.
    fn generate_meta_for_row(&self, row: &[Value]) -> Vec<serde_json::Value> {
        let mut meta = Vec::new();
        for value in row {
            self.count_meta_for_value(value, &mut meta);
        }
        meta
    }

    /// Neo4j 호환 메타 데이터 생성
    /// - 스칼라 값: null
    /// - 노드: {id, elementId, type: "node", deleted: false}
    /// - 관계: {id, elementId, type: "relationship", deleted: false}
    /// - 배열: 각 요소에 대해 재귀적으로 메타 생성
    fn count_meta_for_value(&self, value: &Value, meta: &mut Vec<serde_json::Value>) {
        match value {
            Value::Array(arr) => {
                for item in arr {
                    self.count_meta_for_value(item, meta);
                }
            }
            Value::Node(node_id) => {
                // Neo4j 노드 메타 형식
                meta.push(serde_json::json!({
                    "id": *node_id as i64,
                    "elementId": node_id.to_string(),
                    "type": "node",
                    "deleted": false
                }));
            }
            Value::Edge(edge_id) => {
                // Neo4j 관계 메타 형식
                meta.push(serde_json::json!({
                    "id": *edge_id as i64,
                    "elementId": edge_id.to_string(),
                    "type": "relationship",
                    "deleted": false
                }));
            }
            Value::Path(path) => {
                // 경로의 노드와 엣지 각각에 대해 메타 생성
                for (i, node_id) in path.nodes.iter().enumerate() {
                    meta.push(serde_json::json!({
                        "id": *node_id as i64,
                        "elementId": node_id.to_string(),
                        "type": "node",
                        "deleted": false
                    }));
                    // 노드 사이에 엣지가 있으면 추가
                    if i < path.edges.len() {
                        let edge_id = path.edges[i];
                        meta.push(serde_json::json!({
                            "id": edge_id as i64,
                            "elementId": edge_id.to_string(),
                            "type": "relationship",
                            "deleted": false
                        }));
                    }
                }
            }
            _ => meta.push(serde_json::Value::Null),
        }
    }

    /// 행에서 그래프 데이터 추출
    async fn extract_graph_data(&self, row: &[Value]) -> GraphData {
        let mut nodes = Vec::new();
        let mut relationships = Vec::new();
        let mut seen_node_ids = std::collections::HashSet::new();
        let mut seen_edge_ids = std::collections::HashSet::new();

        for value in row {
            self.collect_graph_elements(
                value,
                &mut nodes,
                &mut relationships,
                &mut seen_node_ids,
                &mut seen_edge_ids,
            )
            .await;
        }

        GraphData {
            nodes,
            relationships,
        }
    }

    /// 값에서 그래프 요소 수집
    #[async_recursion::async_recursion]
    async fn collect_graph_elements(
        &self,
        value: &Value,
        nodes: &mut Vec<NodeData>,
        relationships: &mut Vec<RelationshipData>,
        seen_nodes: &mut std::collections::HashSet<u64>,
        seen_edges: &mut std::collections::HashSet<u64>,
    ) {
        match value {
            Value::Node(node_id) => {
                if !seen_nodes.contains(node_id) {
                    seen_nodes.insert(*node_id);
                    match self.engine.get_node(*node_id).await {
                        Ok(Some(node)) => nodes.push(self.node_to_data(&node).await),
                        Ok(None) => {
                            tracing::debug!("Node {} not found during graph extraction", node_id)
                        }
                        Err(e) => tracing::warn!(
                            "Failed to retrieve node {} for graph data: {}",
                            node_id,
                            e
                        ),
                    }
                }
            }
            Value::Edge(edge_id) => {
                if !seen_edges.contains(edge_id) {
                    seen_edges.insert(*edge_id);
                    match self.engine.get_edge(*edge_id).await {
                        Ok(Some(edge)) => relationships.push(self.edge_to_data(&edge).await),
                        Ok(None) => {
                            tracing::debug!("Edge {} not found during graph extraction", edge_id)
                        }
                        Err(e) => tracing::warn!(
                            "Failed to retrieve edge {} for graph data: {}",
                            edge_id,
                            e
                        ),
                    }
                }
            }
            Value::Path(path) => {
                for node_id in &path.nodes {
                    if !seen_nodes.contains(node_id) {
                        seen_nodes.insert(*node_id);
                        match self.engine.get_node(*node_id).await {
                            Ok(Some(node)) => nodes.push(self.node_to_data(&node).await),
                            Ok(None) => tracing::debug!(
                                "Node {} not found during graph extraction",
                                node_id
                            ),
                            Err(e) => tracing::warn!(
                                "Failed to retrieve node {} for graph data: {}",
                                node_id,
                                e
                            ),
                        }
                    }
                }
                for edge_id in &path.edges {
                    if !seen_edges.contains(edge_id) {
                        seen_edges.insert(*edge_id);
                        match self.engine.get_edge(*edge_id).await {
                            Ok(Some(edge)) => relationships.push(self.edge_to_data(&edge).await),
                            Ok(None) => tracing::debug!(
                                "Edge {} not found during graph extraction",
                                edge_id
                            ),
                            Err(e) => tracing::warn!(
                                "Failed to retrieve edge {} for graph data: {}",
                                edge_id,
                                e
                            ),
                        }
                    }
                }
            }
            Value::Array(list) => {
                for item in list {
                    self.collect_graph_elements(item, nodes, relationships, seen_nodes, seen_edges)
                        .await;
                }
            }
            Value::Map(map) => {
                for v in map.values() {
                    self.collect_graph_elements(v, nodes, relationships, seen_nodes, seen_edges)
                        .await;
                }
            }
            _ => {}
        }
    }

    /// Node를 NodeData로 변환
    async fn node_to_data(&self, node: &zeta4j_graph::core::Node) -> NodeData {
        let mut properties = HashMap::new();
        for (k, v) in node.properties().iter() {
            properties.insert(k.clone(), self.value_to_json(v).await);
        }

        NodeData::new(
            node.id(),
            node.labels().iter().cloned().collect(),
            properties,
        )
    }

    /// Edge를 RelationshipData로 변환
    async fn edge_to_data(&self, edge: &zeta4j_graph::core::Edge) -> RelationshipData {
        let mut properties = HashMap::new();
        for (k, v) in edge.properties().iter() {
            properties.insert(k.clone(), self.value_to_json(v).await);
        }

        RelationshipData::new(
            edge.id(),
            edge.relation_type().to_string(),
            edge.from_node(),
            edge.to_node(),
            properties,
        )
    }

    /// 에러 변환
    fn convert_error(error: GraphError) -> CypherError {
        if matches!(
            error,
            GraphError::Transaction(_)
                | GraphError::Lock { .. }
                | GraphError::Concurrency { .. }
                | GraphError::Memory(_)
        ) {
            let zeta_error: ZetaError = error.into();
            return CypherError::new(zeta_error.code().to_string(), zeta_error.to_string());
        }

        match error {
            GraphError::Parse { message } => CypherError::syntax_error(message),
            GraphError::Validation { message } => CypherError::semantic_error(message),
            GraphError::NodeNotFound { id } => CypherError::new(
                "Zeta.ClientError.Statement.EntityNotFound",
                format!("Node with id {} not found", id),
            ),
            GraphError::EdgeNotFound { id } => CypherError::new(
                "Zeta.ClientError.Statement.EntityNotFound",
                format!("Edge with id {} not found", id),
            ),
            GraphError::Constraint {
                constraint_name,
                message,
            } => CypherError::new(
                "Zeta.ClientError.Schema.ConstraintValidationFailed",
                format!("{}: {}", constraint_name, message),
            ),
            GraphError::AccessDenied { message } => {
                CypherError::new("Zeta.ClientError.Security.Unauthorized", message)
            }
            GraphError::NotImplemented { feature } => CypherError::new(
                "Zeta.ClientError.Statement.NotImplemented",
                format!("Not implemented: {}", feature),
            ),
            GraphError::Schema(message) => {
                CypherError::new("Zeta.ClientError.Schema.SchemaError", message)
            }
            GraphError::BufferPoolExhausted => CypherError::new(
                "Zeta.TransientError.General.MemoryExhausted",
                "Buffer pool exhausted, please retry",
            ),
            GraphError::Zeta(ref zeta_err) => {
                CypherError::new(zeta_err.code().to_string(), format!("{}", error))
            }
            _ => CypherError::internal_error(format!("{}", error)),
        }
    }
}

#[cfg(test)]
mod transaction_error_tests {
    use super::*;
    use zeta4j_graph::transaction::TransactionError;

    #[test]
    fn http_preserves_transaction_lock_codes() {
        let timeout = HttpQueryExecutor::convert_error(GraphError::Transaction(
            TransactionError::LockTimeout(250),
        ));
        assert_eq!(timeout.code, codes::transaction::LOCK_TIMEOUT.to_string());

        let deadlock = HttpQueryExecutor::convert_error(GraphError::Transaction(
            TransactionError::DeadlockDetected(vec![1, 2, 1]),
        ));
        assert_eq!(deadlock.code, codes::transaction::DEADLOCK.to_string());
    }

    #[test]
    fn explicit_transaction_conversion_preserves_memory_admission() {
        let converted = cypher_error_to_zeta(CypherError::new(
            codes::graph::MEMORY_ADMISSION_DENIED.to_string(),
            "limit reached",
        ));
        assert_eq!(converted.code(), &codes::graph::MEMORY_ADMISSION_DENIED);
        assert_eq!(converted.http_status(), 503);
    }
}

// ============================================================================
// Tests
// ============================================================================
