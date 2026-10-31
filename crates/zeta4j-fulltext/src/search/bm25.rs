// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! BM25 검색기
//!
//! Tantivy의 BM25 알고리즘을 활용한 검색을 제공합니다.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeta4j_common::CanonicalEntityRef;

/// 하이라이트 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Highlight {
    /// 시작 오프셋 (바이트)
    pub start: usize,
    /// 종료 오프셋 (바이트)
    pub end: usize,
    /// 하이라이트된 텍스트
    pub text: String,
}

impl Highlight {
    /// 새 하이라이트 생성
    pub fn new(start: usize, end: usize, text: impl Into<String>) -> Self {
        Self {
            start,
            end,
            text: text.into(),
        }
    }
}

/// 검색 결과
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// 청크 ID
    pub chunk_id: u64,
    /// 검색 점수
    pub score: f32,
    /// 본문 내용 (옵션)
    pub content: Option<String>,
    /// 추가 메타데이터
    pub metadata: Option<HashMap<String, String>>,
    /// 하이라이트 정보 (옵션)
    #[serde(default)]
    pub highlights: Option<Vec<Highlight>>,
    /// Canonical source entity.
    pub entity: CanonicalEntityRef,
}

impl SearchResult {
    /// 새 검색 결과 생성
    pub fn new(chunk_id: u64, score: f32, entity: CanonicalEntityRef) -> Self {
        Self {
            chunk_id,
            score,
            content: None,
            metadata: None,
            highlights: None,
            entity,
        }
    }

    /// 내용 설정
    pub fn with_content(mut self, content: String) -> Self {
        self.content = Some(content);
        self
    }

    /// 메타데이터 설정
    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// 하이라이트 설정
    pub fn with_highlights(mut self, highlights: Vec<Highlight>) -> Self {
        self.highlights = Some(highlights);
        self
    }

    /// 메타데이터 추가
    pub fn add_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value.into());
    }
}

/// BM25 검색기 설정
///
/// # 파라미터 제한사항
///
/// `k1`, `b` 파라미터는 **참조용**입니다. Tantivy는 BM25 파라미터를
/// 컴파일 타임 상수(k1=1.2, b=0.75)로 사용하므로 런타임 변경이 불가합니다.
///
/// 실제 검색에 적용되는 옵션:
/// - `min_score`: 최소 점수 필터링
/// - `normalize_scores`: 점수 정규화 (0~1)
#[derive(Debug, Clone)]
pub struct Bm25Config {
    /// k1 파라미터 (참조용, Tantivy 기본값: 1.2)
    ///
    /// Tantivy는 이 값을 런타임에 적용하지 않습니다.
    pub k1: f64,
    /// b 파라미터 (참조용, Tantivy 기본값: 0.75)
    ///
    /// Tantivy는 이 값을 런타임에 적용하지 않습니다.
    pub b: f64,
    /// 최소 점수 (실제 적용)
    ///
    /// 검색 결과에서 이 점수 미만의 문서를 필터링합니다.
    pub min_score: f32,
    /// 점수 정규화 여부 (실제 적용)
    ///
    /// true이면 검색 결과 점수를 0~1 범위로 정규화합니다.
    pub normalize_scores: bool,
}

impl Default for Bm25Config {
    fn default() -> Self {
        Self {
            k1: 1.2,
            b: 0.75,
            min_score: 0.0,
            normalize_scores: true,
        }
    }
}

impl Bm25Config {
    /// 높은 정밀도 설정
    ///
    /// `min_score`를 0.1로 설정하여 낮은 점수의 결과를 필터링합니다.
    ///
    /// **Note:** k1, b 값은 참조용이며 Tantivy에 적용되지 않습니다.
    pub fn high_precision() -> Self {
        Self {
            k1: 1.5,
            b: 0.75,
            min_score: 0.1,
            normalize_scores: true,
        }
    }

