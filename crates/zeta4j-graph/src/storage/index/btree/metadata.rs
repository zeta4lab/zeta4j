//! B+ Tree 메타데이터 페이지
//!
//! 메타데이터 페이지(Page 0)는 B+ Tree의 상태를 영속적으로 저장합니다.
//! 시스템 재시작 시 이 페이지를 읽어 트리를 복구합니다.

use crate::error::{GraphError, Result};
use crate::storage::disk::page::{PAGE_SIZE, Page, PageId, read_u32_le, read_u64_le};
use std::time::{SystemTime, UNIX_EPOCH};

/// 메타데이터 페이지 레이아웃
/// ```text
/// Offset  Size  Field
/// ------  ----  -----
/// 0       4     Magic Number (0x5A455441 = "ZETA")
/// 4       4     Version
/// 8       4     Root Page ID
/// 12      4     Total Page Count
/// 16      8     Next Node ID
/// 24      8     Created Timestamp (Unix epoch, millis)
/// 32      8     Last Modified Timestamp
/// 40      4     Tree Height
/// 44      8     Total Key Count
/// 52      4012  Reserved
/// ```
#[derive(Debug)]
pub struct BTreeMetadata {
    pub magic: u32,
    pub version: u32,
    pub root_page_id: PageId,
    pub total_pages: u32,
    pub next_node_id: u64,
    pub created_at: u64,
    pub modified_at: u64,
    pub tree_height: u32,
    pub total_keys: u64,
}

impl BTreeMetadata {
    /// Magic number: "ZETA" in ASCII
    pub const MAGIC: u32 = 0x5A455441;
    /// Current metadata version
    pub const VERSION: u32 = 1;
    /// Metadata is always stored in page 0
    pub const METADATA_PAGE_ID: PageId = 0;

    // Offsets
    const OFFSET_MAGIC: usize = 0;
    const OFFSET_VERSION: usize = 4;
    const OFFSET_ROOT_PAGE_ID: usize = 8;
    const OFFSET_TOTAL_PAGES: usize = 12;
    const OFFSET_NEXT_NODE_ID: usize = 16;
    const OFFSET_CREATED_AT: usize = 24;
    const OFFSET_MODIFIED_AT: usize = 32;
    const OFFSET_TREE_HEIGHT: usize = 40;
    const OFFSET_TOTAL_KEYS: usize = 44;

    /// 새 메타데이터 생성
    pub fn new(root_page_id: PageId) -> Self {
        let now = Self::current_timestamp();
        Self {
            magic: Self::MAGIC,
            version: Self::VERSION,
            root_page_id,
            total_pages: 2, // page 0 (metadata) + page 1 (root)
            next_node_id: 0,
            created_at: now,
            modified_at: now,
            tree_height: 1,
            total_keys: 0,
        }
    }

    /// 현재 타임스탬프 (Unix epoch millis)
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// 페이지에서 메타데이터 읽기
    pub fn from_page(page: &Page) -> Result<Self> {
        let data = &page.data;

        let magic = read_u32_le(&data[Self::OFFSET_MAGIC..Self::OFFSET_MAGIC + size_of::<u32>()]);

        if magic != Self::MAGIC {
            return Err(GraphError::Storage {
                message: format!(
                    "Invalid metadata magic number: expected 0x{:08X}, got 0x{:08X}",
                    Self::MAGIC,
                    magic
                ),
            });
        }

        let version =
            read_u32_le(&data[Self::OFFSET_VERSION..Self::OFFSET_VERSION + size_of::<u32>()]);

        if version > Self::VERSION {
            return Err(GraphError::Storage {
                message: format!(
                    "Unsupported metadata version: {}, max supported: {}",
                    version,
                    Self::VERSION
                ),
            });
        }

        let root_page_id = read_u32_le(
            &data[Self::OFFSET_ROOT_PAGE_ID..Self::OFFSET_ROOT_PAGE_ID + size_of::<u32>()],
        );
        let total_pages = read_u32_le(
            &data[Self::OFFSET_TOTAL_PAGES..Self::OFFSET_TOTAL_PAGES + size_of::<u32>()],
        );
        let next_node_id = read_u64_le(
            &data[Self::OFFSET_NEXT_NODE_ID..Self::OFFSET_NEXT_NODE_ID + size_of::<u64>()],
        );
        let created_at =
            read_u64_le(&data[Self::OFFSET_CREATED_AT..Self::OFFSET_CREATED_AT + size_of::<u64>()]);
        let modified_at = read_u64_le(
            &data[Self::OFFSET_MODIFIED_AT..Self::OFFSET_MODIFIED_AT + size_of::<u64>()],
        );
        let tree_height = read_u32_le(
            &data[Self::OFFSET_TREE_HEIGHT..Self::OFFSET_TREE_HEIGHT + size_of::<u32>()],
        );
        let total_keys =
            read_u64_le(&data[Self::OFFSET_TOTAL_KEYS..Self::OFFSET_TOTAL_KEYS + size_of::<u64>()]);

        Ok(Self {
            magic,
            version,
            root_page_id,
            total_pages,
            next_node_id,
            created_at,
            modified_at,
            tree_height,
            total_keys,
        })
    }

