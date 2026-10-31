// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 쿼리 결과 타입.

use std::collections::HashMap;

use zeta4j_bolt_protocol::packstream::PackStreamValue;

/// 쿼리 결과
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// 컬럼 이름
    pub columns: Vec<String>,
    /// 레코드들
    pub records: Vec<Vec<PackStreamValue>>,
    /// 북마크
    pub bookmark: Option<String>,
    /// 통계
    pub stats: Option<HashMap<String, PackStreamValue>>,
}

impl QueryResult {
    /// 새 쿼리 결과 생성
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            records: Vec::new(),
            bookmark: None,
            stats: None,
        }
    }

    /// 레코드 수
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 빈 결과인지
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}
