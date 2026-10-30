//! Cypher 쿼리 실행 엔진
//!
//! 파싱된 Cypher AST를 실행하여 그래프 데이터베이스에서 데이터를 조회/변경하는 실행 엔진입니다.
//!
//! # 주요 구성요소
//!
//! - [`QueryExecutor`]: 쿼리 실행기 (메인 API)
//! - [`QueryResult`]: 쿼리 실행 결과 (Rows, Plan, Analyze)
//! - [`ExecutionContext`]: 실행 컨텍스트 (변수 바인딩, 파라미터)
//! - [`ExecutionStats`]: 실행 통계 (EXPLAIN ANALYZE용)
//!
//! # 모듈 구조
//!
//! ```text
//! executor/
//! ├── mod.rs           - QueryExecutor 실행 메서드
//! ├── result.rs        - QueryResult 열거형
//! ├── context.rs       - ExecutionContext, QueryHints
//! ├── core.rs          - QueryExecutor 구조체, 빌더 메서드
//! ├── reading.rs       - MATCH/OPTIONAL MATCH 실행
//! ├── updating.rs      - CREATE/DELETE/SET/MERGE 실행
//! ├── clauses.rs       - WITH/UNWIND/RETURN 실행
//! ├── expression.rs    - 표현식 평가
//! ├── aggregation.rs   - 집계 함수 처리
//! ├── graph_algorithms.rs - 그래프 알고리즘 함수
//! ├── scalar_engine.rs - 시간/공간 함수
//! ├── scalar_graph.rs  - 그래프 기반 스칼라 함수
//! ├── schema.rs        - 스키마 DDL 실행
//! └── stats.rs         - 실행 통계 수집
//! ```

// 타입 정의 모듈
mod context;
mod core;
mod result;

// 실행 로직 모듈
mod admin;
mod aggregation;
mod graph_algorithms;
mod scalar;
mod scalar_graph;
mod schema;
pub mod stats;
mod temporal;

// 새로 분리된 모듈
mod clauses;
mod expression;
mod reading;
mod updating;
mod utils;

// Re-exports from submodules
pub use context::{ExecutionContext, QueryHints, RangeIndexCondition};
pub use core::QueryExecutor;
pub use result::{MaterializedRows, QueryResult};

pub use stats::{AnalyzeFormatter, ExecutionStats, MutationStats, OperatorStats};

// 외부 서비스 트레이트 - cypher-core에서 재수출
pub use zeta4j_cypher_core::services::{
    AdminCapability, AdminService, AdminServiceError, AdminServiceResult, DatabaseInfo, UserInfo,
};

// 그래프 알고리즘 헬퍼 함수 재수출
pub use graph_algorithms::{
    extract_node_id_from_map, extract_node_id_list, extract_single_node_id,
    extract_weighted_edge_list,
};

// 시간 관련 헬퍼 함수 재수출
pub use temporal::{
    get_date_property, get_datetime_property, get_duration_property, get_local_datetime_property,
    get_time_property, parse_iso8601_duration, validate_arg_count,
};

// 온톨로지 자동 검증 모드 재수출 (cypher-core의 AST에서 정의됨)

// Internal imports for execution methods
use indexmap::IndexMap;
use std::time::Instant;
use zeta4j_graph::core::Value;
use zeta4j_graph::storage::memory::{MemoryCategory, MemoryReservation};
use zeta4j_graph::transaction::TransactionId;
use zeta4j_graph::{GraphError, Result};

use zeta4j_cypher_core::ast::*;
use zeta4j_cypher_core::optimizer::union::UnionOptimizer;
use zeta4j_cypher_core::planner::QueryPlanner;

// 내부 타입 재수출
pub(crate) use context::ExecutorIndexCondition;

// ============================================================================
// QueryExecutor 실행 메서드 구현
// ============================================================================

impl QueryExecutor {
    /// 쿼리 실행 (행 반환)
    ///
    /// 쿼리를 실행하고 결과 행들을 반환합니다.
    /// EXPLAIN 쿼리인 경우 에러를 반환합니다.
    ///
    /// # 주의
    ///
    /// EXPLAIN / EXPLAIN ANALYZE 쿼리는 [`execute_with_result`](Self::execute_with_result)를 사용하세요.
    pub async fn execute(&self, query: Query) -> Result<MaterializedRows> {
        self.execute_with_parameters(query, IndexMap::new()).await
    }

    /// 파라미터와 함께 쿼리 실행
    ///
    /// 쿼리에서 `$param` 형식으로 파라미터를 참조할 수 있습니다.
    pub async fn execute_with_parameters(
        &self,
        query: Query,
        parameters: IndexMap<String, Value>,
    ) -> Result<MaterializedRows> {
        if query.explain {
            return Err(GraphError::Parse {
                message: "Use execute_with_result() for EXPLAIN queries".to_string(),
            });
        }
        let tx_id = self.engine.begin_transaction()?;
        match self
            .execute_query_body_with_params(&query.body, tx_id, &parameters)
            .await
        {
            Ok((mut rows, _mutation_stats, mut query_memory)) => {
                self.engine.commit_transaction(tx_id)?;
                // max_result_size 적용 (0 = 무제한)
                if self.max_result_size > 0 && rows.len() > self.max_result_size {
                    rows.truncate(self.max_result_size);
                }
                self.finalize_query_rows(&mut query_memory, &rows)?;
                Ok(MaterializedRows::new(rows, query_memory))
            }
            Err(e) => {
                // 에러 시 트랜잭션 롤백 (락 해제 포함)
                if let Err(rollback_err) = self.engine.rollback_transaction(tx_id) {
                    tracing::error!(
                        tx_id = tx_id,
                        original_error = %e,
                        rollback_error = %rollback_err,
                        "Transaction rollback failed"
                    );
                }
                Err(e)
            }
        }
    }

