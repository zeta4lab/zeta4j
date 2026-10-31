// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! WAL 레코드 타입 정의
//!
//! Page-level ARIES WAL 레코드와 관련 타입을 정의합니다.
//! 실제 WAL 파일 I/O는 `PageWAL`에서 처리됩니다.

use serde::{Deserialize, Serialize};

use super::TransactionId;
use super::lsn::LSN;
use crate::storage::disk::page::PageId;
use crate::storage::disk::slotted_page::SlotId;

/// 페이지 스토리지 유형
///
/// WAL 레코드가 어떤 스토리지 파일에 속하는지 식별합니다.
/// 단일 WAL에서 여러 스토리지 파일의 변경을 관리하기 위해 사용됩니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PageStorageType {
    /// 노드 데이터 (nodes.db)
    #[default]
    Node,
    /// 엣지 데이터 (edges.db)
    Edge,
    /// 노드 레이블 인덱스 (label.idx)
    NodeLabel,
    /// 엣지 Outgoing 인덱스 (outgoing.idx)
    EdgeOutgoing,
    /// 엣지 Incoming 인덱스 (incoming.idx)
    EdgeIncoming,
    /// 엣지 Type 인덱스 (type.idx)
    EdgeType,
    /// B+Tree 메타데이터 (btree_meta.db)
    BTreeMeta,
    /// 벡터 인덱스 (HNSW)
    VectorIndex,
}

/// WAL 설정
#[derive(Debug, Clone)]
pub struct WalConfig {
    /// 최대 로그 파일 크기 (바이트, 기본값: 100MB)
    pub max_file_size: u64,
    /// 최대 보관 로그 파일 수 (기본값: 5)
    pub max_files: usize,
    /// 로테이션 활성화 여부 (기본값: true)
    pub rotation_enabled: bool,
}

impl Default for WalConfig {
    fn default() -> Self {
        Self {
            max_file_size: 100 * 1024 * 1024, // 100MB
            max_files: 5,
            rotation_enabled: true,
        }
    }
}

impl WalConfig {
    /// 로테이션 비활성화 (단일 파일 모드)
    pub fn no_rotation() -> Self {
        Self {
            rotation_enabled: false,
            ..Default::default()
        }
    }

    /// 커스텀 설정
    pub fn with_limits(max_file_size: u64, max_files: usize) -> Self {
        Self {
            max_file_size,
            max_files,
            rotation_enabled: true,
        }
    }

    /// 테스트용 소규모 설정 (1KB, 3파일)
    pub fn small() -> Self {
        Self {
            max_file_size: 1024, // 1KB
            max_files: 3,
            rotation_enabled: true,
        }
    }
}

