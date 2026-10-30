//! Cypher HTTP API Types
//!
//! Bolt HTTP API 호환 Cypher 요청/응답 타입

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use zeta4j_cypher_executor::MutationStats;
use zeta4j_graph::storage::memory::MemoryReservation;

// ============================================================================
// Request Types
// ============================================================================

/// Cypher 실행 요청
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CypherRequest {
    /// 실행할 Cypher 문장 목록
    #[serde(default)]
    pub statements: Vec<Statement>,
}

/// 단일 Cypher 문장
#[derive(Debug, Clone, Deserialize)]
pub struct Statement {
    /// Cypher 쿼리 문자열
    pub statement: String,

    /// 쿼리 파라미터
    #[serde(default)]
    pub parameters: HashMap<String, serde_json::Value>,

    /// 결과 데이터 포맷 (row, graph, rest)
    #[serde(default, rename = "resultDataContents")]
    pub result_data_contents: Vec<ResultDataContent>,

    /// 쿼리 통계 포함 여부
    #[serde(default, rename = "includeStats")]
    pub include_stats: bool,
}

/// 결과 데이터 포맷
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResultDataContent {
    /// 행 데이터 (기본값)
    #[default]
    Row,
    /// 그래프 데이터 (노드, 관계)
    Graph,
    /// REST 포맷 (deprecated)
    Rest,
}

// ============================================================================
// Response Types
// ============================================================================

/// Cypher 실행 응답
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CypherResponse {
    /// 각 문장의 실행 결과
    pub results: Vec<StatementResult>,

    /// 발생한 에러 목록
    pub errors: Vec<CypherError>,

    /// 트랜잭션 커밋 URL (열린 트랜잭션인 경우)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,

    /// 트랜잭션 정보 (열린 트랜잭션인 경우)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<TransactionInfo>,
}

impl CypherResponse {
    /// 성공 응답 생성
    pub fn success(results: Vec<StatementResult>) -> Self {
        Self {
            results,
            errors: vec![],
            commit: None,
            transaction: None,
        }
    }

    /// 에러 응답 생성
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            results: vec![],
            errors: vec![CypherError::new(code, message)],
            commit: None,
            transaction: None,
        }
    }

    /// 트랜잭션 응답 생성
    pub fn with_transaction(mut self, db_name: &str, tx_id: &str, expires: &str) -> Self {
        self.commit = Some(format!("/db/{}/tx/{}/commit", db_name, tx_id));
        self.transaction = Some(TransactionInfo {
            expires: expires.to_string(),
        });
        self
    }
}

/// 단일 문장 실행 결과
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatementResult {
    /// 결과 컬럼 이름
    pub columns: Vec<String>,

    /// 결과 행 데이터
    pub data: Vec<ResultRow>,

    /// 쿼리 통계 (includeStats=true인 경우)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<QueryStats>,

    /// Retains the query reservation through HTTP response serialization.
    #[serde(skip)]
    pub(crate) memory_reservation: Option<Arc<MemoryReservation>>,
}

impl StatementResult {
    /// 새 결과 생성
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            data: vec![],
            stats: None,
            memory_reservation: None,
        }
    }

    /// 행 추가
    pub fn with_row(mut self, row: ResultRow) -> Self {
        self.data.push(row);
        self
    }

    /// 통계 추가
    pub fn with_stats(mut self, stats: QueryStats) -> Self {
        self.stats = Some(stats);
        self
    }

    pub(crate) fn with_memory_reservation(
        mut self,
        memory_reservation: Option<MemoryReservation>,
    ) -> Self {
        self.memory_reservation = memory_reservation.map(Arc::new);
        self
    }
}

/// 결과 행
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResultRow {
    /// 행 데이터 (resultDataContents에 "row" 포함 시)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<Vec<serde_json::Value>>,

    /// 메타데이터 (각 값의 타입 정보)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Vec<serde_json::Value>>,

    /// 그래프 데이터 (resultDataContents에 "graph" 포함 시)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphData>,
}

