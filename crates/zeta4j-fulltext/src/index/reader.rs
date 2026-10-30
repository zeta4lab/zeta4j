//! 인덱스 Reader
//!
//! 인덱스에서 문서를 검색하고 조회하는 기능을 제공합니다.

use tantivy::Term;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, QueryParser, TermQuery};
use tantivy::schema::{IndexRecordOption, Schema, Value};
use tracing::debug;
use zeta4j_common::CanonicalEntityRef;

use super::FulltextIndex;
use crate::error::{FulltextError, Result};
use crate::search::bm25::{SearchResult, TextHighlighter};
use crate::tokenizer::router::DetectedLanguage;

/// 페이지네이션 옵션
#[derive(Debug, Clone, Copy)]
pub struct PaginationOptions {
    /// 건너뛸 결과 수
    pub offset: usize,
    /// 반환할 최대 결과 수
    pub limit: usize,
}

impl PaginationOptions {
    /// 새 페이지네이션 옵션 생성
    pub fn new(offset: usize, limit: usize) -> Self {
        Self { offset, limit }
    }

    /// 페이지 번호로 생성 (0-indexed)
    pub fn page(page: usize, page_size: usize) -> Self {
        Self {
            offset: page * page_size,
            limit: page_size,
        }
    }
}

impl Default for PaginationOptions {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 10,
        }
    }
}

/// 인덱스 Reader
pub struct IndexReader<'a> {
    index: &'a FulltextIndex,
}

impl<'a> IndexReader<'a> {
    /// 새 Reader 생성
    pub(crate) fn new(index: &'a FulltextIndex) -> Self {
        Self { index }
    }

    /// BM25 검색
    ///
    /// Tantivy의 기본 BM25 파라미터(k1=1.2, b=0.75)를 사용하여 검색합니다.
    pub fn search_bm25(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.search_with_config(query, limit, None)
    }

    /// BM25 검색 (후처리 설정 포함)
    ///
    /// # Arguments
    ///
    /// * `query` - 검색 쿼리
    /// * `limit` - 결과 수 제한
    /// * `config` - 후처리 설정 (min_score, normalize_scores). k1, b는 Tantivy에서 적용되지 않음
    ///
    /// # Note
    ///
    /// Tantivy는 BM25 파라미터를 컴파일 타임 상수(k1=1.2, b=0.75)로 사용하므로
    /// 런타임에 k1, b 값을 변경할 수 없습니다. `config`의 k1, b 값은 무시됩니다.
    pub fn search_with_config(
        &self,
        query: &str,
        limit: usize,
        config: Option<&crate::search::bm25::Bm25Config>,
    ) -> Result<Vec<SearchResult>> {
        let schema = self.index.tantivy_index().schema();
        let reader = self.index.tantivy_reader();
        let searcher = reader.searcher();

        // 쿼리 언어 감지
        let lang = self.index.language_router().detect_language(query);
        debug!("Query language detected: {:?}", lang);

        // 검색 필드 결정
        let content_field = schema
            .get_field("content")
            .map_err(|_| FulltextError::FieldNotFound("content".to_string()))?;

        let search_fields = match lang {
            DetectedLanguage::Korean => {
                if let Ok(ko_field) = schema.get_field("content_ko") {
                    vec![content_field, ko_field]
                } else {
                    vec![content_field]
                }
            }
            DetectedLanguage::English => {
                if let Ok(en_field) = schema.get_field("content_en") {
                    vec![content_field, en_field]
                } else {
                    vec![content_field]
                }
            }
            _ => vec![content_field],
        };

        // 쿼리 파서 생성
        let query_parser = QueryParser::for_index(self.index.tantivy_index(), search_fields);
        let parsed_query = query_parser.parse_query(query)?;

        // 검색 실행
        let top_docs = searcher.search(&parsed_query, &TopDocs::with_limit(limit))?;

        // 결과 변환
        let chunk_id_field = schema
            .get_field("chunk_id")
            .map_err(|_| FulltextError::FieldNotFound("chunk_id".to_string()))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc = searcher.doc::<tantivy::TantivyDocument>(doc_address)?;

            let chunk_id = doc
                .get_first(chunk_id_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let content = doc
                .get_first(content_field)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let entity = canonical_entity_ref(&schema, &doc)?;

            results.push(SearchResult {
                chunk_id,
                score,
                content,
                metadata: None,
                highlights: None,
                entity,
            });
        }

        debug!(
            "Search returned {} results for query: {}",
            results.len(),
            query
        );

        // 후처리 적용 (min_score 필터링 및 점수 정규화)
        let results = if let Some(cfg) = config {
            Self::apply_postprocess(results, cfg)
        } else {
            results
        };

        Ok(results)
    }