/// 로그 엔트리 유형
///
/// Page-level ARIES WAL 레코드. 모든 데이터 변경은 페이지 단위로 기록됩니다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogRecord {
    /// 트랜잭션 시작
    Begin(TransactionId),
    /// 트랜잭션 커밋
    Commit(TransactionId),
    /// 트랜잭션 롤백
    Abort(TransactionId),

    // ==================== Page-level WAL (ARIES) ====================
    /// 페이지 수정 로그 (Physical Logging)
    ///
    /// ARIES 알고리즘의 핵심 레코드. 페이지 단위의 물리적 변경을 기록합니다.
    /// - REDO: after_image를 페이지에 적용
    /// - UNDO: before_image를 페이지에 적용
    PageWrite {
        /// 트랜잭션 ID
        tx_id: TransactionId,
        /// 스토리지 유형 (Node, Edge, LabelIndex 등)
        storage_type: PageStorageType,
        /// 변경된 페이지 ID
        page_id: PageId,
        /// 페이지 내 오프셋
        offset: u16,
        /// 변경 길이
        length: u16,
        /// 변경 전 이미지 (UNDO용)
        before_image: Vec<u8>,
        /// 변경 후 이미지 (REDO용)
        after_image: Vec<u8>,
    },

    /// Compensation Log Record (CLR)
    ///
    /// UNDO 작업의 결과를 기록합니다. CLR 자체는 UNDO되지 않습니다.
    /// Recovery 시 이미 UNDO된 작업을 건너뛰는 데 사용됩니다.
    CLR {
        /// 트랜잭션 ID
        tx_id: TransactionId,
        /// 스토리지 유형
        storage_type: PageStorageType,
        /// 다음 UNDO 대상 LSN (UNDO 체인)
        undo_next_lsn: LSN,
        /// UNDO된 페이지 ID
        page_id: PageId,
        /// 페이지 내 오프셋
        offset: u16,
        /// 변경 길이
        length: u16,
        /// UNDO 결과 이미지
        after_image: Vec<u8>,
    },

    /// Fuzzy Checkpoint 시작
    BeginCheckpoint,

    /// Fuzzy Checkpoint 종료
    ///
    /// ARIES 스타일의 체크포인트. 활성 트랜잭션과 dirty 페이지 정보를 기록합니다.
    EndCheckpoint {
        /// 활성 트랜잭션 목록: (tx_id, last_lsn)
        active_transactions: Vec<(TransactionId, LSN)>,
        /// Dirty Page 테이블: (storage_type, page_id, rec_lsn)
        dirty_page_table: Vec<(PageStorageType, PageId, LSN)>,
    },

    // ==================== Slot-level WAL (ARIES Logical UNDO) ====================
    /// 슬롯 삽입 로그
    ///
    /// SlottedPage에 레코드를 삽입할 때 기록합니다.
    /// - REDO: 슬롯에 데이터 삽입
    /// - UNDO: 슬롯 삭제
    SlotInsert {
        /// 트랜잭션 ID
        tx_id: TransactionId,
        /// 스토리지 유형
        storage_type: PageStorageType,
        /// 페이지 ID
        page_id: PageId,
        /// 슬롯 ID
        slot_id: SlotId,
        /// 삽입된 레코드 데이터
        data: Vec<u8>,
    },

    /// 슬롯 삭제 로그
    ///
    /// SlottedPage에서 레코드를 삭제할 때 기록합니다.
    /// - REDO: 슬롯 삭제
    /// - UNDO: 슬롯에 데이터 복원
    SlotDelete {
        /// 트랜잭션 ID
        tx_id: TransactionId,
        /// 스토리지 유형
        storage_type: PageStorageType,
        /// 페이지 ID
        page_id: PageId,
        /// 슬롯 ID
        slot_id: SlotId,
        /// 삭제된 레코드 데이터 (UNDO용)
        data: Vec<u8>,
    },

    /// 슬롯 업데이트 로그
    ///
    /// SlottedPage의 레코드를 수정할 때 기록합니다.
    /// - REDO: after_data 적용
    /// - UNDO: before_data 적용
    SlotUpdate {
        /// 트랜잭션 ID
        tx_id: TransactionId,
        /// 스토리지 유형
        storage_type: PageStorageType,
        /// 페이지 ID
        page_id: PageId,
        /// 슬롯 ID
        slot_id: SlotId,
        /// 수정 전 데이터 (UNDO용)
        before_data: Vec<u8>,
        /// 수정 후 데이터 (REDO용)
        after_data: Vec<u8>,
    },

    /// Slot-level CLR (Compensation Log Record)
    ///
    /// Slot-level UNDO 작업의 결과를 기록합니다. CLR 자체는 UNDO되지 않습니다.
    SlotCLR {
        /// 트랜잭션 ID
        tx_id: TransactionId,
        /// 스토리지 유형
        storage_type: PageStorageType,
        /// 다음 UNDO 대상 LSN (UNDO 체인)
        undo_next_lsn: LSN,
        /// 페이지 ID
        page_id: PageId,
        /// 슬롯 ID
        slot_id: SlotId,
        /// UNDO 작업 유형
        undo_op: SlotUndoOp,
    },
}

/// Slot-level UNDO 작업 유형
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlotUndoOp {
    /// Insert를 UNDO: 슬롯 삭제
    DeleteSlot,
    /// Delete를 UNDO: 슬롯 복원
    RestoreSlot {
        /// 복원할 데이터
        data: Vec<u8>,
    },
    /// Update를 UNDO: 이전 데이터로 복원
    RestoreData {
        /// 복원할 데이터
        data: Vec<u8>,
    },
}

