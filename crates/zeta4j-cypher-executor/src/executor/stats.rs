//! 쿼리 실행 통계
//!
//! EXPLAIN ANALYZE에서 사용되는 실행 통계와 변경 통계 구조체를 정의합니다.

use std::time::Duration;

// ============================================================================
// MutationStats - 변경 통계 (CREATE, DELETE, SET 등)
// ============================================================================

/// 데이터 변경 통계
///
/// CREATE, DELETE, SET, MERGE 등의 변경 작업에서 발생한 변경 내역을 추적합니다.
/// Neo4j의 쿼리 통계와 호환됩니다.
#[derive(Debug, Clone, Default)]
pub struct MutationStats {
    /// 생성된 노드 수
    pub nodes_created: u64,
    /// 삭제된 노드 수
    pub nodes_deleted: u64,
    /// 생성된 관계 수
    pub relationships_created: u64,
    /// 삭제된 관계 수
    pub relationships_deleted: u64,
    /// 설정된 프로퍼티 수
    pub properties_set: u64,
    /// 추가된 레이블 수
    pub labels_added: u64,
    /// 제거된 레이블 수
    pub labels_removed: u64,
    /// 추가된 인덱스 수
    pub indexes_added: u64,
    /// 제거된 인덱스 수
    pub indexes_removed: u64,
    /// 추가된 제약조건 수
    pub constraints_added: u64,
    /// 제거된 제약조건 수
    pub constraints_removed: u64,
}

impl MutationStats {
    /// 새 변경 통계 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 변경 사항이 있는지 확인
    pub fn has_updates(&self) -> bool {
        self.nodes_created > 0
            || self.nodes_deleted > 0
            || self.relationships_created > 0
            || self.relationships_deleted > 0
            || self.properties_set > 0
            || self.labels_added > 0
            || self.labels_removed > 0
            || self.indexes_added > 0
            || self.indexes_removed > 0
            || self.constraints_added > 0
            || self.constraints_removed > 0
    }

    /// 다른 통계와 합치기
    pub fn merge(&mut self, other: &MutationStats) {
        self.nodes_created += other.nodes_created;
        self.nodes_deleted += other.nodes_deleted;
        self.relationships_created += other.relationships_created;
        self.relationships_deleted += other.relationships_deleted;
        self.properties_set += other.properties_set;
        self.labels_added += other.labels_added;
        self.labels_removed += other.labels_removed;
        self.indexes_added += other.indexes_added;
        self.indexes_removed += other.indexes_removed;
        self.constraints_added += other.constraints_added;
        self.constraints_removed += other.constraints_removed;
    }

    /// 노드 생성 카운트 증가
    #[inline]
    pub fn inc_nodes_created(&mut self, count: u64) {
        self.nodes_created += count;
    }

    /// 노드 삭제 카운트 증가
    #[inline]
    pub fn inc_nodes_deleted(&mut self, count: u64) {
        self.nodes_deleted += count;
    }

    /// 관계 생성 카운트 증가
    #[inline]
    pub fn inc_relationships_created(&mut self, count: u64) {
        self.relationships_created += count;
    }

    /// 관계 삭제 카운트 증가
    #[inline]
    pub fn inc_relationships_deleted(&mut self, count: u64) {
        self.relationships_deleted += count;
    }

    /// 프로퍼티 설정 카운트 증가
    #[inline]
    pub fn inc_properties_set(&mut self, count: u64) {
        self.properties_set += count;
    }

    /// 레이블 추가 카운트 증가
    #[inline]
    pub fn inc_labels_added(&mut self, count: u64) {
        self.labels_added += count;
    }

    /// 레이블 제거 카운트 증가
    #[inline]
    pub fn inc_labels_removed(&mut self, count: u64) {
        self.labels_removed += count;
    }
}

// ============================================================================
// OperatorStats - EXPLAIN ANALYZE용 연산자별 실행 통계
// ============================================================================

/// 개별 연산자의 실행 통계
#[derive(Debug, Clone, Default)]
pub struct OperatorStats {
    /// 연산자 이름 (예: "NodeByLabelScan")
    pub operator: String,
    /// 연산자 설명 (예: "n:Person")
    pub description: String,
    /// 예상 행 수 (EXPLAIN에서 추정된 값)
    pub estimated_rows: u64,
    /// 실제 처리된 행 수
    pub actual_rows: u64,
    /// 실제 실행 시간
    pub actual_time: Duration,
    /// 반복 실행 횟수
    pub loops: u64,
}

impl OperatorStats {
    /// 새 연산자 통계 생성
    pub fn new(operator: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            operator: operator.into(),
            description: description.into(),
            estimated_rows: 0,
            actual_rows: 0,
            actual_time: Duration::ZERO,
            loops: 1,
        }
    }

    /// 예상 행 수 설정
    pub fn with_estimated_rows(mut self, rows: u64) -> Self {
        self.estimated_rows = rows;
        self
    }

    /// 실제 행 수 설정
    pub fn with_actual_rows(mut self, rows: u64) -> Self {
        self.actual_rows = rows;
        self
    }

    /// 실제 시간 설정
    pub fn with_actual_time(mut self, time: Duration) -> Self {
        self.actual_time = time;
        self
    }

    /// 루프 횟수 설정
    pub fn with_loops(mut self, loops: u64) -> Self {
        self.loops = loops;
        self
    }

    /// 실제 시간을 밀리초로 반환
    pub fn actual_time_ms(&self) -> f64 {
        self.actual_time.as_secs_f64() * 1000.0
    }

    /// 예상 대비 실제 행 수 비율 (1.0 = 정확히 예측)
    pub fn row_accuracy(&self) -> f64 {
        if self.estimated_rows == 0 {
            if self.actual_rows == 0 {
                1.0
            } else {
                f64::INFINITY
            }
        } else {
            self.actual_rows as f64 / self.estimated_rows as f64
        }
    }
}

