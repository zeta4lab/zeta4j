//! 쿼리 실행 결과 타입 정의.

use super::QueryPlan;
use super::stats::{ExecutionStats, MutationStats};
use zeta4j_graph::core::Value;
use zeta4j_graph::storage::memory::MemoryReservation;

/// Materialized rows with a reservation that lives until rows are consumed.
#[derive(Debug)]
pub struct MaterializedRows {
    rows: Vec<Vec<Value>>,
    _memory_reservation: Option<MemoryReservation>,
}

impl MaterializedRows {
    pub(crate) fn new(
        rows: Vec<Vec<Value>>,
        memory_reservation: Option<MemoryReservation>,
    ) -> Self {
        Self {
            rows,
            _memory_reservation: memory_reservation,
        }
    }
}

impl std::ops::Deref for MaterializedRows {
    type Target = [Vec<Value>];

    fn deref(&self) -> &Self::Target {
        &self.rows
    }
}

/// Consuming iterator that retains the result reservation until iteration ends.
pub struct MaterializedRowsIntoIter {
    rows: std::vec::IntoIter<Vec<Value>>,
    _memory_reservation: Option<MemoryReservation>,
}

impl Iterator for MaterializedRowsIntoIter {
    type Item = Vec<Value>;

    fn next(&mut self) -> Option<Self::Item> {
        self.rows.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.rows.size_hint()
    }
}

impl ExactSizeIterator for MaterializedRowsIntoIter {}

impl IntoIterator for MaterializedRows {
    type Item = Vec<Value>;
    type IntoIter = MaterializedRowsIntoIter;

    fn into_iter(self) -> Self::IntoIter {
        MaterializedRowsIntoIter {
            rows: self.rows.into_iter(),
            _memory_reservation: self._memory_reservation,
        }
    }
}

/// 쿼리 실행 결과
///
/// Cypher 쿼리 실행의 세 가지 결과 유형을 표현합니다.
///
/// # Variants
///
/// * `Rows` - 일반 실행 결과 (가장 일반적인 경우)
/// * `Plan` - EXPLAIN 쿼리 결과 (실행하지 않고 계획만 반환)
/// * `Analyze` - EXPLAIN ANALYZE 쿼리 결과 (실행하며 통계 수집)
///
/// # 예제
///
/// ```ignore
/// let result = executor.execute_with_result(query)?;
///
/// match result {
///     QueryResult::Rows { columns, rows } => {
///         // 일반 실행 결과 처리
///         for (i, col) in columns.iter().enumerate() {
///             println!("{}: {:?}", col, rows[0][i]);
///         }
///     }
///     QueryResult::Plan(plan) => {
///         // 실행 계획 출력
///         println!("{}", plan.to_tree_string());
///     }
///     QueryResult::Analyze { plan, stats, .. } => {
///         // 실행 계획 + 통계
///         println!("{}", plan.to_explain_detail(stats.total_cost()));
///     }
/// }
/// ```
#[derive(Debug)]
pub enum QueryResult {
    /// 일반 실행 결과 (컬럼 이름 + 행들의 리스트)
    Rows {
        /// 컬럼 이름 목록 (RETURN 절에서 추론)
        columns: Vec<String>,
        /// 결과 행들 (각 행은 Value 벡터)
        rows: Vec<Vec<Value>>,
        /// 변경 통계 (CREATE, DELETE, SET 등)
        mutation_stats: MutationStats,
        /// Keeps the query ledger reservation alive through protocol materialization.
        #[doc(hidden)]
        _memory_reservation: Option<MemoryReservation>,
    },
    /// EXPLAIN 결과 - 실행하지 않고 계획만 반환
    Plan(QueryPlan),
    /// EXPLAIN ANALYZE 결과 - 실행하며 통계 수집
    Analyze {
        /// 실행 계획
        plan: QueryPlan,
        /// 실행 통계 (계획 시간, 실행 시간, 연산자별 통계)
        stats: ExecutionStats,
        /// 컬럼 이름 목록
        columns: Vec<String>,
        /// 실제 실행 결과
        rows: Vec<Vec<Value>>,
        /// 변경 통계 (CREATE, DELETE, SET 등)
        mutation_stats: MutationStats,
        /// Keeps the query ledger reservation alive through protocol materialization.
        #[doc(hidden)]
        _memory_reservation: Option<MemoryReservation>,
    },
}

impl QueryResult {
    /// Build a row result while transferring an already-admitted result owner.
    pub fn rows_with_reservation(
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
        mutation_stats: MutationStats,
        memory_reservation: Option<MemoryReservation>,
    ) -> Self {
        Self::Rows {
            columns,
            rows,
            mutation_stats,
            _memory_reservation: memory_reservation,
        }
    }
}