    /// 높은 재현율 설정
    ///
    /// `min_score`를 0.0으로 설정하여 모든 결과를 반환합니다.
    ///
    /// **Note:** k1, b 값은 참조용이며 Tantivy에 적용되지 않습니다.
    pub fn high_recall() -> Self {
        Self {
            k1: 1.0,
            b: 0.5,
            min_score: 0.0,
            normalize_scores: false,
        }
    }

    /// k1 파라미터 설정 (참조용)
    ///
    /// **Warning:** Tantivy는 이 값을 런타임에 적용하지 않습니다.
    /// 기본값 1.2가 항상 사용됩니다.
    pub fn with_k1(mut self, k1: f64) -> Self {
        self.k1 = k1;
        self
    }

    /// b 파라미터 설정 (참조용)
    ///
    /// **Warning:** Tantivy는 이 값을 런타임에 적용하지 않습니다.
    /// 기본값 0.75가 항상 사용됩니다.
    pub fn with_b(mut self, b: f64) -> Self {
        self.b = b;
        self
    }

    /// 최소 점수 설정
    ///
    /// 검색 결과에서 이 점수 미만의 문서를 필터링합니다.
    /// 이 설정은 실제로 적용됩니다.
    pub fn with_min_score(mut self, min_score: f32) -> Self {
        self.min_score = min_score;
        self
    }

    /// 점수 정규화 설정
    ///
    /// true이면 검색 결과 점수를 0~1 범위로 정규화합니다.
    /// 이 설정은 실제로 적용됩니다.
    pub fn with_normalize_scores(mut self, normalize: bool) -> Self {
        self.normalize_scores = normalize;
        self
    }
}

/// BM25 검색기
///
/// Tantivy 인덱스에 대한 BM25 검색을 수행합니다.
/// 내부적으로 `IndexReader`를 사용합니다.
pub struct Bm25Searcher {
    config: Bm25Config,
}

impl Bm25Searcher {
    /// 새 BM25 검색기 생성
    pub fn new(config: Bm25Config) -> Self {
        Self { config }
    }

    /// 기본 설정으로 생성
    pub fn with_default_config() -> Self {
        Self::new(Bm25Config::default())
    }

    /// 설정 반환
    pub fn config(&self) -> &Bm25Config {
        &self.config
    }

    /// 검색 결과 후처리 (정규화 및 필터링)
    pub fn postprocess_results(&self, results: Vec<SearchResult>) -> Vec<SearchResult> {
        if results.is_empty() {
            return results;
        }

        // 최대 점수 찾기
        let max_score = results.iter().map(|r| r.score).fold(0.0f32, f32::max);

        results
            .into_iter()
            .filter(|r| r.score >= self.config.min_score)
            .map(|mut r| {
                if self.config.normalize_scores && max_score > 0.0 {
                    r.score /= max_score;
                }
                r
            })
            .collect()
    }
}

impl Default for Bm25Searcher {
    fn default() -> Self {
        Self::with_default_config()
    }
}

/// 결과 점수 융합 (RRF - Reciprocal Rank Fusion)
pub fn rrf_fusion(results: &[Vec<SearchResult>], k: f32) -> Vec<SearchResult> {
    let mut fused_scores: HashMap<u64, (f32, Option<String>, CanonicalEntityRef)> = HashMap::new();

    for result_list in results {
        for (rank, result) in result_list.iter().enumerate() {
            let rrf_score = 1.0 / (k + rank as f32 + 1.0);

            fused_scores
                .entry(result.chunk_id)
                .and_modify(|(score, _, _)| *score += rrf_score)
                .or_insert((rrf_score, result.content.clone(), result.entity.clone()));
        }
    }

    let mut fused: Vec<SearchResult> = fused_scores
        .into_iter()
        .map(|(chunk_id, (score, content, entity))| {
            let mut result = SearchResult::new(chunk_id, score, entity);
            if let Some(c) = content {
                result = result.with_content(c);
            }
            result
        })
        .collect();

    // 점수 기준 내림차순 정렬
    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    fused
}