    /// 쿼리 실행 (EXPLAIN / EXPLAIN ANALYZE 지원) - **권장 API**
    ///
    /// 쿼리 유형에 따라 적절한 결과를 반환합니다:
    /// - **일반 쿼리**: `QueryResult::Rows` - 컬럼과 행 반환
    /// - **EXPLAIN**: `QueryResult::Plan` - 실행 계획만 반환 (실행 안 함)
    /// - **EXPLAIN ANALYZE**: `QueryResult::Analyze` - 실행 + 통계 수집
    pub async fn execute_with_result(&self, query: Query) -> Result<QueryResult> {
        self.execute_with_result_and_params(query, IndexMap::new())
            .await
    }

    /// 파라미터와 함께 쿼리 실행 (EXPLAIN / EXPLAIN ANALYZE 지원)
    ///
    /// v1.15.0: 동시성 충돌 시 자동 재시도 (최대 3회, 지수 백오프)
    pub async fn execute_with_result_and_params(
        &self,
        query: Query,
        parameters: IndexMap<String, Value>,
    ) -> Result<QueryResult> {
        // EXPLAIN ANALYZE 모드: 실제 실행하면서 통계 수집
        if query.analyze {
            return self
                .execute_with_analyze_and_params(query, &parameters)
                .await;
        }

        if query.explain {
            // EXPLAIN 모드: 실행하지 않고 계획만 반환
            // 인덱스 정보를 플래너에 전달
            let planner = self.create_planner_with_indexes();
            let plan = planner.plan(&query)?;
            return Ok(QueryResult::Plan(plan));
        }

        // 컬럼 이름 추론 (실행 전에)
        let columns = self.extract_column_names_from_body(&query.body);

        // 재시도 설정: 동시성 충돌 시 자동 재시도
        const MAX_RETRIES: u32 = 3;
        const BASE_DELAY_MS: u64 = 10;

        // 재시도 루프
        for attempt in 0..=MAX_RETRIES {
            let tx_id = self.engine.begin_transaction()?;
            match self
                .execute_query_body_with_params(&query.body, tx_id, &parameters)
                .await
            {
                Ok((mut rows, mutation_stats, mut query_memory)) => {
                    self.engine.commit_transaction(tx_id)?;
                    // max_result_size 적용 (0 = 무제한)
                    if self.max_result_size > 0 && rows.len() > self.max_result_size {
                        rows.truncate(self.max_result_size);
                    }
                    self.finalize_query_rows(&mut query_memory, &rows)?;
                    return Ok(QueryResult::Rows {
                        columns,
                        rows,
                        mutation_stats,
                        _memory_reservation: query_memory,
                    });
                }
                Err(e) => {
                    // 에러 시 트랜잭션 롤백 (락 해제 포함)
                    if let Err(rollback_err) = self.engine.rollback_transaction(tx_id) {
                        tracing::error!(
                            tx_id = tx_id,
                            original_error = %e,
                            rollback_error = %rollback_err,
                            "Transaction rollback failed"
                        );
                    }

                    // 재시도 가능하고 재시도 횟수가 남았으면 재시도
                    if e.is_retryable() && attempt < MAX_RETRIES {
                        let delay_ms = BASE_DELAY_MS * (1 << attempt); // 10, 20, 40ms
                        tracing::debug!(
                            attempt = attempt + 1,
                            max_retries = MAX_RETRIES,
                            delay_ms = delay_ms,
                            error = %e,
                            "Concurrency conflict, retrying"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }

                    return Err(e);
                }
            }
        }

        // 여기 도달하면 안 됨 (루프에서 항상 반환)
        unreachable!("Retry loop should not exit without returning")
    }

    /// 기존 트랜잭션 컨텍스트에서 쿼리 실행 (#169)
    ///
    /// Bolt 프로토콜의 명시적 트랜잭션(BEGIN/COMMIT/ROLLBACK)을 지원합니다.
    /// 외부에서 시작된 트랜잭션 ID를 받아 해당 컨텍스트에서 쿼리를 실행합니다.
    ///
    /// **주의**: 이 메서드는 트랜잭션 시작/커밋/롤백을 수행하지 않습니다.
    /// 호출자가 트랜잭션 라이프사이클을 관리해야 합니다.
    pub async fn execute_in_transaction(
        &self,
        query: Query,
        parameters: IndexMap<String, Value>,
        tx_id: TransactionId,
    ) -> Result<QueryResult> {
        // EXPLAIN은 트랜잭션과 무관하게 계획만 반환
        if query.explain && !query.analyze {
            let planner = self.create_planner_with_indexes();
            let plan = planner.plan(&query)?;
            return Ok(QueryResult::Plan(plan));
        }

        // EXPLAIN ANALYZE는 기존 트랜잭션에서 실행
        if query.analyze {
            return self
                .execute_with_analyze_in_transaction(query, &parameters, tx_id)
                .await;
        }

        // 컬럼 이름 추론 (실행 전에)
        let columns = self.extract_column_names_from_body(&query.body);

        // 기존 트랜잭션 컨텍스트에서 실행 (begin/commit 없음)
        let (mut rows, mutation_stats, mut query_memory) = self
            .execute_query_body_with_params(&query.body, tx_id, &parameters)
            .await?;

        // max_result_size 적용 (0 = 무제한)
        if self.max_result_size > 0 && rows.len() > self.max_result_size {
            rows.truncate(self.max_result_size);
        }
        self.finalize_query_rows(&mut query_memory, &rows)?;

        Ok(QueryResult::Rows {
            columns,
            rows,
            mutation_stats,
            _memory_reservation: query_memory,
        })
    }

    /// 기존 트랜잭션 컨텍스트에서 EXPLAIN ANALYZE 실행 (#169)
    async fn execute_with_analyze_in_transaction(
        &self,
        query: Query,
        parameters: &IndexMap<String, Value>,
        tx_id: TransactionId,
    ) -> Result<QueryResult> {
        let total_start = Instant::now();

        // 계획 생성 시간 측정
        let planning_start = Instant::now();
        let planner = self.create_planner_with_indexes();
        let plan = planner.plan(&query)?;
        let planning_time = planning_start.elapsed();

        // 컬럼 이름 추론 (실행 전에)
        let columns = self.extract_column_names_from_body(&query.body);

        // 실행 시간 측정 (begin/commit 없음)
        let execution_start = Instant::now();
        let (mut rows, mutation_stats, mut query_memory) = self
            .execute_query_body_with_params(&query.body, tx_id, parameters)
            .await?;
        let execution_time = execution_start.elapsed();

        // max_result_size 적용 (0 = 무제한)
        if self.max_result_size > 0 && rows.len() > self.max_result_size {
            rows.truncate(self.max_result_size);
        }
        self.finalize_query_rows(&mut query_memory, &rows)?;

        // 실행 통계 수집
        let mut stats = ExecutionStats::new()
            .with_planning_time(planning_time)
            .with_execution_time(execution_time)
            .with_total_rows(rows.len() as u64);

        // 실행 계획의 각 노드에 대한 통계 추가
        self.collect_plan_stats(&plan, &mut stats, &rows);

        let _ = total_start; // avoid unused warning

        Ok(QueryResult::Analyze {
            plan,
            stats,
            columns,
            rows,
            mutation_stats,
            _memory_reservation: query_memory,
        })
    }

    /// EXPLAIN ANALYZE 실행: 쿼리 실행 + 통계 수집
    #[expect(dead_code)] // EXPLAIN ANALYZE 기능에 사용
    async fn execute_with_analyze(&self, query: Query) -> Result<QueryResult> {
        self.execute_with_analyze_and_params(query, &IndexMap::new())
            .await
    }

    /// EXPLAIN ANALYZE 실행: 파라미터와 함께 쿼리 실행 + 통계 수집
    async fn execute_with_analyze_and_params(
        &self,
        query: Query,
        parameters: &IndexMap<String, Value>,
    ) -> Result<QueryResult> {
        let total_start = Instant::now();

        // 계획 생성 시간 측정
        let planning_start = Instant::now();
        let planner = self.create_planner_with_indexes();
        let plan = planner.plan(&query)?;
        let planning_time = planning_start.elapsed();

        // 컬럼 이름 추론 (실행 전에)
        let columns = self.extract_column_names_from_body(&query.body);

        // 실행 시간 측정
        let execution_start = Instant::now();
        let tx_id = self.engine.begin_transaction()?;
        let (mut rows, mutation_stats, mut query_memory) = match self
            .execute_query_body_with_params(&query.body, tx_id, parameters)
            .await
        {
            Ok((rows, mutation_stats, query_memory)) => {
                self.engine.commit_transaction(tx_id)?;
                (rows, mutation_stats, query_memory)
            }
            Err(e) => {
                // 에러 시 트랜잭션 롤백 (락 해제 포함)
                if let Err(rollback_err) = self.engine.rollback_transaction(tx_id) {
                    tracing::error!(
                        tx_id = tx_id,
                        original_error = %e,
                        rollback_error = %rollback_err,
                        "Transaction rollback failed"
                    );
                }
                return Err(e);
            }
        };
        let execution_time = execution_start.elapsed();

        // max_result_size 적용 (0 = 무제한)
        if self.max_result_size > 0 && rows.len() > self.max_result_size {
            rows.truncate(self.max_result_size);
        }
        self.finalize_query_rows(&mut query_memory, &rows)?;

        // 실행 통계 수집
        let mut stats = ExecutionStats::new()
            .with_planning_time(planning_time)
            .with_execution_time(execution_time)
            .with_total_rows(rows.len() as u64);

        // 실행 계획의 각 노드에 대한 통계 추가
        self.collect_plan_stats(&plan, &mut stats, &rows);

        let _ = total_start; // avoid unused warning

        Ok(QueryResult::Analyze {
            plan,
            stats,
            columns,
            rows,
            mutation_stats,
            _memory_reservation: query_memory,
        })
    }

    /// 실행 계획의 각 노드에 대한 통계 수집
    fn collect_plan_stats(
        &self,
        plan: &QueryPlan,
        stats: &mut ExecutionStats,
        result: &[Vec<Value>],
    ) {
        use super::ast::PlanNode;

        // 재귀적으로 계획 노드 순회
        fn collect_node_stats(node: &PlanNode, stats: &mut ExecutionStats, actual_rows: u64) {
            let operator_name = format!("{:?}", node.node_type);
            let op_stats = OperatorStats::new(operator_name, &node.description)
                .with_estimated_rows(node.estimated_rows)
                .with_actual_rows(actual_rows)
                .with_loops(1);

            stats.add_operator(op_stats);

            // 자식 노드 처리
            for child in &node.children {
                collect_node_stats(child, stats, actual_rows);
            }
        }

        let actual_rows = result.len() as u64;
        collect_node_stats(&plan.root, stats, actual_rows);
    }

    /// Reserve a materialized row set that will outlive the current executor call.
    pub fn reserve_query_rows(&self, rows: &[Vec<Value>]) -> Result<Option<MemoryReservation>> {
        self.engine
            .try_reserve_memory(MemoryCategory::Query, Self::rows_memory_size(rows))
    }

    fn finalize_query_rows(
        &self,
        reservation: &mut Option<MemoryReservation>,
        rows: &[Vec<Value>],
    ) -> Result<()> {
        if let Some(reservation) = reservation {
            reservation.resize(Self::rows_memory_size(rows))?;
        }
        Ok(())
    }

    fn rows_memory_size(rows: &[Vec<Value>]) -> usize {
        std::mem::size_of::<Vec<Vec<Value>>>()
            .saturating_add(rows.len().saturating_mul(std::mem::size_of::<Vec<Value>>()))
            .saturating_add(
                rows.iter()
                    .flat_map(|row| row.iter())
                    .map(Value::memory_size)
                    .fold(0usize, usize::saturating_add),
            )
    }

    fn named_values_memory_size(values: &IndexMap<String, Value>) -> usize {
        values
            .iter()
            .map(|(name, value)| {
                std::mem::size_of::<String>()
                    .saturating_add(name.len())
                    .saturating_add(value.memory_size())
            })
            .fold(0usize, usize::saturating_add)
    }

    fn contexts_memory_size(contexts: &[ExecutionContext]) -> usize {
        contexts
            .iter()
            .map(|context| {
                std::mem::size_of::<ExecutionContext>()
                    .saturating_add(Self::named_values_memory_size(&context.variables))
                    .saturating_add(Self::named_values_memory_size(&context.parameters))
            })
            .fold(
                std::mem::size_of::<Vec<ExecutionContext>>(),
                usize::saturating_add,
            )
    }

    fn resize_query_working_set(
        &self,
        reservation: &mut Option<MemoryReservation>,
        contexts: &[ExecutionContext],
        rows: &[Vec<Value>],
    ) -> Result<()> {
        if let Some(reservation) = reservation {
            reservation.resize(
                Self::contexts_memory_size(contexts).saturating_add(Self::rows_memory_size(rows)),
            )?;
        }
        Ok(())
    }

    /// 인덱스 존재 여부 확인 (캐시 활용)
    ///
    /// L1 캐시(moka)를 활용하여 has_property_index 호출을 최적화합니다.
    pub(crate) fn has_property_index_cached(&self, label: &str, prop: &str) -> bool {
        self.index_cache
            .has_index(label, prop, || self.engine.has_property_index(label, prop))
    }

    /// 인덱스 캐시 무효화 (인덱스 생성/삭제 시 호출)
    pub fn invalidate_index_cache(&self, label: &str, prop: &str) {
        self.index_cache.invalidate(label, prop);
    }

    /// 전체 인덱스 캐시 무효화
    pub fn clear_index_cache(&self) {
        self.index_cache.clear();
    }

    /// 인덱스 정보가 포함된 플래너 생성
    fn create_planner_with_indexes(&self) -> QueryPlanner {
        use super::planner::IndexRegistry;
        use std::collections::HashSet;

        let mut index_registry: IndexRegistry = HashSet::new();

        // GraphEngine에서 인덱스 정보 가져오기 (캐시 활용)
        let all_indexes = self.index_cache.get_all(|| {
            let mut set = HashSet::new();
            for (_, label, properties) in self.engine.list_property_indexes() {
                for prop in properties {
                    set.insert((label.clone(), prop));
                }
            }
            set
        });

        index_registry.extend(all_indexes);

        QueryPlanner::with_indexes(index_registry)
    }

    /// 쿼리 본체에서 컬럼 이름 추출 (RETURN 절 분석)
    fn extract_column_names_from_body(&self, body: &QueryBody) -> Vec<String> {
        match body {
            QueryBody::Single(single_query) => self.extract_column_names_from_single(single_query),
            QueryBody::Union { left, .. } => {
                // UNION의 경우 왼쪽 쿼리의 컬럼 이름 사용 (Neo4j와 동일)
                self.extract_column_names_from_body(left)
            }
        }
    }

    /// 단일 쿼리에서 컬럼 이름 추출
    fn extract_column_names_from_single(&self, single_query: &SingleQuery) -> Vec<String> {
        // RETURN 절 찾기
        for stmt in &single_query.statements {
            if let Statement::Return(return_clause) = stmt {
                // 빈 컨텍스트로 컬럼 이름 추출 (RETURN * 의 경우 빈 배열 반환)
                let dummy_ctx = ExecutionContext::new(0);
                return self.extract_column_names(return_clause, &dummy_ctx);
            }
        }
        // RETURN 절이 없는 경우 빈 배열
        vec![]
    }

    /// 쿼리 본체 실행 (Single 또는 Union)
    /// 반환: (결과 행들, 변경 통계)
    #[expect(dead_code)] // 파라미터 없는 버전
    async fn execute_query_body(
        &self,
        body: &QueryBody,
        tx_id: TransactionId,
    ) -> Result<(Vec<Vec<Value>>, MutationStats, Option<MemoryReservation>)> {
        self.execute_query_body_with_params(body, tx_id, &IndexMap::new())
            .await
    }

    /// 파라미터와 함께 쿼리 본체 실행 (Single 또는 Union)
    /// 반환: (결과 행들, 변경 통계)
    #[async_recursion::async_recursion]
    async fn execute_query_body_with_params(
        &self,
        body: &QueryBody,
        tx_id: TransactionId,
        parameters: &IndexMap<String, Value>,
    ) -> Result<(Vec<Vec<Value>>, MutationStats, Option<MemoryReservation>)> {
        match body {
            QueryBody::Single(single_query) => {
                self.execute_single_query_with_params(single_query, tx_id, parameters)
                    .await
            }
            QueryBody::Union { left, right, all } => {
                // UNION 최적화기를 통한 분석
                let optimizer = UnionOptimizer::new();
                let optimization_result = optimizer.optimize(body);

                // 병렬 실행 가능 여부 확인 (읽기 전용 브랜치만)
                let (left_result, left_stats, left_memory, right_result, right_stats, right_memory) =
                    if optimization_result.can_parallelize {
                        // 병렬 실행: tokio::try_join! 사용
                        let left_future =
                            self.execute_query_body_with_params(left, tx_id, parameters);
                        let right_future =
                            self.execute_query_body_with_params(right, tx_id, parameters);

                        let ((lr, ls, lm), (rr, rs, rm)) =
                            tokio::try_join!(left_future, right_future)?;
                        (lr, ls, lm, rr, rs, rm)
                    } else {
                        // 순차 실행 (기존 방식)
                        let (lr, ls, lm) = self
                            .execute_query_body_with_params(left, tx_id, parameters)
                            .await?;
                        let (rr, rs, rm) = self
                            .execute_query_body_with_params(right, tx_id, parameters)
                            .await?;
                        (lr, ls, lm, rr, rs, rm)
                    };

                // 통계 병합
                let mut combined_stats = left_stats;
                combined_stats.merge(&right_stats);

                // 결과 합치기
                let mut combined = left_result;
                combined.extend(right_result);
                let mut combined_memory = match (left_memory, right_memory) {
                    (Some(mut left), Some(right)) => {
                        left.merge(right)?;
                        Some(left)
                    }
                    (None, None) => None,
                    _ => {
                        return Err(GraphError::System {
                            message: "UNION query memory owners are inconsistent".to_string(),
                        });
                    }
                };

                let combined = if *all {
                    // UNION ALL: 중복 포함
                    combined
                } else {
                    // UNION: 중복 제거
                    self.apply_distinct_rows(combined)
                };
                self.finalize_query_rows(&mut combined_memory, &combined)?;
                Ok((combined, combined_stats, combined_memory))
            }
        }
    }

    /// 단일 쿼리 실행
    #[expect(dead_code)] // 파라미터 없는 버전
    async fn execute_single_query(
        &self,
        single_query: &SingleQuery,
        tx_id: TransactionId,
    ) -> Result<(Vec<Vec<Value>>, MutationStats, Option<MemoryReservation>)> {
        self.execute_single_query_with_params(single_query, tx_id, &IndexMap::new())
            .await
    }

    /// 파라미터와 함께 단일 쿼리 실행
    /// 반환: (결과 행들, 변경 통계)
    async fn execute_single_query_with_params(
        &self,
        single_query: &SingleQuery,
        tx_id: TransactionId,
        parameters: &IndexMap<String, Value>,
    ) -> Result<(Vec<Vec<Value>>, MutationStats, Option<MemoryReservation>)> {
        let initial_context_bytes = std::mem::size_of::<Vec<ExecutionContext>>()
            .saturating_add(std::mem::size_of::<ExecutionContext>())
            .saturating_add(Self::named_values_memory_size(parameters));
        let mut query_memory = self
            .engine
            .try_reserve_memory(MemoryCategory::Query, initial_context_bytes)?;
        // 초기: 파라미터를 포함한 컨텍스트 1개
        let mut contexts = vec![ExecutionContext::with_parameters(tx_id, parameters.clone())];
        let mut final_result = Vec::new();
        let mut return_clause_opt: Option<ReturnClause> = None;
        // 전체 변경 통계 (모든 컨텍스트에서 병합)
        let mut total_mutation_stats = MutationStats::new();

        // LIMIT push-down: RETURN 절에서 힌트 추출
        let hints = self.extract_query_hints(single_query);

        for stmt in &single_query.statements {
            match stmt {
                Statement::Reading(clause) => {
                    // MATCH / OPTIONAL MATCH: 컨텍스트 확장 (FlatMap)
                    // LIMIT push-down 적용 (hints 전달)
                    let mut next_contexts = Vec::new();
                    let effective_limit = hints.effective_limit();

                    for ctx in contexts {
                        // 이미 충분한 결과가 있으면 조기 종료
                        if let Some(limit) = effective_limit
                            && next_contexts.len() >= limit
                        {
                            break;
                        }

                        let matches = self
                            .execute_reading_with_hints(clause, &ctx, &hints)
                            .await?;
                        next_contexts.extend(matches);
                    }
                    contexts = next_contexts;

                    // LIMIT 도달 시 조기 종료
                    if let Some(limit) = effective_limit
                        && contexts.len() > limit
                    {
                        contexts.truncate(limit);
                    }

                    // OPTIONAL MATCH는 빈 결과여도 계속 진행 (NULL로 바인딩됨)
                    // 일반 MATCH만 빈 결과시 중단 (단, 집계 RETURN이 있으면 계속 진행)
                    let is_optional = matches!(clause, ReadingClause::Match { optional: true, .. });
                    if contexts.is_empty() && !is_optional && !hints.has_aggregation {
                        break; // 매칭된 것 없음 -> 결과 없음
                    }
                }
                Statement::Updating(clause) => {
                    // CREATE: 각 컨텍스트마다 실행
                    for ctx in &mut contexts {
                        self.execute_updating(clause, ctx).await?;
                    }
                }
                Statement::With(clause) => {
                    // WITH: 컨텍스트 변환 및 필터링
                    contexts = self.execute_with(clause, contexts).await?;

                    // 집계 RETURN이 있으면 빈 결과에서도 계속 진행
                    if contexts.is_empty() && !hints.has_aggregation {
                        break; // 결과 없음
                    }
                }
                Statement::Unwind(clause) => {
                    // UNWIND: 리스트를 행으로 펼침
                    contexts = self.execute_unwind(clause, contexts).await?;

                    // 집계 RETURN이 있으면 빈 결과에서도 계속 진행
                    if contexts.is_empty() && !hints.has_aggregation {
                        break; // 결과 없음
                    }
                }
                Statement::Call(call_subquery) => {
                    // CALL { subquery }: 서브쿼리 실행 및 결과 병합
                    contexts = self.execute_call_subquery(call_subquery, contexts).await?;

                    // 집계 RETURN이 있으면 빈 결과에서도 계속 진행
                    if contexts.is_empty() && !hints.has_aggregation {
                        break; // 결과 없음
                    }
                }
                Statement::Return(clause) => {
                    // RETURN: 집계 함수 여부에 따라 처리 방식 결정
                    if self.has_aggregate_in_return(clause) {
                        // 집계 함수가 있는 경우: 그룹화 후 집계 실행
                        let (rows, aggregated_contexts) =
                            self.execute_aggregated_return(clause, &contexts).await?;
                        final_result = rows;
                        contexts = aggregated_contexts;
                    } else {
                        // 집계 함수가 없는 경우: 기존 방식대로 처리
                        let mut updated_contexts = Vec::with_capacity(contexts.len());
                        for ctx in &contexts {
                            let row = self.execute_return_items(clause, ctx).await?;

                            // alias 매핑을 context에 추가
                            let mut new_ctx = ctx.clone();
                            for (i, item) in clause.items.iter().enumerate() {
                                if let ReturnItem::Expression {
                                    expr: _,
                                    alias: Some(alias_name),
                                } = item
                                {
                                    // alias가 있으면 해당 값을 context에 추가
                                    if i < row.len() {
                                        new_ctx
                                            .variables
                                            .insert(alias_name.clone(), row[i].clone());
                                    }
                                }
                            }
                            updated_contexts.push(new_ctx);
                            final_result.push(row);
                        }
                        contexts = updated_contexts;
                    }
                    return_clause_opt = Some(clause.clone());
                }
                Statement::Index(cmd) => {
                    // 인덱스 관리 명령 실행
                    final_result = self.execute_index_command(cmd, tx_id).await?;
                }
                Statement::Schema(cmd) => {
                    // 스키마 관리 명령 실행
                    final_result = self.execute_schema_command(cmd)?;
                }
                Statement::Constraint(cmd) => {
                    // 제약조건 관리 명령 실행
                    final_result = self.execute_constraint_command(cmd)?;
                }
                Statement::Procedure(proc_call) => {
                    // 프로시저 호출 실행
                    contexts = self.execute_procedure(proc_call, contexts).await?;
                }
                Statement::Admin(cmd) => {
                    // 관리 명령 실행 (사용자, 역할, 권한, 데이터베이스)
                    final_result = self.execute_admin_command(cmd).await?;
                }
                Statement::Finish => {
                    // FINISH: 결과 없이 종료 (Neo4j 5.x)
                    final_result = Vec::new();
                    break;
                }
                Statement::Let(let_clause) => {
                    // LET: 변수 바인딩 (Neo4j 5.x)
                    for ctx in &mut contexts {
                        let value = self
                            .evaluate_expression(&let_clause.expression, ctx)
                            .await?;
                        ctx.variables.insert(let_clause.variable.clone(), value);
                    }
                }
                Statement::Use(use_clause) => {
                    // USE: 데이터베이스 선택
                    match &use_clause.database {
                        DatabaseTarget::Named(name) => {
                            // USE database: 데이터베이스 선택 (세션 레벨에서 처리)
                            let _ = name; // 유효성 검사용
                        }
                        DatabaseTarget::Dynamic(expr) => {
                            // graph.byName(): 동적 데이터베이스 선택
                            let _db_name = if contexts.is_empty() {
                                let empty_ctx = ExecutionContext::new(tx_id);
                                match self.evaluate_expression(expr, &empty_ctx).await? {
                                    Value::String(name) => name,
                                    other => {
                                        return Err(GraphError::Validation {
                                            message: format!(
                                                "USE graph.byName() requires a string, got {:?}",
                                                other
                                            ),
                                        });
                                    }
                                }
                            } else {
                                match self.evaluate_expression(expr, &contexts[0]).await? {
                                    Value::String(name) => name,
                                    other => {
                                        return Err(GraphError::Validation {
                                            message: format!(
                                                "USE graph.byName() requires a string, got {:?}",
                                                other
                                            ),
                                        });
                                    }
                                }
                            };
                        }
                    }
                }
            }
            self.resize_query_working_set(&mut query_memory, &contexts, &final_result)?;
        }

        // ORDER BY, SKIP, LIMIT 적용
        if let Some(return_clause) = return_clause_opt {
            final_result = self
                .apply_order_skip_limit(final_result, &return_clause, &contexts)
                .await?;
            self.resize_query_working_set(&mut query_memory, &contexts, &final_result)?;
        }

        // 모든 컨텍스트의 mutation_stats 병합
        for ctx in &contexts {
            total_mutation_stats.merge(&ctx.mutation_stats);
        }

        Ok((final_result, total_mutation_stats, query_memory))
    }

    /// Prim MST 실행 헬퍼
    async fn run_prim_mst(
        &self,
        config: zeta4j_graph::algorithm::mst::PrimConfig,
    ) -> Result<Value> {
        use zeta4j_graph::algorithm::mst::Prim;

        let prim = Prim::with_config(&self.engine, config);
        let result = prim.run().await?;
        graph_algorithms::mst_result_to_value(result)
    }

    // ============================================
    // LIMIT Push-down 최적화
    // ============================================

    /// 쿼리에서 최적화 힌트 추출
    ///
    /// RETURN 절의 LIMIT, SKIP, ORDER BY, 집계 함수 존재 여부를 분석하여
    /// LIMIT push-down 가능 여부를 판단합니다.
    fn extract_query_hints(&self, single_query: &SingleQuery) -> QueryHints {
        let mut hints = QueryHints::default();

        // RETURN 절 찾기
        for stmt in &single_query.statements {
            if let Statement::Return(return_clause) = stmt {
                // LIMIT 추출 (이미 u64로 파싱됨)
                if let Some(limit_val) = return_clause.limit {
                    hints.limit = Some(limit_val as usize);
                }

                // SKIP 추출 (이미 u64로 파싱됨)
                if let Some(skip_val) = return_clause.skip {
                    hints.skip = Some(skip_val as usize);
                }

                // ORDER BY 존재 여부
                hints.has_order_by = return_clause
                    .order_by
                    .as_ref()
                    .is_some_and(|v| !v.is_empty());

                // 집계 함수 존재 여부
                hints.has_aggregation = self.has_aggregate_in_return(return_clause);

                break;
            }
        }

        // WITH 절에서도 ORDER BY 확인 (중간에 정렬이 있으면 push-down 불가)
        for stmt in &single_query.statements {
            if let Statement::With(with_clause) = stmt
                && with_clause.order_by.as_ref().is_some_and(|v| !v.is_empty())
            {
                hints.has_order_by = true;
                break;
            }
        }

        hints
    }
}

#[cfg(test)]
mod transaction_read_tests {
    use std::sync::Arc;

