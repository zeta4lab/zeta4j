// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Phase 3: 프로시저 시스템 모듈
//!
//! Cypher의 `CALL procedure() YIELD ...` 문법을 지원하는 프로시저 시스템입니다.
//!
//! # 참고
//!
//! 실제 벡터 검색, 전문 검색 기능은 각각 zeta4j-vector,
//! zeta4j-fulltext 크레이트에서 제공됩니다.
//! zeta4j-cypher만 사용하는 경우 해당 프로시저들은 "not implemented" 오류를 반환합니다.
//!
//! # 지원 프로시저
//!
//! ## 벡터 검색 (Stub)
//!
//! - `db.index.vector.queryNodes` - 벡터 인덱스 KNN 검색
//! - `db.index.vector.rangeSearch` - 벡터 범위 검색 (threshold 기반)
//! - `db.index.hybrid.search` - 하이브리드 검색 (벡터 + 그래프)
//! - `db.index.vector.queryNodes.graphAware` - 그래프 인식 벡터 검색
//! - `db.index.vector.queryNodes.inCommunity` - 커뮤니티 내 벡터 검색
//!
//! ## 전문 검색 (Stub)
//!
//! - `db.index.fulltext.queryNodes` - 전문 검색 인덱스에서 노드 검색
//! - `db.index.fulltext.queryRelationships` - 전문 검색 인덱스에서 관계 검색
//! - `db.index.fulltext.listAvailableAnalyzers` - 사용 가능한 분석기 목록
//! - `db.index.fulltext.awaitEventuallyConsistentIndexRefresh` - 인덱스 새로고침 대기
//!
//! ## 임베딩 설정 (Stub)
//!

pub mod fulltext;
pub mod registry;
pub mod vector;

pub use fulltext::{
    AnalyzerInfo,
    FulltextAwaitIndexRefreshProcedure,
    FulltextListAnalyzersProcedure,
    // Procedures
    FulltextQueryNodesProcedure,
    FulltextQueryRelationshipsProcedure,
    // Trait & Types
    FulltextSearchProvider,
    FulltextSearchResult,
    StubFulltextProvider,
    // Registration
    register_fulltext_procedures,
    register_fulltext_procedures_with_provider,
};
pub use registry::{
    ArgumentSpec, Procedure, ProcedureError, ProcedureRegistry, ProcedureResult,
    ProcedureSignature, ValueType, YieldSpec,
};
pub use vector::{
    GraphAwareVectorQueryProcedure, HybridSearchProcedure, InCommunityVectorQueryProcedure,
    VectorCommunitySearchProcedure, VectorGraphAwareSearchProcedure, VectorHybridSearchProcedure,
    VectorQueryNodesProcedure, VectorRangeSearchProcedure, VectorSearchProvider,
    VectorSearchResult,
};

pub fn register_vector_query_with_provider(
    registry: &mut ProcedureRegistry,
    provider: std::sync::Arc<dyn VectorSearchProvider>,
) {
    registry.register(std::sync::Arc::new(VectorQueryNodesProcedure::new(
        provider,
    )));
}