impl ResultRow {
    /// Row 포맷 결과 생성 (Neo4j 호환: meta 필드 포함)
    pub fn row(values: Vec<serde_json::Value>) -> Self {
        let meta = vec![serde_json::Value::Null; values.len()];
        Self {
            row: Some(values),
            meta: Some(meta),
            graph: None,
        }
    }

    /// Graph 포맷 결과 생성
    pub fn graph(nodes: Vec<NodeData>, relationships: Vec<RelationshipData>) -> Self {
        Self {
            row: None,
            meta: None,
            graph: Some(GraphData {
                nodes,
                relationships,
            }),
        }
    }

    /// Row + Graph 포맷 결과 생성 (Neo4j 호환: meta 필드 포함)
    pub fn row_and_graph(
        values: Vec<serde_json::Value>,
        nodes: Vec<NodeData>,
        relationships: Vec<RelationshipData>,
    ) -> Self {
        let meta = vec![serde_json::Value::Null; values.len()];
        Self {
            row: Some(values),
            meta: Some(meta),
            graph: Some(GraphData {
                nodes,
                relationships,
            }),
        }
    }
}

/// 그래프 데이터
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GraphData {
    /// 노드 목록
    pub nodes: Vec<NodeData>,

    /// 관계 목록
    pub relationships: Vec<RelationshipData>,
}

/// 노드 데이터
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeData {
    /// 노드 ID (숫자 문자열)
    pub id: String,

    /// 노드 레이블
    pub labels: Vec<String>,

    /// 노드 프로퍼티
    pub properties: HashMap<String, serde_json::Value>,

    /// Element ID (형식: "4:uuid:id")
    #[serde(rename = "elementId")]
    pub element_id: String,
}

impl NodeData {
    /// 새 노드 데이터 생성
    pub fn new(
        id: u64,
        labels: Vec<String>,
        properties: HashMap<String, serde_json::Value>,
    ) -> Self {
        let id_str = id.to_string();
        Self {
            element_id: format!("4:{}:{}", uuid::Uuid::now_v7(), id),
            id: id_str,
            labels,
            properties,
        }
    }
}

/// 관계 데이터
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipData {
    /// 관계 ID (숫자 문자열)
    pub id: String,

    /// 관계 타입
    #[serde(rename = "type")]
    pub rel_type: String,

    /// 시작 노드 ID
    #[serde(rename = "startNode")]
    pub start_node: String,

    /// 끝 노드 ID
    #[serde(rename = "endNode")]
    pub end_node: String,

    /// 관계 프로퍼티
    pub properties: HashMap<String, serde_json::Value>,

    /// Element ID
    #[serde(rename = "elementId")]
    pub element_id: String,

    /// 시작 노드 Element ID
    #[serde(rename = "startNodeElementId")]
    pub start_node_element_id: String,

    /// 끝 노드 Element ID
    #[serde(rename = "endNodeElementId")]
    pub end_node_element_id: String,
}

impl RelationshipData {
    /// 새 관계 데이터 생성
    pub fn new(
        id: u64,
        rel_type: String,
        start_node: u64,
        end_node: u64,
        properties: HashMap<String, serde_json::Value>,
    ) -> Self {
        let uuid = uuid::Uuid::now_v7();
        Self {
            id: id.to_string(),
            rel_type,
            start_node: start_node.to_string(),
            end_node: end_node.to_string(),
            properties,
            element_id: format!("5:{}:{}", uuid, id),
            start_node_element_id: format!("4:{}:{}", uuid, start_node),
            end_node_element_id: format!("4:{}:{}", uuid, end_node),
        }
    }
}