/// 전체 쿼리 실행 통계
#[derive(Debug, Clone, Default)]
pub struct ExecutionStats {
    /// 연산자별 통계 (실행 순서)
    pub operators: Vec<OperatorStats>,
    /// 계획 생성 시간
    pub planning_time: Duration,
    /// 전체 실행 시간
    pub execution_time: Duration,
    /// 전체 처리된 행 수
    pub total_rows: u64,
}

impl ExecutionStats {
    /// 새 실행 통계 생성
    pub fn new() -> Self {
        Self::default()
    }

    /// 연산자 통계 추가
    pub fn add_operator(&mut self, stats: OperatorStats) {
        self.operators.push(stats);
    }

    /// 계획 시간 설정
    pub fn with_planning_time(mut self, time: Duration) -> Self {
        self.planning_time = time;
        self
    }

    /// 실행 시간 설정
    pub fn with_execution_time(mut self, time: Duration) -> Self {
        self.execution_time = time;
        self
    }

    /// 총 행 수 설정
    pub fn with_total_rows(mut self, rows: u64) -> Self {
        self.total_rows = rows;
        self
    }

    /// 계획 시간을 밀리초로 반환
    pub fn planning_time_ms(&self) -> f64 {
        self.planning_time.as_secs_f64() * 1000.0
    }

    /// 실행 시간을 밀리초로 반환
    pub fn execution_time_ms(&self) -> f64 {
        self.execution_time.as_secs_f64() * 1000.0
    }

    /// 연산자 수 반환
    pub fn operator_count(&self) -> usize {
        self.operators.len()
    }

    /// 예상 대비 실제 행 수의 전체 정확도 (RMSE)
    pub fn overall_row_accuracy(&self) -> f64 {
        if self.operators.is_empty() {
            return 1.0;
        }

        let sum_sq_error: f64 = self
            .operators
            .iter()
            .map(|op| {
                let diff = op.actual_rows as f64 - op.estimated_rows as f64;
                diff * diff
            })
            .sum();

        (sum_sq_error / self.operators.len() as f64).sqrt()
    }
}

/// EXPLAIN ANALYZE 출력 포맷터
pub struct AnalyzeFormatter<'a> {
    stats: &'a ExecutionStats,
    total_cost: f64,
}

impl<'a> AnalyzeFormatter<'a> {
    /// 새 포맷터 생성
    pub fn new(stats: &'a ExecutionStats, total_cost: f64) -> Self {
        Self { stats, total_cost }
    }

    /// 상세 출력 문자열 생성
    pub fn format(&self) -> String {
        let mut result = String::new();

        // 헤더
        result.push_str("+------------------------------------------------------------+\n");
        result.push_str("|                    EXPLAIN ANALYZE                          |\n");
        result.push_str("+------------------------------------------------------------+\n");
        result.push_str(&format!(
            "| Planning Time: {:>10.3} ms                               |\n",
            self.stats.planning_time_ms()
        ));
        result.push_str(&format!(
            "| Execution Time: {:>9.3} ms                               |\n",
            self.stats.execution_time_ms()
        ));
        result.push_str(&format!(
            "| Total Cost: {:>13.4}                                   |\n",
            self.total_cost
        ));
        result.push_str("+------------------------------------------------------------+\n\n");

        // 연산자별 통계
        for (i, op) in self.stats.operators.iter().enumerate() {
            let indent = "  ".repeat(i);
            let connector = if i == 0 { "" } else { "└── " };

            result.push_str(&format!("{}{}{}", indent, connector, op.operator));
            if !op.description.is_empty() {
                result.push_str(&format!(" ({})", op.description));
            }
            result.push('\n');

            let detail_indent = format!("{}    ", indent);
            result.push_str(&format!(
                "{}├── Estimated Rows: {}\n",
                detail_indent, op.estimated_rows
            ));
            result.push_str(&format!(
                "{}├── Actual Rows: {}\n",
                detail_indent, op.actual_rows
            ));
            result.push_str(&format!(
                "{}├── Actual Time: {:.3} ms\n",
                detail_indent,
                op.actual_time_ms()
            ));
            result.push_str(&format!("{}└── Loops: {}\n", detail_indent, op.loops));
            result.push('\n');
        }

        // 요약
        result.push_str("+------------------------------------------------------------+\n");
        result.push_str("|                        SUMMARY                              |\n");
        result.push_str("+------------------------------------------------------------+\n");
        result.push_str(&format!(
            "| Operators: {:>10}                                       |\n",
            self.stats.operator_count()
        ));
        result.push_str(&format!(
            "| Total Rows: {:>9}                                       |\n",
            self.stats.total_rows
        ));
        result.push_str(&format!(
            "| Planning Time: {:>10.3} ms                               |\n",
            self.stats.planning_time_ms()
        ));
        result.push_str(&format!(
            "| Execution Time: {:>9.3} ms                               |\n",
            self.stats.execution_time_ms()
        ));
        result.push_str("+------------------------------------------------------------+\n");

        result
    }
}
