//! 쿼리 실행 컨텍스트 정의.

use indexmap::IndexMap;
use zeta4j_graph::core::{Path, Value};
use zeta4j_graph::transaction::TransactionId;

use super::stats::MutationStats;

/// 쿼리 실행 컨텍스트
///
/// 쿼리 실행 중 변수 바인딩, 파라미터, 트랜잭션 정보를 관리합니다.
/// MATCH에서 매칭된 노드/관계가 변수에 바인딩되고,
/// 이후 절에서 해당 변수를 참조할 수 있습니다.
///
/// # 예제
///
/// ```ignore
/// // 기본 컨텍스트 생성
/// let ctx = ExecutionContext::new(tx_id);
///
/// // 파라미터 포함 컨텍스트 생성
/// let mut params = IndexMap::new();
/// params.insert("name".to_string(), Value::String("Alice".to_string()));
/// let ctx = ExecutionContext::with_parameters(tx_id, params);
/// ```
#[derive(Clone)]
pub struct ExecutionContext {
    /// 변수명 -> Value 매핑 (MATCH에서 바인딩된 노드/관계 등)
    pub variables: IndexMap<String, Value>,
    /// 쿼리 파라미터 ($param -> Value 매핑)
    pub parameters: IndexMap<String, Value>,
    /// 현재 활성 트랜잭션 ID
    pub tx_id: TransactionId,
    /// 변경 통계 (CREATE, DELETE, SET 등) - 실행 중 업데이트됨
    pub mutation_stats: MutationStats,
}

/// 인덱스 사용 가능한 조건 (WHERE n.prop = value)
/// NOTE: ast::IndexCondition과 구분하기 위해 별도 이름 사용
#[expect(dead_code)] // 향후 인덱스 최적화에 사용
#[derive(Debug, Clone)]
pub(crate) struct ExecutorIndexCondition {
    /// 변수명 (예: "n")
    pub variable: String,
    /// 프로퍼티 (예: "name")
    pub property: String,
    /// 검색 값
    pub value: Value,
}

/// 범위 인덱스 조건 (WHERE n.prop > value AND n.prop < value)
#[derive(Debug, Clone)]
pub struct RangeIndexCondition {
    /// 변수명 (예: "n")
    pub variable: String,
    /// 프로퍼티 (예: "age")
    pub property: String,
    /// 하한값 (None이면 무한대)
    pub lower: Option<Value>,
    /// 하한 포함 여부 (>= vs >)
    pub lower_inclusive: bool,
    /// 상한값 (None이면 무한대)
    pub upper: Option<Value>,
    /// 상한 포함 여부 (<= vs <)
    pub upper_inclusive: bool,
}

/// 쿼리 실행 힌트 (최적화용)
///
/// LIMIT Push-down 등 최적화에 사용되는 힌트 정보입니다.
/// 특정 조건에서 MATCH 단계에서 조기 종료를 가능하게 합니다.
#[derive(Debug, Clone, Default)]
pub struct QueryHints {
    /// LIMIT 값 (push-down 가능한 경우)
    pub limit: Option<usize>,
    /// SKIP 값
    pub skip: Option<usize>,
    /// ORDER BY 존재 여부 (있으면 limit push-down 불가)
    pub has_order_by: bool,
    /// 집계 함수 존재 여부 (있으면 limit push-down 불가)
    pub has_aggregation: bool,
}

impl QueryHints {
    /// LIMIT push-down 가능 여부
    /// - ORDER BY가 없어야 함 (정렬 후 LIMIT 적용 필요)
    /// - 집계 함수가 없어야 함 (전체 데이터 필요)
    #[inline]
    pub fn can_push_limit(&self) -> bool {
        self.limit.is_some() && !self.has_order_by && !self.has_aggregation
    }

    /// 실제 적용할 limit 값 (skip 고려)
    #[inline]
    pub fn effective_limit(&self) -> Option<usize> {
        if self.can_push_limit() {
            self.limit.map(|l| l + self.skip.unwrap_or(0))
        } else {
            None
        }
    }
}

impl ExecutionContext {
    pub fn new(tx_id: TransactionId) -> Self {
        Self {
            variables: IndexMap::new(),
            parameters: IndexMap::new(),
            tx_id,
            mutation_stats: MutationStats::new(),
        }
    }

    /// 파라미터를 포함한 컨텍스트 생성
    pub fn with_parameters(tx_id: TransactionId, parameters: IndexMap<String, Value>) -> Self {
        Self {
            variables: IndexMap::new(),
            parameters,
            tx_id,
            mutation_stats: MutationStats::new(),
        }
    }

    // =========================================================================
    // Builder 메서드 - 변수 바인딩용
    // =========================================================================

    /// 노드 ID를 변수에 바인딩
    ///
    /// # 예시
    ///
    /// ```ignore
    /// ctx.bind_node("n", 42);
    /// // ctx.variables["n"] = Value::Node(42)
    /// ```
    pub fn bind_node(&mut self, name: impl Into<String>, id: u64) -> &mut Self {
        self.variables.insert(name.into(), Value::Node(id));
        self
    }

    /// 엣지 ID를 변수에 바인딩
    ///
    /// # 예시
    ///
    /// ```ignore
    /// ctx.bind_edge("r", 123);
    /// // ctx.variables["r"] = Value::Edge(123)
    /// ```
    pub fn bind_edge(&mut self, name: impl Into<String>, id: u64) -> &mut Self {
        self.variables.insert(name.into(), Value::Edge(id));
        self
    }

    /// 경로를 변수에 바인딩
    ///
    /// # 예시
    ///
    /// ```ignore
    /// let path = Path { nodes: vec![1, 2, 3], edges: vec![10, 20] };
    /// ctx.bind_path("p", path);
    /// ```
    pub fn bind_path(&mut self, name: impl Into<String>, path: Path) -> &mut Self {
        self.variables.insert(name.into(), Value::Path(path));
        self
    }

    /// NULL 값을 변수에 바인딩
    ///
    /// # 예시
    ///
    /// ```ignore
    /// ctx.bind_null("optional");
    /// // ctx.variables["optional"] = Value::Null
    /// ```
    pub fn bind_null(&mut self, name: impl Into<String>) -> &mut Self {
        self.variables.insert(name.into(), Value::Null);
        self
    }

    /// 여러 바인딩을 한 번에 추가
    ///
    /// # 예시
    ///
    /// ```ignore
    /// ctx.bind_all([
    ///     ("n".to_string(), Value::Node(1)),
    ///     ("r".to_string(), Value::Edge(10)),
    /// ]);
    /// ```
    pub fn bind_all(&mut self, bindings: impl IntoIterator<Item = (String, Value)>) -> &mut Self {
        for (name, value) in bindings {
            self.variables.insert(name, value);
        }
        self
    }

    /// 임의의 값을 변수에 바인딩
    ///
    /// # 예시
    ///
    /// ```ignore
    /// ctx.bind_value("count", Value::Integer(42));
    /// ctx.bind_value("name", Value::String("Alice".to_string()));
    /// ```
    pub fn bind_value(&mut self, name: impl Into<String>, value: Value) -> &mut Self {
        self.variables.insert(name.into(), value);
        self
    }
}
