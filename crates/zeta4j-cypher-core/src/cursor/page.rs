//! 페이지네이션 관련 타입 정의 모듈

use super::cursor::CursorId;

/// 페이지 요청
///
/// 페이지 기반 쿼리 결과를 요청할 때 사용합니다.
#[derive(Debug, Clone)]
pub struct PageRequest {
    /// 페이지 크기
    page_size: usize,
    /// 페이지 번호 (0-based)
    page_number: usize,
    /// 커서 ID (커서 기반 시)
    cursor_id: Option<CursorId>,
}

impl PageRequest {
    /// 새 PageRequest 생성
    pub fn new(page_size: usize, page_number: usize) -> Self {
        Self {
            page_size,
            page_number,
            cursor_id: None,
        }
    }

    /// 첫 페이지 요청
    pub fn first(page_size: usize) -> Self {
        Self::new(page_size, 0)
    }

    /// 다음 페이지 요청
    pub fn next(page_size: usize, page_number: usize) -> Self {
        Self::new(page_size, page_number)
    }

    /// 커서 기반 요청
    pub fn with_cursor(cursor_id: CursorId, page_size: usize) -> Self {
        Self {
            page_size,
            page_number: 0,
            cursor_id: Some(cursor_id),
        }
    }

    /// 페이지 크기 반환
    pub fn page_size(&self) -> usize {
        self.page_size
    }

    /// 페이지 번호 반환
    pub fn page_number(&self) -> usize {
        self.page_number
    }

    /// 커서 ID 반환
    pub fn cursor_id(&self) -> Option<&CursorId> {
        self.cursor_id.as_ref()
    }

    /// 커서 기반인지 확인
    pub fn is_cursor_based(&self) -> bool {
        self.cursor_id.is_some()
    }

    /// SKIP 값 계산
    pub fn skip(&self) -> usize {
        self.page_size * self.page_number
    }

    /// LIMIT 값 반환
    pub fn limit(&self) -> usize {
        self.page_size
    }

    /// 페이지 크기 설정
    pub fn set_page_size(&mut self, size: usize) {
        self.page_size = size;
    }

    /// 다음 페이지 요청으로 변환
    pub fn next_page(&self) -> Self {
        Self {
            page_size: self.page_size,
            page_number: self.page_number + 1,
            cursor_id: self.cursor_id.clone(),
        }
    }

    /// 이전 페이지 요청으로 변환
    pub fn prev_page(&self) -> Option<Self> {
        if self.page_number == 0 {
            None
        } else {
            Some(Self {
                page_size: self.page_size,
                page_number: self.page_number - 1,
                cursor_id: self.cursor_id.clone(),
            })
        }
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self::first(20)
    }
}

/// 페이지 정보
///
/// 현재 페이지에 대한 메타데이터를 제공합니다.
#[derive(Debug, Clone)]
pub struct PageInfo {
    /// 현재 페이지 번호 (0-based)
    pub page_number: usize,
    /// 페이지 크기
    pub page_size: usize,
    /// 현재 페이지의 항목 수
    pub count: usize,
    /// 총 항목 수 (알려진 경우)
    pub total_count: Option<usize>,
    /// 총 페이지 수 (알려진 경우)
    pub total_pages: Option<usize>,
    /// 다음 페이지 존재 여부
    pub has_next: bool,
    /// 이전 페이지 존재 여부
    pub has_previous: bool,
    /// 커서 ID (커서 기반 시)
    pub cursor_id: Option<CursorId>,
}

impl PageInfo {
    /// 새 PageInfo 생성
    pub fn new(page_number: usize, page_size: usize, count: usize, has_next: bool) -> Self {
        Self {
            page_number,
            page_size,
            count,
            total_count: None,
            total_pages: None,
            has_next,
            has_previous: page_number > 0,
            cursor_id: None,
        }
    }

