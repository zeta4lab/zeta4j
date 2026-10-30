//! 캐시 키 모듈

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// 쿼리 캐시 키
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryCacheKey {
    /// 쿼리 문자열 해시
    query_hash: u64,
    /// 파라미터 타입 시그니처 (Plan Cache용)
    param_signature: Option<String>,
}

impl QueryCacheKey {
    /// 쿼리 문자열로 키 생성
    pub fn from_query(query: &str) -> Self {
        Self {
            query_hash: Self::hash_query(query),
            param_signature: None,
        }
    }

    /// 쿼리 + 파라미터 타입으로 키 생성
    pub fn from_query_with_params(query: &str, param_types: &[&str]) -> Self {
        let signature = if param_types.is_empty() {
            None
        } else {
            Some(param_types.join(","))
        };

        Self {
            query_hash: Self::hash_query(query),
            param_signature: signature,
        }
    }

    /// 쿼리 해시값 반환
    pub fn query_hash(&self) -> u64 {
        self.query_hash
    }

    /// 파라미터 시그니처 반환
    pub fn param_signature(&self) -> Option<&str> {
        self.param_signature.as_deref()
    }

    /// 쿼리 문자열 해싱
    fn hash_query(query: &str) -> u64 {
        // 정규화: 공백 압축, 대소문자 통일
        let normalized = Self::normalize_query(query);
        let mut hasher = DefaultHasher::new();
        normalized.hash(&mut hasher);
        hasher.finish()
    }

    /// 쿼리 정규화 (공백 압축, 대소문자 통일)
    fn normalize_query(query: &str) -> String {
        query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_uppercase()
    }
}
