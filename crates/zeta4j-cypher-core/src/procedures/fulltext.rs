// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Fulltext 검색 프로시저
//!
//! Neo4j 호환 Fulltext 검색 프로시저를 제공합니다.
//!
//! - `CALL db.index.fulltext.queryNodes('indexName', 'query') YIELD node, score`
//! - `CALL db.index.fulltext.queryRelationships('indexName', 'query') YIELD relationship, score`
//! - `CALL db.index.fulltext.listAvailableAnalyzers() YIELD analyzer`
//!
//! # 트레이트 기반 추상화
//!
//! `FulltextSearchProvider` 트레이트를 통해 실제 검색 기능을 주입받습니다.
//! zeta4j-cypher만 사용하는 경우 Stub 구현이 사용되며,
//! zeta4j-server에서 zeta4j-fulltext 기반 구현체를 제공합니다.

use std::collections::HashMap;
use std::sync::Arc;

use zeta4j_graph::core::Value;
use zeta4j_graph::engine::GraphEngine;

use super::registry::{
    ArgumentSpec, Procedure, ProcedureError, ProcedureResult, ProcedureSignature, ValueType,
    YieldSpec,
};

// =============================================================================
// Fulltext Search Provider Trait
// =============================================================================

/// Fulltext 검색 결과
#[derive(Debug, Clone)]
pub struct FulltextSearchResult {
    /// 엔티티 ID (노드 또는 엣지)
    pub entity_id: u64,
    /// 검색 점수
    pub score: f32,
    /// 매칭된 콘텐츠 (옵션)
    pub content: Option<String>,
}

/// Fulltext 분석기 정보
#[derive(Debug, Clone)]
pub struct AnalyzerInfo {
    /// 분석기 이름
    pub name: String,
    /// 분석기 설명
    pub description: String,
}