    /// 총 개수를 포함하여 생성
    pub fn with_total(
        page_number: usize,
        page_size: usize,
        count: usize,
        total_count: usize,
    ) -> Self {
        let total_pages = if page_size > 0 {
            total_count.div_ceil(page_size)
        } else {
            0
        };
        let has_next = page_number + 1 < total_pages;

        Self {
            page_number,
            page_size,
            count,
            total_count: Some(total_count),
            total_pages: Some(total_pages),
            has_next,
            has_previous: page_number > 0,
            cursor_id: None,
        }
    }

    /// 커서 ID 설정
    pub fn with_cursor(mut self, cursor_id: CursorId) -> Self {
        self.cursor_id = Some(cursor_id);
        self
    }

    /// 첫 페이지 여부
    pub fn is_first(&self) -> bool {
        self.page_number == 0
    }

    /// 마지막 페이지 여부
    pub fn is_last(&self) -> bool {
        !self.has_next
    }

    /// 빈 페이지 여부
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 현재 페이지 번호 (1-based, 표시용)
    pub fn display_page_number(&self) -> usize {
        self.page_number + 1
    }

    /// 시작 인덱스 (1-based, 표시용)
    pub fn start_index(&self) -> usize {
        if self.count == 0 {
            0
        } else {
            self.page_number * self.page_size + 1
        }
    }

    /// 끝 인덱스 (1-based, 표시용)
    pub fn end_index(&self) -> usize {
        if self.count == 0 {
            0
        } else {
            self.page_number * self.page_size + self.count
        }
    }
}

impl Default for PageInfo {
    fn default() -> Self {
        Self::new(0, 20, 0, false)
    }
}

/// 페이지 결과
///
/// 페이지네이션된 쿼리 결과를 나타냅니다.
#[derive(Debug, Clone)]
pub struct PagedResult<T> {
    /// 결과 데이터
    data: Vec<T>,
    /// 페이지 정보
    page_info: PageInfo,
}

impl<T> PagedResult<T> {
    /// 새 PagedResult 생성
    pub fn new(data: Vec<T>, page_info: PageInfo) -> Self {
        Self { data, page_info }
    }

    /// 빈 결과 생성
    pub fn empty(page_size: usize) -> Self {
        Self {
            data: Vec::new(),
            page_info: PageInfo::new(0, page_size, 0, false),
        }
    }

    /// 첫 페이지 결과 생성
    pub fn first_page(data: Vec<T>, page_size: usize, has_next: bool) -> Self {
        let count = data.len();
        Self {
            data,
            page_info: PageInfo::new(0, page_size, count, has_next),
        }
    }

    /// 결과 데이터 반환 (참조)
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// 결과 데이터 반환 (소유권 이전)
    pub fn into_data(self) -> Vec<T> {
        self.data
    }

    /// 페이지 정보 반환
    pub fn page_info(&self) -> &PageInfo {
        &self.page_info
    }

    /// 페이지 정보 반환 (mutable)
    pub fn page_info_mut(&mut self) -> &mut PageInfo {
        &mut self.page_info
    }

    /// 결과가 비어있는지
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// 결과 수
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// 첫 번째 항목
    pub fn first(&self) -> Option<&T> {
        self.data.first()
    }

    /// 마지막 항목
    pub fn last(&self) -> Option<&T> {
        self.data.last()
    }

    /// 이터레이터
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }

    /// 다음 페이지가 있는지
    pub fn has_next(&self) -> bool {
        self.page_info.has_next
    }

    /// 이전 페이지가 있는지
    pub fn has_previous(&self) -> bool {
        self.page_info.has_previous
    }

    /// 데이터 변환
    pub fn map<U, F: FnMut(T) -> U>(self, f: F) -> PagedResult<U> {
        PagedResult {
            data: self.data.into_iter().map(f).collect(),
            page_info: self.page_info,
        }
    }
}

impl<T> IntoIterator for PagedResult<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.data.into_iter()
    }
}

impl<T> Default for PagedResult<T> {
    fn default() -> Self {
        Self::empty(20)
    }
}