impl LogRecord {
    /// 로그 레코드의 트랜잭션 ID 추출 (체크포인트는 0 반환)
    pub fn transaction_id(&self) -> TransactionId {
        match self {
            LogRecord::Begin(tx_id) | LogRecord::Commit(tx_id) | LogRecord::Abort(tx_id) => *tx_id,
            // Page-level WAL
            LogRecord::PageWrite { tx_id, .. } | LogRecord::CLR { tx_id, .. } => *tx_id,
            // Checkpoint
            LogRecord::BeginCheckpoint | LogRecord::EndCheckpoint { .. } => 0,
            // Slot-level WAL
            LogRecord::SlotInsert { tx_id, .. }
            | LogRecord::SlotDelete { tx_id, .. }
            | LogRecord::SlotUpdate { tx_id, .. }
            | LogRecord::SlotCLR { tx_id, .. } => *tx_id,
        }
    }

    /// 체크포인트 레코드인지 확인
    pub fn is_checkpoint(&self) -> bool {
        matches!(
            self,
            LogRecord::BeginCheckpoint | LogRecord::EndCheckpoint { .. }
        )
    }

    /// 페이지 수정 로그인지 확인 (물리 REDO/UNDO 대상)
    pub fn is_page_modification(&self) -> bool {
        matches!(self, LogRecord::PageWrite { .. } | LogRecord::CLR { .. })
    }

    /// 슬롯 수정 로그인지 확인 (Slot-level REDO/UNDO 대상)
    pub fn is_slot_modification(&self) -> bool {
        matches!(
            self,
            LogRecord::SlotInsert { .. }
                | LogRecord::SlotDelete { .. }
                | LogRecord::SlotUpdate { .. }
        )
    }

    /// 데이터 변경 로그인지 확인 (페이지 또는 슬롯 수정)
    pub fn is_data_modification(&self) -> bool {
        self.is_page_modification() || self.is_slot_modification()
    }

    /// CLR 레코드인지 확인 (Page-level 또는 Slot-level)
    pub fn is_clr(&self) -> bool {
        matches!(self, LogRecord::CLR { .. } | LogRecord::SlotCLR { .. })
    }

    /// Slot-level CLR 레코드인지 확인
    pub fn is_slot_clr(&self) -> bool {
        matches!(self, LogRecord::SlotCLR { .. })
    }

    /// PageWrite 또는 Slot-level 레코드에서 page_id 추출
    pub fn page_id(&self) -> Option<PageId> {
        match self {
            LogRecord::PageWrite { page_id, .. } => Some(*page_id),
            LogRecord::CLR { page_id, .. } => Some(*page_id),
            LogRecord::SlotInsert { page_id, .. } => Some(*page_id),
            LogRecord::SlotDelete { page_id, .. } => Some(*page_id),
            LogRecord::SlotUpdate { page_id, .. } => Some(*page_id),
            LogRecord::SlotCLR { page_id, .. } => Some(*page_id),
            _ => None,
        }
    }

    /// Slot-level 레코드에서 slot_id 추출
    pub fn slot_id(&self) -> Option<SlotId> {
        match self {
            LogRecord::SlotInsert { slot_id, .. } => Some(*slot_id),
            LogRecord::SlotDelete { slot_id, .. } => Some(*slot_id),
            LogRecord::SlotUpdate { slot_id, .. } => Some(*slot_id),
            LogRecord::SlotCLR { slot_id, .. } => Some(*slot_id),
            _ => None,
        }
    }

    /// 레코드에서 storage_type 추출
    pub fn storage_type(&self) -> Option<PageStorageType> {
        match self {
            LogRecord::PageWrite { storage_type, .. } => Some(*storage_type),
            LogRecord::CLR { storage_type, .. } => Some(*storage_type),
            LogRecord::SlotInsert { storage_type, .. } => Some(*storage_type),
            LogRecord::SlotDelete { storage_type, .. } => Some(*storage_type),
            LogRecord::SlotUpdate { storage_type, .. } => Some(*storage_type),
            LogRecord::SlotCLR { storage_type, .. } => Some(*storage_type),
            _ => None,
        }
    }
}
