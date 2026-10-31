// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! QueryExecutor 구조체 및 빌더 메서드.

use std::sync::Arc;

use zeta4j_graph::engine::GraphEngine;

use zeta4j_cypher_core::procedures::{FulltextSearchProvider, ProcedureRegistry};
use zeta4j_cypher_core::services::AdminService;

/// Cypher 쿼리 실행기
///
/// 파싱된 Cypher AST를 실행하여 그래프 데이터베이스를 조회/변경합니다.
/// GraphEngine을 사용하여 실제 데이터 조작을 수행합니다.
///
/// # 주요 메서드
///
/// - [`execute`](Self::execute): 단순 쿼리 실행 (행 반환)
/// - [`execute_with_parameters`](Self::execute_with_parameters): 파라미터와 함께 실행
/// - [`execute_with_result`](Self::execute_with_result): EXPLAIN/ANALYZE 지원 (권장)
/// - [`execute_with_result_and_params`](Self::execute_with_result_and_params): 파라미터 + EXPLAIN/ANALYZE
///
/// # 쿼리 캐시
///
/// v1.15.0부터 쿼리 파싱 캐시와 실행 계획 캐시를 지원합니다.
/// `with_query_cache()`로 활성화할 수 있습니다.
pub struct QueryExecutor {
    /// 그래프 엔진 참조 (실제 데이터 조작 수행)
    pub(super) engine: Arc<GraphEngine>,
    /// canonical admin service (관리 명령용, 선택적)
    pub(super) admin_service: Option<Arc<dyn AdminService>>,
    /// 프로시저 레지스트리 (CALL 프로시저용)
    pub(super) procedure_registry: Arc<ProcedureRegistry>,
    /// 현재 사용자 이름 (SHOW CURRENT USER용)
    pub(super) current_user: Option<String>,
    /// 최대 결과 크기 (0 = 무제한)
    pub(super) max_result_size: usize,
    /// 쿼리 타임아웃 (밀리초, 0 = 무제한)
    pub(super) timeout_ms: u64,
    /// 인덱스 메타데이터 캐시 (L1 moka + L2 redb)
    /// Arc로 래핑하여 여러 쿼리 실행기 간 공유 가능
    pub(super) index_cache: Arc<zeta4j_cache::IndexCache>,
    /// 쿼리 캐시 (파싱 + 실행 계획)
    pub(super) query_cache: Option<Arc<crate::cache::ConcurrentQueryCacheManager>>,
}

impl QueryExecutor {
    /// 새 QueryExecutor 생성
    ///
    /// # 인자
    ///
    /// * `engine` - GraphEngine의 Arc 참조
    pub fn new(engine: Arc<GraphEngine>) -> Self {
        Self {
            engine,
            admin_service: None,
            procedure_registry: Arc::new(ProcedureRegistry::new()),
            current_user: None,
            max_result_size: 0, // 0 = 무제한
            timeout_ms: 0,      // 0 = 무제한
            index_cache: Arc::new(zeta4j_cache::IndexCache::l1_only()),
            query_cache: None,
        }
    }

    /// 쿼리 설정 적용 (빌더 패턴)
    ///
    /// core.toml의 [query] 섹션 설정을 적용합니다.
    pub fn with_query_config(mut self, max_result_size: usize, timeout_ms: u64) -> Self {
        self.max_result_size = max_result_size;
        self.timeout_ms = timeout_ms;
        self
    }

    /// 최대 결과 크기 설정
    pub fn set_max_result_size(&mut self, max_result_size: usize) {
        self.max_result_size = max_result_size;
    }

    /// 쿼리 타임아웃 설정 (밀리초)
    pub fn set_timeout_ms(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
    }

    /// canonical admin service 설정 (빌더 패턴)
    pub fn with_admin_service(mut self, admin_service: Arc<dyn AdminService>) -> Self {
        self.admin_service = Some(admin_service);
        self
    }

    /// canonical admin service 설정 (mutable reference)
    pub fn set_admin_service(&mut self, admin_service: Arc<dyn AdminService>) {
        self.admin_service = Some(admin_service);
    }

    /// canonical admin service 반환
    pub fn admin_service(&self) -> Option<&Arc<dyn AdminService>> {
        self.admin_service.as_ref()
    }

    /// Fulltext 검색 Provider 설정 (빌더 패턴)
    ///
    /// Fulltext 프로시저(db.index.fulltext.*)를 실행하려면 설정해야 합니다.
    pub fn with_fulltext_provider(mut self, provider: Arc<dyn FulltextSearchProvider>) -> Self {
        use zeta4j_cypher_core::procedures::register_fulltext_procedures_with_provider;
        let mut registry = self.procedure_registry.as_ref().clone();
        register_fulltext_procedures_with_provider(&mut registry, provider);
        self.procedure_registry = Arc::new(registry);
        self
    }

    /// Fulltext 검색 Provider 설정 (mutable reference)
    pub fn set_fulltext_provider(&mut self, provider: Arc<dyn FulltextSearchProvider>) {
        use zeta4j_cypher_core::procedures::register_fulltext_procedures_with_provider;
        let mut registry = self.procedure_registry.as_ref().clone();
        register_fulltext_procedures_with_provider(&mut registry, provider);
        self.procedure_registry = Arc::new(registry);
    }

