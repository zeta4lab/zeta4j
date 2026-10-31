// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Bolt Query Executor implementation.
//!
//! Bridges the Bolt protocol AsyncQueryExecutor trait with the GraphEngine.
//!
//! v1.79.0: Changed to async implementation (block_on removed).

use indexmap::IndexMap;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::server::bolt::connection::handler::{
    AsyncQueryExecutor, ExecutionPrincipal, QueryResult,
};
use crate::server::bolt::message::QueryStats;
use crate::server::bolt::packstream::PackStreamValue;
use crate::server::bolt::packstream::structures::{
    PackStreamNode, PackStreamPath, PackStreamRelationship, PackStreamUnboundRelationship,
};
use crate::server::bolt::{BoltError, BoltResult};
use crate::server::tenant::TenantDatabaseManager;
use zeta4j_cypher_executor::AdminService;
use zeta4j_cypher_executor::ast::DatabaseTarget;
use zeta4j_cypher_executor::executor::QueryExecutor as CypherQueryExecutor;
use zeta4j_cypher_executor::parser::{parse_query_safe, parse_use, split_statements};
use zeta4j_cypher_executor::planner::QueryPlanner;
use zeta4j_cypher_executor::procedures::{FulltextSearchProvider, VectorSearchProvider};
use zeta4j_graph::core::PropertyContainer;
use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;
use zeta4j_graph::transaction::TransactionId;

/// Bolt 트랜잭션 상태 (GraphEngine 트랜잭션과 연동)
///
/// #169: Bolt 프로토콜의 BEGIN/COMMIT/ROLLBACK이 GraphEngine 트랜잭션과 연동됩니다.
struct BoltTransaction {
    /// Bolt 세션용 트랜잭션 ID (클라이언트에게 반환되는 ID)
    #[allow(dead_code)]
    bolt_tx_id: u64,
    /// GraphEngine 트랜잭션 ID (실제 데이터베이스 트랜잭션)
    engine_tx_id: TransactionId,
    /// 데이터베이스 이름
    database: String,
    /// 해당 데이터베이스의 GraphEngine 참조
    engine: Arc<GraphEngine>,
}

/// Bolt query executor that wraps GraphEngine.
pub struct BoltQueryExecutor {
    /// 기본 그래프 엔진 (단일 DB 모드용)
    engine: Arc<GraphEngine>,
    /// 테넌트 데이터베이스 관리자 (멀티 DB 모드용)
    db_manager: Option<Arc<TenantDatabaseManager>>,
    /// canonical admin service
    admin_service: Option<Arc<dyn AdminService>>,
    /// Fulltext 검색 Provider (#155)
    fulltext_provider: Option<Arc<dyn FulltextSearchProvider>>,
    vector_provider: Option<Arc<dyn VectorSearchProvider>>,
    /// 공유 인덱스 캐시 (L2 영속화 지원)
    shared_index_cache: Option<Arc<zeta4j_cache::IndexCache>>,
    /// Query planner
    planner: RwLock<QueryPlanner>,
    /// Active Bolt transactions (GraphEngine 트랜잭션과 연동)
    transactions: RwLock<HashMap<u64, BoltTransaction>>,
    /// Transaction ID counter
    next_tx_id: AtomicU64,
    /// 최대 결과 크기 (0 = 무제한)
    max_result_size: usize,
    /// 쿼리 타임아웃 (밀리초, 0 = 무제한)
    timeout_ms: u64,
    /// 쿼리 플랜 캐시 (파싱 + 플래닝 결과 캐싱)
    query_cache: Option<Arc<zeta4j_cypher_executor::cache::ConcurrentQueryCacheManager>>,
}

impl BoltQueryExecutor {
    /// Create a new Bolt query executor (단일 DB 모드).
    pub fn new(engine: Arc<GraphEngine>) -> Self {
        Self {
            engine,
            db_manager: None,
            admin_service: None,
            fulltext_provider: None,
            vector_provider: None,
            shared_index_cache: None,
            planner: RwLock::new(QueryPlanner::new()),
            transactions: RwLock::new(HashMap::new()),
            next_tx_id: AtomicU64::new(1),
            max_result_size: 0,
            timeout_ms: 0,
            query_cache: None,
        }
    }

