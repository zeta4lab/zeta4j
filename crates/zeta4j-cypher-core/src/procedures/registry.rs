//! Phase 3: 프로시저 레지스트리
//!
//! 프로시저 등록 및 조회를 담당합니다.
//!
//! # 예제
//!
//! ```ignore
//! let registry = ProcedureRegistry::new();
//! let proc = registry.get("db.index.vector.queryNodes").unwrap();
//! let result = proc.execute(args, None, &engine)?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

use zeta4j_common::{ZetaError, codes};
use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;

/// 프로시저 에러
#[derive(Error, Debug)]
pub enum ProcedureError {
    #[error("Procedure not found: {0}")]
    NotFound(String),

    #[error("Invalid argument at position {position}: expected {expected}")]
    ArgumentType {
        position: usize,
        expected: &'static str,
    },

    #[error("Missing required argument: {0}")]
    MissingArgument(String),

    #[error("Index not found: {0}")]
    IndexNotFound(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),
}

// =============================================================================
// From<ProcedureError> for ZetaError
// =============================================================================

impl From<ProcedureError> for ZetaError {
    fn from(err: ProcedureError) -> Self {
        match err {
            ProcedureError::NotFound(name) => ZetaError::Procedure {
                code: codes::cypher::PROCEDURE_NOT_FOUND,
                message: format!("Procedure not found: {}", name),
            },
            ProcedureError::ArgumentType { position, expected } => ZetaError::Procedure {
                code: codes::cypher::PROCEDURE_INVALID_ARGS,
                message: format!(
                    "Invalid argument at position {}: expected {}",
                    position, expected
                ),
            },
            ProcedureError::MissingArgument(arg) => ZetaError::Procedure {
                code: codes::cypher::PROCEDURE_INVALID_ARGS,
                message: format!("Missing required argument: {}", arg),
            },
            ProcedureError::IndexNotFound(name) => ZetaError::Index {
                code: codes::graph::INDEX_NOT_FOUND,
                message: format!("Index not found: {}", name),
            },
            ProcedureError::Execution(msg) => ZetaError::Procedure {
                code: codes::cypher::PROCEDURE_EXECUTION_FAILED,
                message: msg,
            },
            ProcedureError::Internal(msg) => ZetaError::Internal {
                code: codes::graph::SYSTEM_ERROR,
                message: msg,
            },
            ProcedureError::NotImplemented(feature) => ZetaError::NotImplemented {
                code: codes::graph::NOT_IMPLEMENTED,
                feature,
            },
        }
    }
}

/// 프로시저 실행 결과
#[derive(Debug, Clone)]
pub struct ProcedureResult {
    /// 컬럼 이름들
    pub columns: Vec<String>,
    /// 행 데이터 (각 행은 컬럼 순서대로 값을 가짐)
    pub rows: Vec<Vec<Value>>,
}

impl ProcedureResult {
    /// 빈 결과 생성
    pub fn empty(columns: Vec<String>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
        }
    }

    /// 단일 행 결과 생성
    pub fn single(columns: Vec<String>, row: Vec<Value>) -> Self {
        Self {
            columns,
            rows: vec![row],
        }
    }

    /// 행 개수
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// 값 타입 (시그니처에서 사용)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    String,
    Integer,
    Float,
    Boolean,
    Vector,
    Map,
    Array,
    Node,
    Edge,
    Path,
    Any,
}

impl ValueType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ValueType::String => "String",
            ValueType::Integer => "Integer",
            ValueType::Float => "Float",
            ValueType::Boolean => "Boolean",
            ValueType::Vector => "Vector",
            ValueType::Map => "Map",
            ValueType::Array => "Array",
            ValueType::Node => "Node",
            ValueType::Edge => "Edge",
            ValueType::Path => "Path",
            ValueType::Any => "Any",
        }
    }
}

/// 인수 명세
#[derive(Debug, Clone)]
pub struct ArgumentSpec {
    /// 인수 이름
    pub name: String,
    /// 인수 타입
    pub value_type: ValueType,
    /// 선택적 여부
    pub optional: bool,
}

/// YIELD 명세
#[derive(Debug, Clone)]
pub struct YieldSpec {
    /// 변수 이름
    pub name: String,
    /// 반환 타입
    pub value_type: ValueType,
}

/// 프로시저 시그니처
#[derive(Debug, Clone)]
pub struct ProcedureSignature {
    /// 인수 목록
    pub arguments: Vec<ArgumentSpec>,
    /// YIELD 변수 목록
    pub yields: Vec<YieldSpec>,
    /// 프로시저 설명
    pub description: String,
}

/// 프로시저 트레이트
///
/// 모든 프로시저는 이 트레이트를 구현해야 합니다.
pub trait Procedure: Send + Sync {
    /// 프로시저 이름 (e.g., "db.index.vector.queryNodes")
    fn name(&self) -> &str;

    /// 프로시저 시그니처
    fn signature(&self) -> ProcedureSignature;

    /// 프로시저 실행
    ///
    /// # Arguments
    ///
    /// * `args` - 인수 값들
    /// * `options` - 옵션 맵 (마지막 인수가 맵인 경우)
    /// * `engine` - 그래프 엔진 참조
    ///
    /// # Returns
    ///
    /// 프로시저 실행 결과 또는 에러
    fn execute(
        &self,
        args: Vec<Value>,
        options: Option<HashMap<String, Value>>,
        engine: &GraphEngine,
    ) -> Result<ProcedureResult, ProcedureError>;
}

/// 프로시저 레지스트리
///
/// 등록된 프로시저들을 관리합니다.
#[derive(Clone)]
pub struct ProcedureRegistry {
    procedures: HashMap<String, Arc<dyn Procedure>>,
}

impl Default for ProcedureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcedureRegistry {
    /// 새 레지스트리 생성 (기본 프로시저 등록)
    pub fn new() -> Self {
        let mut registry = Self {
            procedures: HashMap::new(),
        };

        // 벡터 검색 프로시저 등록
        registry.register(Arc::new(super::vector::VectorQueryNodesProcedure::stub()));
        registry.register(Arc::new(super::vector::VectorRangeSearchProcedure));
        registry.register(Arc::new(super::vector::HybridSearchProcedure));

        // 그래프 인식 벡터 검색 프로시저 등록
        registry.register(Arc::new(super::vector::GraphAwareVectorQueryProcedure));
        registry.register(Arc::new(super::vector::InCommunityVectorQueryProcedure));

        // Fulltext 검색 프로시저 등록
        super::fulltext::register_fulltext_procedures(&mut registry);

        registry
    }

    /// 프로시저 등록
    pub fn register(&mut self, proc: Arc<dyn Procedure>) {
        self.procedures.insert(proc.name().to_string(), proc);
    }

    /// 프로시저 조회
    pub fn get(&self, name: &str) -> Option<Arc<dyn Procedure>> {
        self.procedures.get(name).cloned()
    }

    /// 모든 프로시저 이름 조회
    pub fn list_names(&self) -> Vec<&str> {
        self.procedures.keys().map(|s| s.as_str()).collect()
    }

    /// 프로시저 개수
    pub fn len(&self) -> usize {
        self.procedures.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.procedures.is_empty()
    }
}
