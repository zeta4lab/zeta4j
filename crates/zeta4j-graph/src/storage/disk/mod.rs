//! 디스크 I/O 계층
//!
//! 디스크 매니저, 페이지, 슬롯 페이지, Direct I/O, 디스크 노드/엣지 저장소를 제공합니다.

pub mod direct_io;
pub mod edge_storage;
pub mod manager;
pub mod node_storage;
pub mod page;
pub mod slotted_page;

pub use direct_io::{
    AlignedBuffer, DEFAULT_ALIGNMENT, DirectIoConfig, DirectIoDiskManager, DirectIoError,
    DirectIoFile, DirectIoStats, MIN_ALIGNMENT,
};
pub use edge_storage::DiskEdgeStorage;
pub use manager::DiskManager;
pub use node_storage::DiskNodeStorage;
pub use page::{PAGE_SIZE, Page, PageId};
pub use slotted_page::{SlotId, SlottedPage};