/// Fulltext 검색 Provider 트레이트
///
/// 이 트레이트를 구현하여 실제 전문 검색 기능을 제공합니다.
/// zeta4j-server에서 zeta4j-fulltext를 사용하여 구현합니다.
///
/// # 예제
///
/// ```ignore
/// use zeta4j_cypher::procedures::fulltext::FulltextSearchProvider;
///
/// struct MyFulltextProvider {
///     index: Arc<FulltextIndex>,
/// }
///
/// impl FulltextSearchProvider for MyFulltextProvider {
///     fn query_nodes(&self, engine: &GraphEngine, index_name: &str, query: &str, limit: usize)
///         -> Result<Vec<FulltextSearchResult>, ProcedureError> {
///         let results = self.index.reader().search_bm25(query, limit)?;
///         // 변환...
///     }
/// }
/// ```
pub trait FulltextSearchProvider: Send + Sync {
    /// 노드 검색
    ///
    /// # Arguments
    /// * `index_name` - 전문 검색 인덱스 이름
    /// * `query` - 검색 쿼리
    /// * `limit` - 최대 결과 수
    fn query_nodes(
        &self,
        engine: &GraphEngine,
        index_name: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FulltextSearchResult>, ProcedureError>;

    /// 관계 검색
    ///
    /// # Arguments
    /// * `index_name` - 전문 검색 인덱스 이름
    /// * `query` - 검색 쿼리
    /// * `limit` - 최대 결과 수
    fn query_relationships(
        &self,
        engine: &GraphEngine,
        index_name: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FulltextSearchResult>, ProcedureError>;

    /// 사용 가능한 분석기 목록
    fn list_analyzers(&self) -> Result<Vec<AnalyzerInfo>, ProcedureError>;

    /// 인덱스 새로고침 대기
    fn await_index_refresh(&self) -> Result<(), ProcedureError>;
}

/// Stub Fulltext Provider (기본 구현)
///
/// zeta4j-fulltext 없이 사용할 때 "not implemented" 오류를 반환합니다.
pub struct StubFulltextProvider;

impl FulltextSearchProvider for StubFulltextProvider {
    fn query_nodes(
        &self,
        _engine: &GraphEngine,
        _index_name: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<FulltextSearchResult>, ProcedureError> {
        Err(ProcedureError::NotImplemented(
            "Fulltext search requires FulltextSearchProvider. Use Standard edition or higher."
                .to_string(),
        ))
    }

    fn query_relationships(
        &self,
        _engine: &GraphEngine,
        _index_name: &str,
        _query: &str,
        _limit: usize,
    ) -> Result<Vec<FulltextSearchResult>, ProcedureError> {
        Err(ProcedureError::NotImplemented(
            "Fulltext search requires FulltextSearchProvider. Use Standard edition or higher."
                .to_string(),
        ))
    }

    fn list_analyzers(&self) -> Result<Vec<AnalyzerInfo>, ProcedureError> {
        // 기본 분석기 목록 반환
        Ok(vec![
            AnalyzerInfo {
                name: "standard".to_string(),
                description: "Standard analyzer (default)".to_string(),
            },
            AnalyzerInfo {
                name: "korean".to_string(),
                description: "Korean morphological analyzer (Lindera)".to_string(),
            },
            AnalyzerInfo {
                name: "english".to_string(),
                description: "English analyzer with stemming".to_string(),
            },
        ])
    }

    fn await_index_refresh(&self) -> Result<(), ProcedureError> {
        // Stub에서는 즉시 반환
        Ok(())
    }
}

// =============================================================================
// Fulltext Procedures (Provider 기반)
// =============================================================================

/// db.index.fulltext.queryNodes 프로시저
///
/// 전문 검색 인덱스에서 노드를 검색합니다.
pub struct FulltextQueryNodesProcedure {
    provider: Arc<dyn FulltextSearchProvider>,
}

impl FulltextQueryNodesProcedure {
    /// Provider를 사용하는 프로시저 생성
    pub fn new(provider: Arc<dyn FulltextSearchProvider>) -> Self {
        Self { provider }
    }

    /// Stub provider를 사용하는 프로시저 생성
    pub fn stub() -> Self {
        Self {
            provider: Arc::new(StubFulltextProvider),
        }
    }
}

impl Procedure for FulltextQueryNodesProcedure {
    fn name(&self) -> &str {
        "db.index.fulltext.queryNodes"
    }

    fn signature(&self) -> ProcedureSignature {
        ProcedureSignature {
            arguments: vec![
                ArgumentSpec {
                    name: "indexName".to_string(),
                    value_type: ValueType::String,
                    optional: false,
                },
                ArgumentSpec {
                    name: "queryString".to_string(),
                    value_type: ValueType::String,
                    optional: false,
                },
            ],
            yields: vec![
                YieldSpec {
                    name: "node".to_string(),
                    value_type: ValueType::Node,
                },
                YieldSpec {
                    name: "score".to_string(),
                    value_type: ValueType::Float,
                },
            ],
            description: "Query nodes in a fulltext index".to_string(),
        }
    }

    fn execute(
        &self,
        args: Vec<Value>,
        _options: Option<HashMap<String, Value>>,
        engine: &GraphEngine,
    ) -> Result<ProcedureResult, ProcedureError> {
        // 인자 파싱
        let index_name = args
            .first()
            .and_then(|v| v.as_string())
            .ok_or_else(|| ProcedureError::MissingArgument("indexName".to_string()))?;

        let query = args
            .get(1)
            .and_then(|v| v.as_string())
            .ok_or_else(|| ProcedureError::MissingArgument("queryString".to_string()))?;

        // Provider를 통해 검색 실행
        let results = self.provider.query_nodes(engine, index_name, query, 100)?;

        // 결과 변환
        let rows: Vec<Vec<Value>> = results
            .into_iter()
            .map(|r| vec![Value::Node(r.entity_id), Value::Float(r.score as f64)])
            .collect();

        Ok(ProcedureResult {
            columns: vec!["node".to_string(), "score".to_string()],
            rows,
        })
    }
}

/// db.index.fulltext.queryRelationships 프로시저
pub struct FulltextQueryRelationshipsProcedure {
    provider: Arc<dyn FulltextSearchProvider>,
}

impl FulltextQueryRelationshipsProcedure {
    pub fn new(provider: Arc<dyn FulltextSearchProvider>) -> Self {
        Self { provider }
    }

    pub fn stub() -> Self {
        Self {
            provider: Arc::new(StubFulltextProvider),
        }
    }
}

impl Procedure for FulltextQueryRelationshipsProcedure {
    fn name(&self) -> &str {
        "db.index.fulltext.queryRelationships"
    }

    fn signature(&self) -> ProcedureSignature {
        ProcedureSignature {
            arguments: vec![
                ArgumentSpec {
                    name: "indexName".to_string(),
                    value_type: ValueType::String,
                    optional: false,
                },
                ArgumentSpec {
                    name: "queryString".to_string(),
                    value_type: ValueType::String,
                    optional: false,
                },
            ],
            yields: vec![
                YieldSpec {
                    name: "relationship".to_string(),
                    value_type: ValueType::Edge,
                },
                YieldSpec {
                    name: "score".to_string(),
                    value_type: ValueType::Float,
                },
            ],
            description: "Query relationships in a fulltext index".to_string(),
        }
    }

    fn execute(
        &self,
        args: Vec<Value>,
        _options: Option<HashMap<String, Value>>,
        engine: &GraphEngine,
    ) -> Result<ProcedureResult, ProcedureError> {
        let index_name = args
            .first()
            .and_then(|v| v.as_string())
            .ok_or_else(|| ProcedureError::MissingArgument("indexName".to_string()))?;

        let query = args
            .get(1)
            .and_then(|v| v.as_string())
            .ok_or_else(|| ProcedureError::MissingArgument("queryString".to_string()))?;

        let results = self
            .provider
            .query_relationships(engine, index_name, query, 100)?;

        let rows: Vec<Vec<Value>> = results
            .into_iter()
            .map(|r| vec![Value::Edge(r.entity_id), Value::Float(r.score as f64)])
            .collect();

        Ok(ProcedureResult {
            columns: vec!["relationship".to_string(), "score".to_string()],
            rows,
        })
    }
}

/// db.index.fulltext.listAvailableAnalyzers 프로시저
pub struct FulltextListAnalyzersProcedure {
    provider: Arc<dyn FulltextSearchProvider>,
}

impl FulltextListAnalyzersProcedure {
    pub fn new(provider: Arc<dyn FulltextSearchProvider>) -> Self {
        Self { provider }
    }

    pub fn stub() -> Self {
        Self {
            provider: Arc::new(StubFulltextProvider),
        }
    }
}

impl Procedure for FulltextListAnalyzersProcedure {
    fn name(&self) -> &str {
        "db.index.fulltext.listAvailableAnalyzers"
    }

    fn signature(&self) -> ProcedureSignature {
        ProcedureSignature {
            arguments: vec![],
            yields: vec![
                YieldSpec {
                    name: "analyzer".to_string(),
                    value_type: ValueType::String,
                },
                YieldSpec {
                    name: "description".to_string(),
                    value_type: ValueType::String,
                },
            ],
            description: "List available fulltext analyzers".to_string(),
        }
    }

    fn execute(
        &self,
        _args: Vec<Value>,
        _options: Option<HashMap<String, Value>>,
        _engine: &GraphEngine,
    ) -> Result<ProcedureResult, ProcedureError> {
        let analyzers = self.provider.list_analyzers()?;

        let rows: Vec<Vec<Value>> = analyzers
            .into_iter()
            .map(|a| vec![Value::String(a.name), Value::String(a.description)])
            .collect();

        Ok(ProcedureResult {
            columns: vec!["analyzer".to_string(), "description".to_string()],
            rows,
        })
    }
}

/// db.index.fulltext.awaitEventuallyConsistentIndexRefresh 프로시저
pub struct FulltextAwaitIndexRefreshProcedure {
    provider: Arc<dyn FulltextSearchProvider>,
}

impl FulltextAwaitIndexRefreshProcedure {
    pub fn new(provider: Arc<dyn FulltextSearchProvider>) -> Self {
        Self { provider }
    }

    pub fn stub() -> Self {
        Self {
            provider: Arc::new(StubFulltextProvider),
        }
    }
}

impl Procedure for FulltextAwaitIndexRefreshProcedure {
    fn name(&self) -> &str {
        "db.index.fulltext.awaitEventuallyConsistentIndexRefresh"
    }

    fn signature(&self) -> ProcedureSignature {
        ProcedureSignature {
            arguments: vec![],
            yields: vec![],
            description: "Wait for eventual consistency of fulltext indexes".to_string(),
        }
    }

    fn execute(
        &self,
        _args: Vec<Value>,
        _options: Option<HashMap<String, Value>>,
        _engine: &GraphEngine,
    ) -> Result<ProcedureResult, ProcedureError> {
        self.provider.await_index_refresh()?;
        Ok(ProcedureResult {
            columns: vec![],
            rows: vec![],
        })
    }
}

// =============================================================================
// Registration
// =============================================================================

/// Stub Fulltext 프로시저들을 레지스트리에 등록
pub fn register_fulltext_procedures(registry: &mut super::registry::ProcedureRegistry) {
    registry.register(Arc::new(FulltextQueryNodesProcedure::stub()));
    registry.register(Arc::new(FulltextQueryRelationshipsProcedure::stub()));
    registry.register(Arc::new(FulltextListAnalyzersProcedure::stub()));
    registry.register(Arc::new(FulltextAwaitIndexRefreshProcedure::stub()));
}

/// Provider를 사용하는 Fulltext 프로시저들을 레지스트리에 등록
pub fn register_fulltext_procedures_with_provider(
    registry: &mut super::registry::ProcedureRegistry,
    provider: Arc<dyn FulltextSearchProvider>,
) {
    registry.register(Arc::new(FulltextQueryNodesProcedure::new(provider.clone())));
    registry.register(Arc::new(FulltextQueryRelationshipsProcedure::new(
        provider.clone(),
    )));
    registry.register(Arc::new(FulltextListAnalyzersProcedure::new(
        provider.clone(),
    )));
    registry.register(Arc::new(FulltextAwaitIndexRefreshProcedure::new(provider)));
}
