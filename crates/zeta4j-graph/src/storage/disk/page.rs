//! 디스크 페이지 구조
//!
//! Page-level WAL을 위한 LSN 추적을 지원합니다.

use crate::transaction::lsn::{INVALID_LSN, LSN};

/// 페이지 크기 (4KB)
pub const PAGE_SIZE: usize = 4096;

/// 페이지 헤더 크기 (16 bytes)
///
/// 레이아웃 (SlottedPage와 동일):
/// - [0..4]   page_id: u32
/// - [4..12]  page_lsn: u64
/// - [12..14] slot_count: u16
/// - [14..16] free_space_ptr: u16
pub const PAGE_HEADER_SIZE: usize = 16;

/// 페이지 헤더 내 LSN 오프셋 (SlottedPage와 동일)
const PAGE_LSN_OFFSET: usize = 4;

/// 페이지 ID 타입
pub type PageId = u32;

pub(crate) fn read_u16_le(bytes: &[u8]) -> u16 {
    let mut value = [0; size_of::<u16>()];
    value.copy_from_slice(bytes);
    u16::from_le_bytes(value)
}

pub(crate) fn read_u32_le(bytes: &[u8]) -> u32 {
    let mut value = [0; size_of::<u32>()];
    value.copy_from_slice(bytes);
    u32::from_le_bytes(value)
}

pub(crate) fn read_u64_le(bytes: &[u8]) -> u64 {
    let mut value = [0; size_of::<u64>()];
    value.copy_from_slice(bytes);
    u64::from_le_bytes(value)
}

/// 디스크 페이지 구조체
///
/// Page-level WAL을 위한 LSN 필드를 포함합니다:
/// - `page_lsn`: 페이지에 마지막으로 적용된 WAL LSN
/// - `rec_lsn`: 페이지가 dirty 상태가 된 첫 번째 LSN (Recovery용)
#[derive(Debug, Clone, Copy)]
pub struct Page {
    /// 페이지 ID (None이면 아직 할당 안됨/메모리 전용)
    pub id: Option<PageId>,
    /// 페이지 데이터 (4KB)
    pub data: [u8; PAGE_SIZE],
    /// Dirty 플래그 (BufferPool에서 디스크 쓰기 필요 여부)
    pub is_dirty: bool,
    /// Pin 카운트 (사용 중인 참조 수)
    pub pin_count: u32,
    /// 페이지에 마지막으로 적용된 WAL LSN
    ///
    /// REDO 조건: `page_lsn < record.lsn` 이면 REDO 수행
    pub page_lsn: LSN,
    /// 페이지가 dirty 상태가 된 첫 번째 LSN
    ///
    /// Recovery의 Analysis Phase에서 Dirty Page Table 구축에 사용됩니다.
    /// None이면 clean 상태, Some이면 dirty 상태입니다.
    pub rec_lsn: Option<LSN>,
}

impl Page {
    /// 새 페이지 생성
    pub fn new() -> Self {
        Self {
            id: None,
            data: [0; PAGE_SIZE],
            is_dirty: false,
            pin_count: 0,
            page_lsn: INVALID_LSN,
            rec_lsn: None,
        }
    }

    /// 페이지 상태 초기화
    pub fn reset(&mut self) {
        self.id = None;
        self.data.fill(0);
        self.is_dirty = false;
        self.pin_count = 0;
        self.page_lsn = INVALID_LSN;
        self.rec_lsn = None;
    }

    /// 페이지 헤더에서 LSN 읽기 (디스크에서 로드 시)
    ///
    /// 디스크에서 읽은 페이지 데이터에서 LSN을 추출합니다.
    pub fn read_lsn_from_header(&self) -> LSN {
        let bytes = &self.data[PAGE_LSN_OFFSET..PAGE_LSN_OFFSET + 8];
        read_u64_le(bytes)
    }

    /// 페이지 헤더에 LSN 기록 (디스크 쓰기 전)
    ///
    /// 디스크에 쓰기 전에 페이지 헤더에 LSN을 기록합니다.
    pub fn write_lsn_to_header(&mut self, lsn: LSN) {
        self.data[PAGE_LSN_OFFSET..PAGE_LSN_OFFSET + 8].copy_from_slice(&lsn.to_le_bytes());
        self.page_lsn = lsn;
    }

    /// 디스크에서 로드 후 LSN 동기화
    ///
    /// 디스크에서 읽은 페이지의 헤더에서 LSN을 읽어 page_lsn 필드에 설정합니다.
    pub fn sync_lsn_from_header(&mut self) {
        self.page_lsn = self.read_lsn_from_header();
    }

    /// REDO 필요 여부 확인
    ///
    /// WAL 레코드의 LSN이 페이지 LSN보다 크면 REDO가 필요합니다.
    #[inline]
    pub fn needs_redo(&self, record_lsn: LSN) -> bool {
        self.page_lsn < record_lsn
    }

    /// 페이지를 dirty로 마킹하고 LSN 업데이트
    ///
    /// 처음 dirty가 될 때 rec_lsn을 설정합니다.
    pub fn mark_dirty_with_lsn(&mut self, lsn: LSN) {
        if self.rec_lsn.is_none() {
            self.rec_lsn = Some(lsn);
        }
        self.page_lsn = lsn;
        self.is_dirty = true;
    }

    /// Clean 상태로 전환 (flush 후)
    pub fn mark_clean(&mut self) {
        self.is_dirty = false;
        self.rec_lsn = None;
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}