/// 쿼리 실행 통계
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryStats {
    /// 업데이트 발생 여부
    #[serde(rename = "contains_updates")]
    pub contains_updates: bool,

    /// 생성된 노드 수
    #[serde(rename = "nodes_created")]
    pub nodes_created: u64,

    /// 삭제된 노드 수
    #[serde(rename = "nodes_deleted")]
    pub nodes_deleted: u64,

    /// 설정된 프로퍼티 수
    #[serde(rename = "properties_set")]
    pub properties_set: u64,

    /// 생성된 관계 수
    #[serde(rename = "relationships_created")]
    pub relationships_created: u64,

    /// 삭제된 관계 수
    #[serde(rename = "relationships_deleted")]
    pub relationships_deleted: u64,

    /// 추가된 레이블 수
    #[serde(rename = "labels_added")]
    pub labels_added: u64,

    /// 제거된 레이블 수
    #[serde(rename = "labels_removed")]
    pub labels_removed: u64,

    /// 추가된 인덱스 수
    #[serde(rename = "indexes_added")]
    pub indexes_added: u64,

    /// 제거된 인덱스 수
    #[serde(rename = "indexes_removed")]
    pub indexes_removed: u64,

    /// 추가된 제약조건 수
    #[serde(rename = "constraints_added")]
    pub constraints_added: u64,

    /// 제거된 제약조건 수
    #[serde(rename = "constraints_removed")]
    pub constraints_removed: u64,
}

impl From<&MutationStats> for QueryStats {
    fn from(ms: &MutationStats) -> Self {
        Self {
            nodes_created: ms.nodes_created,
            nodes_deleted: ms.nodes_deleted,
            relationships_created: ms.relationships_created,
            relationships_deleted: ms.relationships_deleted,
            properties_set: ms.properties_set,
            labels_added: ms.labels_added,
            labels_removed: ms.labels_removed,
            indexes_added: ms.indexes_added,
            indexes_removed: ms.indexes_removed,
            constraints_added: ms.constraints_added,
            constraints_removed: ms.constraints_removed,
            contains_updates: ms.has_updates(),
        }
    }
}

impl QueryStats {
    /// 업데이트 여부 계산
    pub fn compute_contains_updates(&mut self) {
        self.contains_updates = self.nodes_created > 0
            || self.nodes_deleted > 0
            || self.properties_set > 0
            || self.relationships_created > 0
            || self.relationships_deleted > 0
            || self.labels_added > 0
            || self.labels_removed > 0
            || self.indexes_added > 0
            || self.indexes_removed > 0
            || self.constraints_added > 0
            || self.constraints_removed > 0;
    }
}

/// 트랜잭션 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInfo {
    /// 만료 시간 (ISO 8601 형식)
    pub expires: String,
}

/// Cypher 에러
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CypherError {
    /// Bolt 호환 에러 코드
    pub code: String,

    /// 에러 메시지
    pub message: String,
}

impl std::fmt::Display for CypherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for CypherError {}

impl CypherError {
    /// 새 에러 생성
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// 구문 에러
    pub fn syntax_error(message: impl Into<String>) -> Self {
        Self::new("Zeta.ClientError.Statement.SyntaxError", message)
    }

    /// 의미 에러
    pub fn semantic_error(message: impl Into<String>) -> Self {
        Self::new("Zeta.ClientError.Statement.SemanticError", message)
    }

    /// 파라미터 누락 에러
    pub fn parameter_missing(param: &str) -> Self {
        Self::new(
            "Zeta.ClientError.Statement.ParameterMissing",
            format!("Expected parameter(s): {}", param),
        )
    }

    /// 트랜잭션 없음 에러
    pub fn transaction_not_found(tx_id: &str) -> Self {
        Self::new(
            "Zeta.ClientError.Transaction.TransactionNotFound",
            format!("Transaction {} not found", tx_id),
        )
    }

    /// 트랜잭션 타임아웃 에러
    pub fn transaction_timeout(tx_id: &str) -> Self {
        Self::new(
            "Zeta.TransientError.Transaction.TransactionTimedOut",
            format!("Transaction {} has timed out", tx_id),
        )
    }

    /// 데이터베이스 없음 에러
    pub fn database_not_found(db_name: &str) -> Self {
        Self::new(
            "Zeta.ClientError.Database.DatabaseNotFound",
            format!("Database '{}' not found", db_name),
        )
    }

    /// 내부 에러
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new("Zeta.DatabaseError.General.UnknownError", message)
    }
}