    /// Create a new Bolt query executor with TenantDatabaseManager (멀티 DB 모드).
    pub fn with_db_manager(
        engine: Arc<GraphEngine>,
        db_manager: Arc<TenantDatabaseManager>,
    ) -> Self {
        Self {
            engine,
            db_manager: Some(db_manager),
            admin_service: None,
            fulltext_provider: None,
            vector_provider: None,
            shared_index_cache: None,
            planner: RwLock::new(QueryPlanner::new()),
            transactions: RwLock::new(HashMap::new()),
            next_tx_id: AtomicU64::new(1),
            max_result_size: 0,
            timeout_ms: 0,
            query_cache: None,
        }
    }

    pub fn with_admin_service(mut self, admin_service: Arc<dyn AdminService>) -> Self {
        self.admin_service = Some(admin_service);
        self
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

    /// 공유 인덱스 캐시 설정 (빌더 패턴)
    ///
    /// L2 영속화된 인덱스 메타데이터 캐시를 공유합니다.
    /// 서버 부트스트랩에서 생성된 캐시를 전달하여 잠금 충돌을 방지합니다.
    pub fn with_shared_index_cache(mut self, cache: Arc<zeta4j_cache::IndexCache>) -> Self {
        self.shared_index_cache = Some(cache);
        self
    }

    /// 쿼리 플랜 캐시 설정 (빌더 패턴)
    ///
    /// 파싱 및 플래닝 결과를 캐싱하여 반복 쿼리 성능을 향상시킵니다.
    pub fn with_query_cache(
        mut self,
        cache: Arc<zeta4j_cypher_executor::cache::ConcurrentQueryCacheManager>,
    ) -> Self {
        self.query_cache = Some(cache);
        self
    }

    /// 멀티 DB 모드 여부 확인
    fn is_multi_db_mode(&self) -> bool {
        self.db_manager.is_some()
    }

    /// 쿼리 문자열에서 USE 절의 데이터베이스 이름 추출
    fn extract_use_database(query: &str) -> Option<String> {
        let trimmed = query.trim();
        if !trimmed.to_uppercase().starts_with("USE ") {
            return None;
        }

        match parse_use(trimmed) {
            Ok((_, use_clause)) => match use_clause.database {
                DatabaseTarget::Named(name) => Some(name),
                DatabaseTarget::Dynamic(_) => None,
            },
            Err(_) => None,
        }
    }

    /// 데이터베이스 이름으로 엔진 조회 (트랜잭션 시작용)
    ///
    /// USE 절 없이 데이터베이스 이름만으로 엔진을 조회합니다.
    /// 멀티 DB 모드에서는 해당 데이터베이스가 존재해야 하고,
    /// 단일 DB 모드에서는 기본 엔진을 반환합니다.
    async fn resolve_engine_for_database(&self, db_name: &str) -> BoltResult<Arc<GraphEngine>> {
        if self.is_multi_db_mode() {
            self.get_engine_or_error(db_name).await
        } else {
            // 단일 DB 모드: 기본 엔진 사용
            Ok(self.engine.clone())
        }
    }

    /// USE 절을 고려하여 엔진 조회
    ///
    /// 우선순위:
    /// 1. 쿼리 내 USE 문의 데이터베이스 (있으면 필수 존재)
    /// 2. RUN 메시지의 database 파라미터 (멀티 DB 모드에서 필수 존재)
    /// 3. 단일 DB 모드: 기본 엔진
    async fn resolve_engine(&self, query: &str, db_name: &str) -> BoltResult<Arc<GraphEngine>> {
        // USE 절에서 데이터베이스 이름 추출 시도
        if let Some(use_db) = Self::extract_use_database(query) {
            return self.get_engine_or_error(&use_db).await;
        }

        // 멀티 DB 모드: RUN의 database 파라미터 필수 존재
        if self.is_multi_db_mode() {
            return self.get_engine_or_error(db_name).await;
        }

        // 단일 DB 모드: 기본 엔진 사용
        Ok(self.engine.clone())
    }

    /// 데이터베이스 엔진 조회 (존재하지 않으면 에러)
    async fn get_engine_or_error(&self, db_name: &str) -> BoltResult<Arc<GraphEngine>> {
        if let Some(ref db_manager) = self.db_manager {
            db_manager
                .get_engine(db_name)
                .await
                .ok_or_else(|| BoltError::Query(format!("Database '{}' does not exist", db_name)))
        } else {
            Err(BoltError::Query(format!(
                "Database '{}' not available in single-database mode",
                db_name
            )))
        }
    }

    /// Convert PackStreamValue parameters to Value format.
    fn convert_parameters(
        &self,
        params: &HashMap<String, PackStreamValue>,
    ) -> IndexMap<String, Value> {
        params
            .iter()
            .map(|(k, v)| (k.clone(), self.packstream_to_value(v)))
            .collect()
    }

    /// Convert PackStreamValue to Value.
    fn packstream_to_value(&self, value: &PackStreamValue) -> Value {
        match value {
            PackStreamValue::Null => Value::Null,
            PackStreamValue::Boolean(b) => Value::Boolean(*b),
            PackStreamValue::Integer(i) => Value::Integer(*i),
            PackStreamValue::Float(f) => Value::Float(*f),
            PackStreamValue::String(s) => Value::String(s.clone()),
            PackStreamValue::List(list) => {
                Value::Array(list.iter().map(|v| self.packstream_to_value(v)).collect())
            }
            PackStreamValue::Map(map) => Value::Map(
                map.iter()
                    .map(|(k, v)| (k.clone(), self.packstream_to_value(v)))
                    .collect(),
            ),
            PackStreamValue::Bytes(bytes) => Value::String(format!("{:?}", bytes)),
            PackStreamValue::Structure(_) => {
                // Structures are complex types (Node, Relationship, etc.)
                // For now, return null - full implementation would decode the structure
                Value::Null
            }
        }
    }

    /// Convert Value to PackStreamValue.
    #[async_recursion::async_recursion]
    async fn value_to_packstream(
        &self,
        engine: &Arc<GraphEngine>,
        value: &Value,
    ) -> PackStreamValue {
        match value {
            Value::Null => PackStreamValue::Null,
            Value::Boolean(b) => PackStreamValue::Boolean(*b),
            Value::Integer(i) => PackStreamValue::Integer(*i),
            Value::Float(f) => PackStreamValue::Float(*f),
            Value::String(s) => PackStreamValue::String(s.clone()),
            Value::Array(list) => {
                let mut result = Vec::with_capacity(list.len());
                for v in list {
                    result.push(self.value_to_packstream(engine, v).await);
                }
                PackStreamValue::List(result)
            }
            Value::Map(map) => {
                let mut result = HashMap::new();
                for (k, v) in map.iter() {
                    result.insert(k.clone(), self.value_to_packstream(engine, v).await);
                }
                PackStreamValue::Map(result)
            }
            Value::Node(id) => {
                // Fetch node from engine and create proper PackStreamNode
                if let Ok(Some(node)) = engine.get_node(*id).await {
                    let labels: Vec<String> = node.labels().iter().cloned().collect();
                    let mut props = HashMap::new();
                    for (k, v) in node.properties().iter() {
                        props.insert(k.clone(), self.value_to_packstream(engine, v).await);
                    }

                    let ps_node = PackStreamNode::new(*id as i64, labels, props)
                        .with_element_id(id.to_string());
                    ps_node.to_value()
                } else {
                    PackStreamValue::Null
                }
            }
            Value::Edge(id) => {
                // Fetch edge from engine and create proper PackStreamRelationship
                if let Ok(Some(edge)) = engine.get_edge(*id).await {
                    let mut props = HashMap::new();
                    for (k, v) in edge.properties().iter() {
                        props.insert(k.clone(), self.value_to_packstream(engine, v).await);
                    }

                    let mut ps_rel = PackStreamRelationship::new(
                        *id as i64,
                        edge.from_node() as i64,
                        edge.to_node() as i64,
                        edge.relation_type().to_string(),
                        props,
                    );
                    ps_rel.element_id = Some(id.to_string());
                    ps_rel.start_node_element_id = Some(edge.from_node().to_string());
                    ps_rel.end_node_element_id = Some(edge.to_node().to_string());
                    ps_rel.to_value()
                } else {
                    PackStreamValue::Null
                }
            }
            Value::Path(path) => {
                // Create proper PackStreamPath
                let mut nodes = Vec::new();
                for node_id in &path.nodes {
                    if let Ok(Some(node)) = engine.get_node(*node_id).await {
                        let labels: Vec<String> = node.labels().iter().cloned().collect();
                        let mut props = HashMap::new();
                        for (k, v) in node.properties().iter() {
                            props.insert(k.clone(), self.value_to_packstream(engine, v).await);
                        }
                        nodes.push(
                            PackStreamNode::new(*node_id as i64, labels, props)
                                .with_element_id(node_id.to_string()),
                        );
                    }
                }

                let mut relationships = Vec::new();
                for edge_id in &path.edges {
                    if let Ok(Some(edge)) = engine.get_edge(*edge_id).await {
                        let mut props = HashMap::new();
                        for (k, v) in edge.properties().iter() {
                            props.insert(k.clone(), self.value_to_packstream(engine, v).await);
                        }
                        relationships.push(PackStreamUnboundRelationship {
                            id: *edge_id as i64,
                            rel_type: edge.relation_type().to_string(),
                            properties: props,
                            element_id: Some(edge_id.to_string()),
                        });
                    }
                }

                // Build indices for path traversal
                let mut indices = Vec::new();
                for (i, _) in path.edges.iter().enumerate() {
                    indices.push((i + 1) as i64); // relationship index (1-based)
                    indices.push((i + 1) as i64); // next node index
                }

                let ps_path = PackStreamPath {
                    nodes,
                    relationships,
                    indices,
                };
                ps_path.to_value()
            }
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
}

#[async_trait]
impl AsyncQueryExecutor for BoltQueryExecutor {
    /// 쿼리 실행
    ///
    /// 세미콜론으로 구분된 여러 쿼리를 순차적으로 실행하고 마지막 결과를 반환합니다.
    ///
    /// v1.79.0: block_on 제거, 직접 await 사용
    /// v1.80.0: 세미콜론 구분 다중 쿼리 지원 (#156)
    /// v1.80.0: Admin 명령 권한 검사를 위한 username 파라미터 추가 (#166)
    async fn execute(
        &self,
        query: &str,
        parameters: &HashMap<String, PackStreamValue>,
        database: &str,
        tx_id: Option<u64>,
        principal: Option<&ExecutionPrincipal>,
    ) -> BoltResult<QueryResult> {
        // 세미콜론으로 쿼리 분할
        let queries = split_statements(query);

        // 단일 쿼리인 경우 기존 로직
        if queries.len() <= 1 {
            return self
                .execute_single_query(
                    queries.first().copied().unwrap_or(query),
                    parameters,
                    database,
                    tx_id,
                    principal,
                )
                .await;
        }

        // 다중 쿼리: 순차 실행, 마지막 결과 반환
        let mut last_result = None;
        for query_str in queries {
            last_result = Some(
                self.execute_single_query(query_str, parameters, database, tx_id, principal)
                    .await?,
            );
        }

        last_result.ok_or_else(|| BoltError::Query("Empty query".to_string()))
    }

    async fn begin_transaction(&self, database: &str) -> BoltResult<u64> {
        let bolt_tx_id = self.next_tx_id.fetch_add(1, Ordering::SeqCst);

        // GraphEngine 조회 및 트랜잭션 시작 (#169)
        let engine = self.resolve_engine_for_database(database).await?;
        let engine_tx_id = engine
            .begin_transaction()
            .map_err(|e| BoltError::Transaction(format!("Failed to begin transaction: {}", e)))?;

        let tx = BoltTransaction {
            bolt_tx_id,
            engine_tx_id,
            database: database.to_string(),
            engine: engine.clone(),
        };

        self.transactions.write().insert(bolt_tx_id, tx);

        tracing::debug!(
            bolt_tx_id = bolt_tx_id,
            engine_tx_id = engine_tx_id,
            database = database,
            "Bolt transaction started with GraphEngine transaction"
        );

        Ok(bolt_tx_id)
    }

    async fn commit_transaction(&self, bolt_tx_id: u64) -> BoltResult<String> {
        let tx = self
            .transactions
            .write()
            .remove(&bolt_tx_id)
            .ok_or_else(|| {
                BoltError::Transaction(format!("Transaction {} not found", bolt_tx_id))
            })?;

        // GraphEngine 트랜잭션 커밋 (#169)
        tx.engine
            .commit_transaction(tx.engine_tx_id)
            .map_err(|e| BoltError::Transaction(format!("Failed to commit transaction: {}", e)))?;

        tracing::debug!(
            bolt_tx_id = bolt_tx_id,
            engine_tx_id = tx.engine_tx_id,
            "Bolt transaction committed"
        );

        // 북마크에 engine_tx_id 포함 (실제 커밋된 트랜잭션 ID)
        let bookmark = format!("zeta4j:bookmark:v1:tx{}", tx.engine_tx_id);
        Ok(bookmark)
    }

    async fn rollback_transaction(&self, bolt_tx_id: u64) -> BoltResult<()> {
        let tx = self
            .transactions
            .write()
            .remove(&bolt_tx_id)
            .ok_or_else(|| {
                BoltError::Transaction(format!("Transaction {} not found", bolt_tx_id))
            })?;

        tracing::info!(
            bolt_tx_id = bolt_tx_id,
            engine_tx_id = tx.engine_tx_id,
            "Starting Bolt transaction rollback"
        );

        // GraphEngine 트랜잭션 롤백 (#169)
        // 롤백 실패 시에도 Bolt 트랜잭션은 이미 제거되었으므로 에러 로깅 후 계속 진행
        if let Err(e) = tx.engine.rollback_transaction(tx.engine_tx_id) {
            tracing::error!(
                bolt_tx_id = bolt_tx_id,
                engine_tx_id = tx.engine_tx_id,
                error = %e,
                "GraphEngine rollback failed, but continuing"
            );
        }

        tracing::info!(
            bolt_tx_id = bolt_tx_id,
            engine_tx_id = tx.engine_tx_id,
            "Bolt transaction rolled back"
        );

        Ok(())
    }
}

impl BoltQueryExecutor {
    /// 단일 쿼리 실행 (내부 헬퍼)
    ///
    /// #169: bolt_tx_id가 있으면 해당 트랜잭션 컨텍스트에서 실행합니다.
    /// #166: username이 있으면 Admin 명령 권한 검사에 사용합니다.
    async fn execute_single_query(
        &self,
        query: &str,
        parameters: &HashMap<String, PackStreamValue>,
        database: &str,
        bolt_tx_id: Option<u64>,
        principal: Option<&ExecutionPrincipal>,
    ) -> BoltResult<QueryResult> {
        // 트랜잭션 컨텍스트 확인 (#169)
        let (engine, engine_tx_id) = if let Some(bolt_id) = bolt_tx_id {
            let transactions = self.transactions.read();
            let tx = transactions.get(&bolt_id).ok_or_else(|| {
                BoltError::Transaction(format!("Transaction {} not found", bolt_id))
            })?;

            // 트랜잭션 내에서 다른 데이터베이스 접근 시도 확인
            if !database.is_empty() && tx.database != database {
                return Err(BoltError::Transaction(format!(
                    "Cannot access database '{}' in transaction started on database '{}'",
                    database, tx.database
                )));
            }

            (tx.engine.clone(), Some(tx.engine_tx_id))
        } else {
            // 트랜잭션 없음: USE 절 및 database 파라미터를 고려하여 엔진 결정
            let engine = self.resolve_engine(query, database).await?;
            (engine, None)
        };

        // Parse query
        let parsed = parse_query_safe(query).map_err(BoltError::from_zeta_error)?;

        // Plan query
        let _plan = {
            let planner = self.planner.read();
            planner
                .plan(&parsed)
                .map_err(|e| BoltError::Query(format!("Plan error: {}", e)))?
        };

        // Convert parameters
        let params = self.convert_parameters(parameters);

        // Execute query with resolved engine
        let mut executor = CypherQueryExecutor::new(engine.clone())
            .with_query_config(self.max_result_size, self.timeout_ms);
        if let Some(admin_service) = &self.admin_service {
            executor = executor.with_admin_service(admin_service.clone());
        }
        // 인증된 사용자 설정 (Admin 명령 권한 검사용, #166)
        if let Some(principal) = principal {
            executor = executor.with_current_user(principal.username.clone());
        }
        // Fulltext Provider 연결 (#155)
        if let Some(provider) = &self.fulltext_provider {
            executor = executor.with_fulltext_provider(provider.clone());
        }
        if let Some(provider) = &self.vector_provider {
            executor = executor.with_vector_provider(provider.clone());
        }
        // 공유 인덱스 캐시 연결 (L2 영속화)
        if let Some(cache) = &self.shared_index_cache {
            executor = executor.with_shared_index_cache(cache.clone());
        }
        // 쿼리 플랜 캐시 연결
        if let Some(cache) = &self.query_cache {
            executor = executor.with_shared_query_cache(cache.clone());
        }

        // 트랜잭션 모드에 따라 실행 (#169)
        let result = if let Some(tx_id) = engine_tx_id {
            // 명시적 트랜잭션 내에서 실행 (begin/commit 없음)
            match executor.execute_in_transaction(parsed, params, tx_id).await {
                Ok(result) => result,
                Err(error) => {
                    if error.requires_rollback()
                        && let Some(bolt_id) = bolt_tx_id
                    {
                        if let Err(cleanup_error) = engine.abort_transaction(tx_id) {
                            tracing::error!(
                                transaction_id = tx_id,
                                bolt_transaction_id = bolt_id,
                                error = %cleanup_error,
                                "failed to clean up terminal Bolt transaction"
                            );
                        }
                        self.transactions.write().remove(&bolt_id);
                    }
                    return Err(BoltError::from_graph_error(error));
                }
            }
        } else {
            // 자동 트랜잭션 (기존 방식: 내부에서 begin/commit)
            executor
                .execute_with_result_and_params(parsed, params)
                .await
                .map_err(BoltError::from_graph_error)?
        };

        // Convert results to PackStream format
        let (columns, rows, mutation_stats, memory_reservation) = match result {
            zeta4j_cypher_executor::executor::QueryResult::Rows {
                columns,
                rows,
                mutation_stats,
                _memory_reservation,
            } => (columns, rows, mutation_stats, _memory_reservation),
            zeta4j_cypher_executor::executor::QueryResult::Plan(_) => (
                vec![],
                vec![],
                zeta4j_cypher_executor::executor::MutationStats::new(),
                None,
            ),
            zeta4j_cypher_executor::executor::QueryResult::Analyze {
                columns,
                rows,
                mutation_stats,
                _memory_reservation,
                ..
            } => (columns, rows, mutation_stats, _memory_reservation),
        };

        let fields = columns;
        let mut records = Vec::new();

        for row in &rows {
            let mut record = Vec::with_capacity(row.len());
            for v in row {
                record.push(self.value_to_packstream(&engine, v).await);
            }
            records.push(record);
        }

        // MutationStats를 QueryStats로 변환
        let query_stats = QueryStats {
            nodes_created: mutation_stats.nodes_created as i64,
            nodes_deleted: mutation_stats.nodes_deleted as i64,
            relationships_created: mutation_stats.relationships_created as i64,
            relationships_deleted: mutation_stats.relationships_deleted as i64,
            properties_set: mutation_stats.properties_set as i64,
            labels_added: mutation_stats.labels_added as i64,
            labels_removed: mutation_stats.labels_removed as i64,
            indexes_added: mutation_stats.indexes_added as i64,
            indexes_removed: mutation_stats.indexes_removed as i64,
            constraints_added: mutation_stats.constraints_added as i64,
            constraints_removed: mutation_stats.constraints_removed as i64,
            contains_updates: mutation_stats.has_updates(),
            contains_system_updates: false,
        };

        Ok(QueryResult {
            fields,
            records,
            stats: query_stats,
            bookmark: None,
            memory_reservation: memory_reservation.map(Arc::new),
        })
    }
}