    use indexmap::IndexMap;
    use zeta4j_cypher_core::parser::parse_query;
    use zeta4j_graph::core::Value;
    use zeta4j_graph::engine::{GraphEngineConfig, InMemoryCanonicalGraphStore};
    use zeta4j_graph::storage::memory::{MemoryCategory, MemoryLimiterConfig};
    use zeta4j_graph::{GraphEngine, GraphError};

    use super::{QueryExecutor, QueryResult};

    fn parse(input: &str) -> Result<zeta4j_cypher_core::ast::Query, std::io::Error> {
        parse_query(input)
            .map(|(_, query)| query)
            .map_err(|error| std::io::Error::other(format!("failed to parse Cypher: {error:?}")))
    }

    #[tokio::test]
    async fn explicit_transaction_match_reads_created_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = Arc::new(GraphEngine::with_temp_dir().await?);
        let executor = QueryExecutor::new(Arc::clone(&engine));
        let tx_id = engine.begin_transaction()?;

        executor
            .execute_in_transaction(
                parse(
                    "CREATE (a:Person {name: 'Ada'})-[:KNOWS {since: 2020}]->(b:Person {name: 'Bob'})",
                )?,
                IndexMap::new(),
                tx_id,
            )
            .await?;
        let result = executor
            .execute_in_transaction(
                parse(
                    "MATCH (a:Person)-[r:KNOWS]->(b:Person) \
                     RETURN a.name, labels(a), type(r), properties(r), keys(a), count(b)",
                )?,
                IndexMap::new(),
                tx_id,
            )
            .await?;

