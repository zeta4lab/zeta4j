// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 언어 감지 및 라우팅
//!
//! 텍스트의 언어를 자동으로 감지하고 적절한 토크나이저로 라우팅합니다.

use whatlang::{Lang, detect};

/// 감지된 언어
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetectedLanguage {
    /// 한국어
    Korean,
    /// 영어
    English,
    /// 일본어
    Japanese,
    /// 중국어
    Chinese,
    /// 알 수 없음
    Unknown,
}

impl DetectedLanguage {
    /// 언어 코드 반환
    pub fn code(&self) -> &'static str {
        match self {
            DetectedLanguage::Korean => "ko",
            DetectedLanguage::English => "en",
            DetectedLanguage::Japanese => "ja",
            DetectedLanguage::Chinese => "zh",
            DetectedLanguage::Unknown => "unknown",
        }
    }

    /// 토크나이저 이름 반환
    pub fn tokenizer_name(&self) -> &'static str {
        match self {
            DetectedLanguage::Korean => "ko",
            DetectedLanguage::English => "en_stem",
            DetectedLanguage::Japanese => "ja",
            DetectedLanguage::Chinese => "zh",
            DetectedLanguage::Unknown => "default",
        }
    }
}

impl std::fmt::Display for DetectedLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}

/// 언어 라우터
#[derive(Debug, Clone)]
pub struct LanguageRouter {
    /// 기본 언어 (감지 실패 시)
    default_language: DetectedLanguage,
    /// 최소 텍스트 길이 (이보다 짧으면 기본 언어 사용)
    min_text_length: usize,
    /// 최소 신뢰도 (이보다 낮으면 기본 언어 사용)
    min_confidence: f64,
}

impl LanguageRouter {
    /// 새 언어 라우터 생성
    pub fn new() -> Self {
        Self {
            default_language: DetectedLanguage::English,
            min_text_length: 10,
            min_confidence: 0.5,
        }
    }

    /// 기본 언어 설정
    pub fn with_default_language(mut self, lang: DetectedLanguage) -> Self {
        self.default_language = lang;
        self
    }

    /// 최소 텍스트 길이 설정
    pub fn with_min_text_length(mut self, len: usize) -> Self {
        self.min_text_length = len;
        self
    }

    /// 최소 신뢰도 설정
    pub fn with_min_confidence(mut self, confidence: f64) -> Self {
        self.min_confidence = confidence;
        self
    }

    /// 텍스트 언어 감지
    pub fn detect_language(&self, text: &str) -> DetectedLanguage {
        // 텍스트가 너무 짧으면 기본 언어 반환
        if text.len() < self.min_text_length {
            return self.detect_by_characters(text);
        }

        match detect(text) {
            Some(info) => {
                // 신뢰도가 낮으면 문자 기반 감지 시도
                if info.confidence() < self.min_confidence {
                    return self.detect_by_characters(text);
                }

                match info.lang() {
                    Lang::Kor => DetectedLanguage::Korean,
                    Lang::Eng => DetectedLanguage::English,
                    Lang::Jpn => DetectedLanguage::Japanese,
                    Lang::Cmn => DetectedLanguage::Chinese,
                    _ => self.default_language,
                }
            }
            None => self.detect_by_characters(text),
        }
    }

    /// 문자 기반 언어 감지 (짧은 텍스트용)
    fn detect_by_characters(&self, text: &str) -> DetectedLanguage {
        let mut korean_count = 0;
        let mut japanese_count = 0;
        let mut chinese_count = 0;
        let mut latin_count = 0;
        let mut total_count = 0;

        for c in text.chars() {
            if c.is_whitespace() || c.is_ascii_punctuation() {
                continue;
            }

            total_count += 1;

            if is_korean(c) {
                korean_count += 1;
            } else if is_japanese(c) {
                japanese_count += 1;
            } else if is_chinese(c) {
                chinese_count += 1;
            } else if c.is_ascii_alphabetic() {
                latin_count += 1;
            }
        }

        if total_count == 0 {
            return self.default_language;
        }

        // 가장 많은 문자 유형으로 판단
        let korean_ratio = korean_count as f64 / total_count as f64;
        let japanese_ratio = japanese_count as f64 / total_count as f64;
        let chinese_ratio = chinese_count as f64 / total_count as f64;
        let latin_ratio = latin_count as f64 / total_count as f64;

        if korean_ratio > 0.3 {
            DetectedLanguage::Korean
        } else if japanese_ratio > 0.3 {
            DetectedLanguage::Japanese
        } else if chinese_ratio > 0.3 {
            DetectedLanguage::Chinese
        } else if latin_ratio > 0.5 {
            DetectedLanguage::English
        } else {
            self.default_language
        }
    }

    /// 언어에 따른 content 필드 이름 반환
    pub fn get_content_field(&self, lang: DetectedLanguage) -> &'static str {
        match lang {
            DetectedLanguage::Korean => "content_ko",
            DetectedLanguage::English => "content_en",
            DetectedLanguage::Japanese => "content_ja",
            DetectedLanguage::Chinese => "content_zh",
            DetectedLanguage::Unknown => "content",
        }
    }

    /// 쿼리 확장 (다국어 검색용)
    pub fn expand_query_fields(&self, lang: DetectedLanguage) -> Vec<&'static str> {
        match lang {
            DetectedLanguage::Korean => vec!["content_ko", "content"],
            DetectedLanguage::English => vec!["content_en", "content"],
            DetectedLanguage::Japanese => vec!["content_ja", "content"],
            DetectedLanguage::Chinese => vec!["content_zh", "content"],
            DetectedLanguage::Unknown => vec!["content"],
        }
    }
}

impl Default for LanguageRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// 한글 문자인지 확인
fn is_korean(c: char) -> bool {
    // 한글 음절: U+AC00 ~ U+D7AF
    // 한글 자모: U+1100 ~ U+11FF
    // 호환용 한글 자모: U+3130 ~ U+318F
    matches!(c, '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}')
}

/// 일본어 문자인지 확인 (히라가나, 카타카나)
fn is_japanese(c: char) -> bool {
    // 히라가나: U+3040 ~ U+309F
    // 카타카나: U+30A0 ~ U+30FF
    matches!(c, '\u{3040}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}')
}

/// 중국어 문자인지 확인 (CJK 통합 한자)
fn is_chinese(c: char) -> bool {
    // CJK 통합 한자: U+4E00 ~ U+9FFF
    // CJK 확장: U+3400 ~ U+4DBF, U+20000 ~ U+2A6DF
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
}
