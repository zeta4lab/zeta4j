//! 가져오기 결과 정의 모듈

use zeta4j_graph::core::Value;

/// 행 타입 (결과 행)
pub type Row = Vec<Value>;

/// 가져오기 결과
///
/// 커서에서 데이터를 가져온 결과를 나타냅니다.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// 결과 행들
    rows: Vec<Row>,
    /// 더 많은 행이 있는지
    has_more: bool,
    /// 다음 커서 위치
    next_position: usize,
    /// 가져온 행 수
    fetched_count: usize,
}

impl FetchResult {
    /// 새 FetchResult 생성
    pub fn new(rows: Vec<Row>, has_more: bool, next_position: usize) -> Self {
        let fetched_count = rows.len();
        Self {
            rows,
            has_more,
            next_position,
            fetched_count,
        }
    }

    /// 빈 결과 생성
    pub fn empty() -> Self {
        Self {
            rows: Vec::new(),
            has_more: false,
            next_position: 0,
            fetched_count: 0,
        }
    }

    /// 결과 행들 반환 (참조)
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// 결과 행들 반환 (소유권 이전)
    pub fn into_rows(self) -> Vec<Row> {
        self.rows
    }

    /// 더 많은 행이 있는지
    pub fn has_more(&self) -> bool {
        self.has_more
    }

    /// 다음 커서 위치
    pub fn next_position(&self) -> usize {
        self.next_position
    }

    /// 가져온 행 수
    pub fn fetched_count(&self) -> usize {
        self.fetched_count
    }

    /// 결과가 비어있는지
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 결과 행 수
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 첫 번째 행 반환
    pub fn first(&self) -> Option<&Row> {
        self.rows.first()
    }

    /// 마지막 행 반환
    pub fn last(&self) -> Option<&Row> {
        self.rows.last()
    }

    /// 행 이터레이터
    pub fn iter(&self) -> impl Iterator<Item = &Row> {
        self.rows.iter()
    }
}

impl IntoIterator for FetchResult {
    type Item = Row;
    type IntoIter = std::vec::IntoIter<Row>;

    fn into_iter(self) -> Self::IntoIter {
        self.rows.into_iter()
    }
}

impl Default for FetchResult {
    fn default() -> Self {
        Self::empty()
    }
}