        match result {
            QueryResult::Rows { rows, .. } => {
                assert_eq!(rows.len(), 1);
                let mut relationship_properties = IndexMap::new();
                relationship_properties.insert("since".to_string(), Value::Integer(2020));
                assert_eq!(
                    rows[0],
                    vec![
                        Value::String("Ada".to_string()),
                        Value::Array(vec![Value::String("Person".to_string())]),
                        Value::String("KNOWS".to_string()),
                        Value::Map(relationship_properties),
                        Value::Array(vec![Value::String("name".to_string())]),
                        Value::Integer(1),
                    ]
                );
            }
            other => {
                return Err(std::io::Error::other(format!(
                    "expected rows from MATCH, got {other:?}"
                ))
                .into());
            }
        }

        let merge_result = executor
            .execute_in_transaction(
                parse("MERGE (a:Person {name: 'Ada'}) RETURN a.name, labels(a)")?,
                IndexMap::new(),
                tx_id,
            )
            .await?;
        match merge_result {
            QueryResult::Rows { rows, .. } => assert_eq!(
                rows,
                vec![vec![
                    Value::String("Ada".to_string()),
                    Value::Array(vec![Value::String("Person".to_string())]),
                ]]
            ),
            other => {
                return Err(std::io::Error::other(format!(
                    "expected rows from MERGE, got {other:?}"
                ))
                .into());
            }
        }
        let delta = engine.prepare_transaction_delta(tx_id).await?;
        assert_eq!(delta.upsert_nodes.len(), 2);
        assert_eq!(delta.upsert_edges.len(), 1);
        engine.commit_transaction(tx_id)?;
        Ok(())
    }