    /// 메타데이터를 페이지에 쓰기
    pub fn to_page(&self, page: &mut Page) {
        let data = &mut page.data;

        // Clear the page first
        data[..PAGE_SIZE].fill(0);

        data[Self::OFFSET_MAGIC..Self::OFFSET_MAGIC + 4].copy_from_slice(&self.magic.to_le_bytes());
        data[Self::OFFSET_VERSION..Self::OFFSET_VERSION + 4]
            .copy_from_slice(&self.version.to_le_bytes());
        data[Self::OFFSET_ROOT_PAGE_ID..Self::OFFSET_ROOT_PAGE_ID + 4]
            .copy_from_slice(&self.root_page_id.to_le_bytes());
        data[Self::OFFSET_TOTAL_PAGES..Self::OFFSET_TOTAL_PAGES + 4]
            .copy_from_slice(&self.total_pages.to_le_bytes());
        data[Self::OFFSET_NEXT_NODE_ID..Self::OFFSET_NEXT_NODE_ID + 8]
            .copy_from_slice(&self.next_node_id.to_le_bytes());
        data[Self::OFFSET_CREATED_AT..Self::OFFSET_CREATED_AT + 8]
            .copy_from_slice(&self.created_at.to_le_bytes());
        data[Self::OFFSET_MODIFIED_AT..Self::OFFSET_MODIFIED_AT + 8]
            .copy_from_slice(&self.modified_at.to_le_bytes());
        data[Self::OFFSET_TREE_HEIGHT..Self::OFFSET_TREE_HEIGHT + 4]
            .copy_from_slice(&self.tree_height.to_le_bytes());
        data[Self::OFFSET_TOTAL_KEYS..Self::OFFSET_TOTAL_KEYS + 8]
            .copy_from_slice(&self.total_keys.to_le_bytes());

        page.is_dirty = true;
    }

    /// modified_at 타임스탬프 업데이트
    pub fn touch(&mut self) {
        self.modified_at = Self::current_timestamp();
    }

    /// 유효성 검증
    pub fn validate(&self) -> Result<()> {
        if self.magic != Self::MAGIC {
            return Err(GraphError::Storage {
                message: "Invalid magic number".to_string(),
            });
        }
        if self.version > Self::VERSION {
            return Err(GraphError::Storage {
                message: format!("Unsupported version: {}", self.version),
            });
        }
        Ok(())
    }

    /// 손상된 메타데이터 감지 및 재초기화
    ///
    /// 메타데이터 페이지가 손상된 경우(magic=0x00000000) 새 메타데이터로 재초기화합니다.
    /// 이는 인덱스 생성 중 서버가 강제 종료되어 버퍼 풀의 dirty 페이지가
    /// 디스크에 플러시되지 않은 경우를 복구합니다.
    ///
    /// # Arguments
    /// * `page` - 메타데이터 페이지
    /// * `default_root_id` - 복구 시 사용할 기본 루트 페이지 ID
    ///
    /// # Returns
    /// * `Ok(Self)` - 정상 메타데이터 또는 재초기화된 메타데이터
    /// * `Err` - 복구 불가능한 손상 (magic이 0도 아니고 MAGIC도 아닌 경우)
    pub fn recover_or_create(page: &mut Page, default_root_id: PageId) -> Result<Self> {
        let magic = u32::from_le_bytes(
            page.data[Self::OFFSET_MAGIC..Self::OFFSET_MAGIC + 4]
                .try_into()
                .map_err(|_| GraphError::Storage {
                    message: "slice conversion failed".into(),
                })?,
        );

        if magic == 0 {
            // 플러시 전 크래시로 인한 손상 - 재초기화
            tracing::warn!(
                "BTree metadata corruption detected (magic=0x00000000). Reinitializing with root_page_id={}",
                default_root_id
            );
            let metadata = Self::new(default_root_id);
            metadata.to_page(page);
            Ok(metadata)
        } else if magic != Self::MAGIC {
            Err(GraphError::Storage {
                message: format!(
                    "Unrecoverable metadata corruption: expected 0x{:08X}, got 0x{:08X}",
                    Self::MAGIC,
                    magic
                ),
            })
        } else {
            Self::from_page(page)
        }
    }
}

impl Default for BTreeMetadata {
    fn default() -> Self {
        Self::new(1) // Default root page is page 1 (page 0 is metadata)
    }
}