/// 결과 점수 융합 (가중 합)
pub fn weighted_sum_fusion(
    results: &[(Vec<SearchResult>, f32)], // (결과, 가중치)
) -> Vec<SearchResult> {
    let mut fused_scores: HashMap<u64, (f32, Option<String>, CanonicalEntityRef)> = HashMap::new();

    for (result_list, weight) in results {
        // 정규화를 위한 최대 점수
        let max_score = result_list.iter().map(|r| r.score).fold(0.0f32, f32::max);

        for result in result_list {
            let normalized_score = if max_score > 0.0 {
                result.score / max_score
            } else {
                0.0
            };

            let weighted_score = normalized_score * weight;

            fused_scores
                .entry(result.chunk_id)
                .and_modify(|(score, _, _)| *score += weighted_score)
                .or_insert((
                    weighted_score,
                    result.content.clone(),
                    result.entity.clone(),
                ));
        }
    }

    let mut fused: Vec<SearchResult> = fused_scores
        .into_iter()
        .map(|(chunk_id, (score, content, entity))| {
            let mut result = SearchResult::new(chunk_id, score, entity);
            if let Some(c) = content {
                result = result.with_content(c);
            }
            result
        })
        .collect();

    fused.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    fused
}

/// 텍스트 하이라이터
///
/// 검색 쿼리 키워드를 텍스트에서 찾아 하이라이트합니다.
#[derive(Default)]
pub struct TextHighlighter {
    /// 컴파일된 정규식 패턴
    pattern: Option<Regex>,
}

impl TextHighlighter {
    /// 새 하이라이터 생성
    ///
    /// # Arguments
    ///
    /// * `query` - 검색 쿼리 (공백으로 분리된 키워드)
    pub fn new(query: &str) -> Self {
        let pattern = Self::build_pattern(query);
        Self { pattern }
    }

    /// 쿼리에서 정규식 패턴 생성
    fn build_pattern(query: &str) -> Option<Regex> {
        let keywords: Vec<&str> = query
            .split_whitespace()
            .filter(|w| !w.is_empty() && w.len() >= 2)
            .collect();

        if keywords.is_empty() {
            return None;
        }

        // 키워드를 이스케이프하고 OR로 연결
        let escaped: Vec<String> = keywords.iter().map(|k| regex::escape(k)).collect();

        let pattern_str = escaped.join("|");

        // 대소문자 무시 패턴 생성
        Regex::new(&format!("(?i)({})", pattern_str)).ok()
    }

    /// 텍스트에서 하이라이트 추출
    ///
    /// # Arguments
    ///
    /// * `text` - 하이라이트할 텍스트
    ///
    /// # Returns
    ///
    /// 하이라이트 목록 (시작 위치 순)
    pub fn highlight(&self, text: &str) -> Vec<Highlight> {
        let Some(ref pattern) = self.pattern else {
            return Vec::new();
        };

        let mut highlights = Vec::new();

        for mat in pattern.find_iter(text) {
            highlights.push(Highlight {
                start: mat.start(),
                end: mat.end(),
                text: mat.as_str().to_string(),
            });
        }

        highlights
    }

    /// 텍스트에 HTML 태그로 하이라이트 적용
    ///
    /// # Arguments
    ///
    /// * `text` - 원본 텍스트
    /// * `tag` - 래핑할 HTML 태그 (예: "mark", "b", "em")
    ///
    /// # Returns
    ///
    /// 하이라이트가 적용된 텍스트
    pub fn highlight_html(&self, text: &str, tag: &str) -> String {
        let Some(ref pattern) = self.pattern else {
            return text.to_string();
        };

        pattern
            .replace_all(text, &format!("<{}>$0</{}>", tag, tag))
            .to_string()
    }

    /// 검색 결과에 하이라이트 추가
    pub fn apply_to_result(&self, result: &mut SearchResult) {
        if let Some(ref content) = result.content {
            let highlights = self.highlight(content);
            if !highlights.is_empty() {
                result.highlights = Some(highlights);
            }
        }
    }

    /// 검색 결과 목록에 하이라이트 추가
    pub fn apply_to_results(&self, results: &mut [SearchResult]) {
        for result in results {
            self.apply_to_result(result);
        }
    }
}