    /// document_id로 해당 문서의 모든 chunk를 조회
    ///
    /// Stage-based 인덱싱에서 저장된 chunk content를 읽어올 때 사용합니다.
    pub fn get_chunks_by_doc_id(&self, doc_id: u64) -> Result<Vec<SearchResult>> {
        let schema = self.index.tantivy_index().schema();
        let reader = self.index.tantivy_reader();
        let searcher = reader.searcher();

        let doc_id_field = schema
            .get_field("document_id")
            .map_err(|_| FulltextError::FieldNotFound("document_id".to_string()))?;
        let chunk_id_field = schema
            .get_field("chunk_id")
            .map_err(|_| FulltextError::FieldNotFound("chunk_id".to_string()))?;
        let content_field = schema
            .get_field("content")
            .map_err(|_| FulltextError::FieldNotFound("content".to_string()))?;

        let term = Term::from_field_u64(doc_id_field, doc_id);
        let query = TermQuery::new(term, IndexRecordOption::Basic);
        let top_docs = searcher.search(&query, &TopDocs::with_limit(10000))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (_score, doc_address) in top_docs {
            let doc = searcher.doc::<tantivy::TantivyDocument>(doc_address)?;

            let chunk_id = doc
                .get_first(chunk_id_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let content = doc
                .get_first(content_field)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let entity = canonical_entity_ref(&schema, &doc)?;

            results.push(SearchResult {
                chunk_id,
                score: 1.0,
                content,
                metadata: None,
                highlights: None,
                entity,
            });
        }

        Ok(results)
    }

    /// 후처리 적용 (min_score 필터링 및 점수 정규화)
    fn apply_postprocess(
        results: Vec<SearchResult>,
        config: &crate::search::bm25::Bm25Config,
    ) -> Vec<SearchResult> {
        if results.is_empty() {
            return results;
        }

        // 최대 점수 찾기 (정규화용)
        let max_score = results.iter().map(|r| r.score).fold(0.0f32, f32::max);

        results
            .into_iter()
            .filter(|r| r.score >= config.min_score)
            .map(|mut r| {
                if config.normalize_scores && max_score > 0.0 {
                    r.score /= max_score;
                }
                r
            })
            .collect()
    }

    /// 다중 필드 검색
    pub fn search_fields(
        &self,
        query: &str,
        fields: &[&str],
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let schema = self.index.tantivy_index().schema();
        let reader = self.index.tantivy_reader();
        let searcher = reader.searcher();

        // 필드 목록 생성
        let mut search_fields = Vec::new();
        for field_name in fields {
            if let Ok(field) = schema.get_field(field_name) {
                search_fields.push(field);
            }
        }

        if search_fields.is_empty() {
            return Err(FulltextError::FieldNotFound(
                "No valid search fields".to_string(),
            ));
        }

        // 쿼리 파서 생성
        let query_parser = QueryParser::for_index(self.index.tantivy_index(), search_fields);
        let parsed_query = query_parser.parse_query(query)?;

        // 검색 실행
        let top_docs = searcher.search(&parsed_query, &TopDocs::with_limit(limit))?;

        // 결과 변환
        let chunk_id_field = schema
            .get_field("chunk_id")
            .map_err(|_| FulltextError::FieldNotFound("chunk_id".to_string()))?;
        let content_field = schema
            .get_field("content")
            .map_err(|_| FulltextError::FieldNotFound("content".to_string()))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc = searcher.doc::<tantivy::TantivyDocument>(doc_address)?;

            let chunk_id = doc
                .get_first(chunk_id_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let content = doc
                .get_first(content_field)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            results.push(SearchResult {
                chunk_id,
                score,
                content,
                metadata: None,
                highlights: None,
                entity: canonical_entity_ref(&schema, &doc)?,
            });
        }

        Ok(results)
    }

    /// 필터 검색 (테넌트 ID)
    ///
    /// # Security
    ///
    /// tenant_id는 TermQuery로 리터럴 처리되어 쿼리 인젝션을 방지합니다.
    pub fn search_with_tenant(
        &self,
        query: &str,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let schema = self.index.tantivy_index().schema();
        let reader = self.index.tantivy_reader();
        let searcher = reader.searcher();

        let content_field = schema
            .get_field("content")
            .map_err(|_| FulltextError::FieldNotFound("content".to_string()))?;
        let tenant_id_field = schema
            .get_field("tenant_id")
            .map_err(|_| FulltextError::FieldNotFound("tenant_id".to_string()))?;

        // 콘텐츠 쿼리 파싱
        let query_parser = QueryParser::for_index(self.index.tantivy_index(), vec![content_field]);
        let content_query = query_parser.parse_query(query)?;

        // tenant_id를 TermQuery로 리터럴 처리 (쿼리 인젝션 방지)
        let tenant_term = Term::from_field_text(tenant_id_field, tenant_id);
        let tenant_query = TermQuery::new(tenant_term, IndexRecordOption::Basic);

        // BooleanQuery로 AND 조합
        let combined_query = BooleanQuery::new(vec![
            (Occur::Must, content_query),
            (Occur::Must, Box::new(tenant_query)),
        ]);

        // 검색 실행
        let top_docs = searcher.search(&combined_query, &TopDocs::with_limit(limit))?;

        // 결과 변환
        let chunk_id_field = schema
            .get_field("chunk_id")
            .map_err(|_| FulltextError::FieldNotFound("chunk_id".to_string()))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc = searcher.doc::<tantivy::TantivyDocument>(doc_address)?;

            let chunk_id = doc
                .get_first(chunk_id_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let content = doc
                .get_first(content_field)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            results.push(SearchResult {
                chunk_id,
                score,
                content,
                metadata: None,
                highlights: None,
                entity: canonical_entity_ref(&schema, &doc)?,
            });
        }

        Ok(results)
    }

    /// 문서 수 조회
    pub fn num_docs(&self) -> u64 {
        let reader = self.index.tantivy_reader();
        let searcher = reader.searcher();
        searcher.num_docs()
    }

    /// 특정 청크 ID로 문서 조회
    pub fn get_by_chunk_id(&self, chunk_id: u64) -> Result<Option<SearchResult>> {
        let schema = self.index.tantivy_index().schema();
        let reader = self.index.tantivy_reader();
        let searcher = reader.searcher();

        let chunk_id_field = schema
            .get_field("chunk_id")
            .map_err(|_| FulltextError::FieldNotFound("chunk_id".to_string()))?;
        let content_field = schema
            .get_field("content")
            .map_err(|_| FulltextError::FieldNotFound("content".to_string()))?;

        // Term query로 검색
        let term = tantivy::Term::from_field_u64(chunk_id_field, chunk_id);
        let term_query =
            tantivy::query::TermQuery::new(term, tantivy::schema::IndexRecordOption::Basic);

        let top_docs = searcher.search(&term_query, &TopDocs::with_limit(1))?;

        if let Some((score, doc_address)) = top_docs.first() {
            let doc = searcher.doc::<tantivy::TantivyDocument>(*doc_address)?;

            let content = doc
                .get_first(content_field)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            Ok(Some(SearchResult {
                chunk_id,
                score: *score,
                content,
                metadata: None,
                highlights: None,
                entity: canonical_entity_ref(&schema, &doc)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// 페이지네이션 BM25 검색
    ///
    /// # Arguments
    ///
    /// * `query` - 검색 쿼리
    /// * `pagination` - 페이지네이션 옵션 (offset, limit)
    ///
    /// # Note
    ///
    /// 내부적으로 offset+limit 개를 가져온 후 슬라이싱합니다 (Tantivy 제약).
    pub fn search_bm25_paginated(
        &self,
        query: &str,
        pagination: PaginationOptions,
    ) -> Result<Vec<SearchResult>> {
        // offset + limit 개의 결과를 가져온 후 슬라이싱
        let total_fetch = pagination.offset + pagination.limit;
        let results = self.search_bm25(query, total_fetch)?;

        // offset부터 limit개 반환
        Ok(results.into_iter().skip(pagination.offset).collect())
    }

    /// BM25 검색 (하이라이팅 포함)
    ///
    /// # Arguments
    ///
    /// * `query` - 검색 쿼리
    /// * `limit` - 결과 수 제한
    ///
    /// # Returns
    ///
    /// 검색 결과 목록 (각 결과에 하이라이트 정보 포함)
    pub fn search_bm25_with_highlights(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let mut results = self.search_bm25(query, limit)?;
        let highlighter = TextHighlighter::new(query);
        highlighter.apply_to_results(&mut results);
        Ok(results)
    }

    /// 페이지네이션 BM25 검색 (하이라이팅 포함)
    pub fn search_bm25_with_highlights_paginated(
        &self,
        query: &str,
        pagination: PaginationOptions,
    ) -> Result<Vec<SearchResult>> {
        let mut results = self.search_bm25_paginated(query, pagination)?;
        let highlighter = TextHighlighter::new(query);
        highlighter.apply_to_results(&mut results);
        Ok(results)
    }

    /// 페이지네이션 검색 (후처리 설정 포함)
    pub fn search_with_config_paginated(
        &self,
        query: &str,
        pagination: PaginationOptions,
        config: Option<&crate::search::bm25::Bm25Config>,
    ) -> Result<Vec<SearchResult>> {
        let total_fetch = pagination.offset + pagination.limit;
        let results = self.search_with_config(query, total_fetch, config)?;

        Ok(results.into_iter().skip(pagination.offset).collect())
    }

    /// 페이지네이션 테넌트 필터 검색
    pub fn search_with_tenant_paginated(
        &self,
        query: &str,
        tenant_id: &str,
        pagination: PaginationOptions,
    ) -> Result<Vec<SearchResult>> {
        let total_fetch = pagination.offset + pagination.limit;
        let results = self.search_with_tenant(query, tenant_id, total_fetch)?;

        Ok(results.into_iter().skip(pagination.offset).collect())
    }
}

fn canonical_entity_ref(
    schema: &Schema,
    document: &tantivy::TantivyDocument,
) -> Result<CanonicalEntityRef> {
    let field = schema
        .get_field("entity_ref")
        .map_err(|_| FulltextError::FieldNotFound("entity_ref".to_string()))?;
    let encoded = document
        .get_first(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| FulltextError::Schema("document has no canonical entity_ref".to_string()))?;
    let entity: CanonicalEntityRef = serde_json::from_str(encoded)?;
    entity
        .validate()
        .map_err(|message| FulltextError::Schema(message.to_string()))?;
    Ok(entity)
}