    #[tokio::test]
    async fn memory_admission_rejects_large_result_and_recovers_after_result_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine = Arc::new(
            GraphEngine::new(
                GraphEngineConfig::default().with_memory_limit(MemoryLimiterConfig {
                    max_memory_bytes: 4096,
                    auto_cleanup_enabled: false,
                    ..MemoryLimiterConfig::default()
                }),
                Arc::new(InMemoryCanonicalGraphStore::default()),
            )
            .await?,
        );
        let executor = QueryExecutor::new(Arc::clone(&engine));
        let large_query = format!("RETURN '{}' AS payload", "x".repeat(16 * 1024));
        let rejected = executor.execute_with_result(parse(&large_query)?).await;
        assert!(matches!(rejected, Err(GraphError::Memory(_))));
        let mut large_parameters = IndexMap::new();
        large_parameters.insert("payload".to_string(), Value::String("p".repeat(16 * 1024)));
        let rejected_parameter = executor
            .execute_with_result_and_params(parse("RETURN $payload AS payload")?, large_parameters)
            .await;
        assert!(matches!(rejected_parameter, Err(GraphError::Memory(_))));
        assert_eq!(
            engine.memory_limiter_stats().and_then(|stats| {
                stats
                    .category(MemoryCategory::Query)
                    .map(|category| category.current_usage)
            }),
            Some(0)
        );

        let result = executor
            .execute_with_result(parse("RETURN 1 AS value")?)
            .await?;
        let query_bytes = engine
            .memory_limiter_stats()
            .and_then(|stats| {
                stats
                    .category(MemoryCategory::Query)
                    .map(|category| category.current_usage)
            })
            .unwrap_or(0);
        assert!(query_bytes > 0);
        drop(result);
        assert_eq!(
            engine.memory_limiter_stats().and_then(|stats| {
                stats
                    .category(MemoryCategory::Query)
                    .map(|category| category.current_usage)
            }),
            Some(0)
        );

        let materialized = executor.execute(parse("RETURN 2 AS value")?).await?;
        assert!(
            engine
                .memory_limiter_stats()
                .and_then(|stats| {
                    stats
                        .category(MemoryCategory::Query)
                        .map(|category| category.current_usage)
                })
                .unwrap_or(0)
                > 0
        );
        drop(materialized);
        assert_eq!(
            engine.memory_limiter_stats().and_then(|stats| {
                stats
                    .category(MemoryCategory::Query)
                    .map(|category| category.current_usage)
            }),
            Some(0)
        );
        Ok(())
    }
}
