use crate::storage::disk::page::PageId;

/// 레코드 식별자 (PageId + SlotId)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordId {
    pub page_id: PageId,
    pub slot_id: u16,
}

/// B+ Tree 노드 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexPageType {
    LeafPage,
    InternalPage,
}

/// 모든 B+ Tree 페이지의 공통 헤더
pub trait BTreePageHeader {
    fn is_leaf_page(&self) -> bool;
    fn is_root_page(&self) -> bool;
    fn set_root_page(&mut self, root: bool);
    fn get_page_type(&self) -> IndexPageType;
}

// 상수 정의
pub const INVALID_PAGE_ID: PageId = u32::MAX;