    pub fn with_vector_provider(
        mut self,
        provider: Arc<dyn zeta4j_cypher_core::procedures::VectorSearchProvider>,
    ) -> Self {
        let mut registry = self.procedure_registry.as_ref().clone();
        zeta4j_cypher_core::procedures::register_vector_query_with_provider(
            &mut registry,
            provider,
        );
        self.procedure_registry = Arc::new(registry);
        self
    }

    pub fn set_vector_provider(
        &mut self,
        provider: Arc<dyn zeta4j_cypher_core::procedures::VectorSearchProvider>,
    ) {
        let mut registry = self.procedure_registry.as_ref().clone();
        zeta4j_cypher_core::procedures::register_vector_query_with_provider(
            &mut registry,
            provider,
        );
        self.procedure_registry = Arc::new(registry);
    }

    /// 프로시저 레지스트리 반환
    pub fn procedure_registry(&self) -> &Arc<ProcedureRegistry> {
        &self.procedure_registry
    }

    /// 현재 사용자 설정 (빌더 패턴)
    ///
    /// SHOW CURRENT USER 명령을 위해 설정해야 합니다.
    pub fn with_current_user(mut self, username: String) -> Self {
        self.current_user = Some(username);
        self
    }

    /// 현재 사용자 설정 (mutable reference)
    pub fn set_current_user(&mut self, username: String) {
        self.current_user = Some(username);
    }

    /// 현재 사용자 반환
    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    /// 공유 인덱스 캐시 설정 (권장)
    ///
    /// 여러 쿼리 실행기 간 인덱스 캐시를 공유합니다.
    /// 서버 부트스트랩에서 생성된 캐시를 전달하면 잠금 충돌 없이 L2 캐시 사용 가능.
    pub fn with_shared_index_cache(mut self, cache: Arc<zeta4j_cache::IndexCache>) -> Self {
        self.index_cache = cache;
        self
    }

    /// 인덱스 캐시 반환
    pub fn index_cache(&self) -> &zeta4j_cache::IndexCache {
        &self.index_cache
    }

    // =========================================================================
    // 쿼리 캐시 설정 및 관리
    // =========================================================================

    /// 쿼리 캐시 설정 (빌더 패턴)
    ///
    /// 쿼리 파싱 캐시와 실행 계획 캐시를 활성화합니다.
    /// 반복 쿼리 실행 시 파싱/플래닝 비용을 절약할 수 있습니다.
    ///
    /// # 예제
    ///
    /// ```ignore
    /// use zeta4j_cypher_core::cache::CacheConfig;
    ///
    /// let executor = QueryExecutor::new(engine)
    ///     .with_query_cache(CacheConfig::default());
    /// ```
    pub fn with_query_cache(mut self, config: crate::cache::CacheConfig) -> Self {
        self.query_cache = Some(Arc::new(crate::cache::ConcurrentQueryCacheManager::new(
            config,
        )));
        self
    }

    /// 쿼리 캐시 직접 설정 (Arc 공유)
    ///
    /// 여러 QueryExecutor 인스턴스가 동일한 캐시를 공유할 수 있습니다.
    pub fn with_shared_query_cache(
        mut self,
        cache: Arc<crate::cache::ConcurrentQueryCacheManager>,
    ) -> Self {
        self.query_cache = Some(cache);
        self
    }

    /// 쿼리 캐시 설정 (mutable reference)
    pub fn set_query_cache(&mut self, config: crate::cache::CacheConfig) {
        self.query_cache = Some(Arc::new(crate::cache::ConcurrentQueryCacheManager::new(
            config,
        )));
    }

    /// 쿼리 캐시 반환
    pub fn query_cache(&self) -> Option<&Arc<crate::cache::ConcurrentQueryCacheManager>> {
        self.query_cache.as_ref()
    }

    /// 쿼리 캐시 통계 반환
    pub fn query_cache_stats(&self) -> Option<crate::cache::QueryCacheStats> {
        self.query_cache.as_ref().map(|c| c.stats())
    }

    /// 특정 라벨 관련 쿼리 플랜 캐시 무효화
    ///
    /// 인덱스 생성/삭제 시 호출되어 해당 라벨을 사용하는 쿼리 플랜을 무효화합니다.
    /// schema.rs의 인덱스 명령에서 자동으로 호출됩니다.
    pub fn invalidate_query_cache_by_label(&self, label: &str) {
        if let Some(cache) = &self.query_cache {
            cache.invalidate_by_label(label);
        }
    }

    /// 전체 쿼리 캐시 무효화
    ///
    /// 스키마 변경, 제약조건 변경 등 전체 무효화가 필요한 경우 호출합니다.
    /// schema.rs의 DDL 명령에서 자동으로 호출됩니다.
    pub fn clear_query_cache(&self) {
        if let Some(cache) = &self.query_cache {
            cache.invalidate_all();
        }
    }
}
