// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 버퍼 풀 관리자
//!
//! Page-level WAL을 위한 LSN 추적을 지원합니다.
//!
//! ## PageWAL 통합 (v3.0.1)
//!
//! BufferPoolManager는 선택적으로 PageWAL과 통합되어:
//! - 페이지 수정 시 before/after 이미지를 WAL에 기록
//! - 크래시 복구 시 UNDO를 위한 정보 제공
//! - 페이지 evict 전 WAL flush 보장

pub use super::types::{BufferPoolStats, FrameId};

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::error::{GraphError, Result};
use crate::transaction::{PageStorageType, PageWAL, TransactionId};

use crate::storage::disk::manager::DiskManager;
use crate::storage::disk::page::{PAGE_SIZE, Page, PageId};
use crate::storage::memory::LRUReplacer;

/// 버퍼 풀 관리자 (개선된 동시성)
///
/// LRU 정책을 사용하여 페이지 교체를 관리합니다.
/// 내부 가변성(Interior Mutability)을 사용하여 세밀한 동시성 제어를 제공합니다.
///
/// ## 동시성 모델
/// - `page_table`: RwLock으로 보호 (읽기는 병렬, 쓰기만 독점)
/// - `free_list`: Mutex로 보호 (할당/해제 시에만 락)
/// - `replacer`: Mutex로 보호 (LRU 연산 시에만 락)
/// - `pages`: 개별 페이지는 Mutex로 보호 (페이지별 독립 접근)
///
/// ## PageWAL 통합
/// - `page_wal`: 선택적 PageWAL 참조
/// - `before_images`: 수정 전 페이지 이미지 (UNDO용)
pub struct BufferPoolManager {
    pub(super) disk_manager: Arc<DiskManager>,
    pub(super) pool_size: usize,
    pub(super) pages: Vec<Arc<Mutex<Page>>>, // 실제 페이지 데이터 (Thread-safe, 개별 락)
    pub(super) page_table: RwLock<HashMap<PageId, FrameId>>, // 읽기 병렬, 쓰기 독점
    pub(super) free_list: Mutex<Vec<FrameId>>, // 할당/해제 시에만 락
    pub(super) replacer: Mutex<LRUReplacer>, // LRU 연산 시에만 락
    /// Page-level WAL (선택적)
    pub(super) page_wal: RwLock<Option<Arc<PageWAL>>>,
    /// 스토리지 유형 (WAL 로깅에 사용)
    pub(super) storage_type: PageStorageType,
    /// 수정 전 페이지 이미지 ((page_id, tx_id) -> before_image)
    /// 각 트랜잭션별로 별도의 before_image를 유지합니다.
    pub(super) before_images: RwLock<HashMap<(PageId, TransactionId), [u8; PAGE_SIZE]>>,
    /// 현재 트랜잭션 ID (설정되면 unpin_page에서 자동으로 WAL 로깅)
    current_tx_id: Mutex<Option<TransactionId>>,
}

impl BufferPoolManager {
    /// 새 버퍼 풀 관리자 생성
    pub fn new(
        disk_manager: Arc<DiskManager>,
        pool_size: usize,
        storage_type: PageStorageType,
    ) -> Self {
        let mut pages = Vec::with_capacity(pool_size);
        let mut free_list = Vec::with_capacity(pool_size);

        for i in 0..pool_size {
            pages.push(Arc::new(Mutex::new(Page::new())));
            free_list.push(i);
        }

        Self {
            disk_manager,
            pool_size,
            pages,
            page_table: RwLock::new(HashMap::new()),
            free_list: Mutex::new(free_list),
            replacer: Mutex::new(LRUReplacer::new(pool_size)),
            page_wal: RwLock::new(None),
            storage_type,
            before_images: RwLock::new(HashMap::new()),
            current_tx_id: Mutex::new(None),
        }
    }

    /// PageWAL과 함께 버퍼 풀 관리자 생성
    pub fn with_page_wal(
        disk_manager: Arc<DiskManager>,
        pool_size: usize,
        storage_type: PageStorageType,
        page_wal: Arc<PageWAL>,
    ) -> Self {
        let mut bpm = Self::new(disk_manager, pool_size, storage_type);
        bpm.page_wal = RwLock::new(Some(page_wal));
        bpm
    }

    /// 스토리지 유형 반환
    pub fn storage_type(&self) -> PageStorageType {
        self.storage_type
    }

    /// 현재 트랜잭션 ID 설정
    ///
    /// 이 값이 설정되면 unpin_page(dirty=true) 호출 시 자동으로 PageWAL에 로깅됩니다.
    /// B+Tree 등 tx_id를 직접 전달할 수 없는 코드에서 사용합니다.
    pub async fn set_current_tx(&self, tx_id: TransactionId) {
        let mut current = self.current_tx_id.lock().await;
        *current = Some(tx_id);
    }

    /// 현재 트랜잭션 ID 해제
    pub async fn clear_current_tx(&self) {
        let mut current = self.current_tx_id.lock().await;
        *current = None;
    }

    /// 현재 트랜잭션 ID 조회
    async fn get_current_tx(&self) -> Option<TransactionId> {
        let current = self.current_tx_id.lock().await;
        *current
    }

    /// 페이지 가져오기 (없으면 로드)
    ///
    /// 동시성 개선: `&self`로 호출 (내부 락 사용)
    ///
    /// current_tx_id가 설정되어 있으면 자동으로 before_image를 캡처합니다.
    pub async fn fetch_page(&self, page_id: PageId) -> Result<Arc<Mutex<Page>>> {
        // 현재 트랜잭션 ID (자동 before_image 캡처용)
        let current_tx = self.get_current_tx().await;

        // 1. 이미 풀에 있는지 확인 (읽기 락)
        {
            let page_table = self.page_table.read().await;

            if let Some(&frame_id) = page_table.get(&page_id) {
                let page_ref = self.pages[frame_id].clone();
                {
                    let mut page_guard = page_ref.lock().await;
                    page_guard.pin_count += 1;

                    // Pin되면 LRU에서 제거 (교체 대상에서 제외)
                    if page_guard.pin_count == 1 {
                        let mut replacer = self.replacer.lock().await;
                        replacer.pin(frame_id);
                    }

                    // current_tx가 있으면 before_image 캡처
                    if let Some(tx_id) = current_tx {
                        self.capture_before_image_internal(page_id, tx_id, &page_guard.data)
                            .await?;
                    }
                }

                return Ok(page_ref);
            }
        }

        // 2. 빈 프레임 확보
        let frame_id = {
            let mut free_list = self.free_list.lock().await;

            if let Some(fid) = free_list.pop() {
                fid
            } else {
                drop(free_list); // 락 해제 후 evict 호출
                self.evict().await?
            }
        };

        // 3. 디스크에서 읽기
        let page_data = self.disk_manager.read_page(page_id).await?;

        let page_ref = self.pages[frame_id].clone();
        {
            let mut page = page_ref.lock().await;
            *page = page_data;
            page.pin_count = 1;
            // 디스크에서 로드 후 헤더에서 LSN 동기화
            page.sync_lsn_from_header();

            // current_tx가 있으면 before_image 캡처
            if let Some(tx_id) = current_tx {
                self.capture_before_image_internal(page_id, tx_id, &page.data)
                    .await?;
            }
        }

        // page_table에 삽입 (쓰기 락)
        {
            let mut page_table = self.page_table.write().await;
            page_table.insert(page_id, frame_id);
        }

        Ok(page_ref.clone())
    }

    /// 새 페이지 생성
    pub async fn new_page(&self) -> Result<Arc<Mutex<Page>>> {
        let page_id = self.disk_manager.allocate_page().await?;

        // 빈 프레임 확보
        let frame_id = {
            let mut free_list = self.free_list.lock().await;

            if let Some(fid) = free_list.pop() {
                fid
            } else {
                drop(free_list);
                self.evict().await?
            }
        };

        let page_ref = self.pages[frame_id].clone();
        {
            let mut page = page_ref.lock().await;

            // 초기화
            page.reset();
            page.id = Some(page_id);
            page.pin_count = 1;
        }

        // 매핑 추가 (쓰기 락)
        {
            let mut page_table = self.page_table.write().await;
            page_table.insert(page_id, frame_id);
        }

        Ok(page_ref.clone())
    }

    /// 페이지 고정 해제 (사용 완료 시 호출 필수)
    ///
    /// current_tx_id가 설정되어 있고 is_dirty가 true이면 자동으로 PageWAL에 로깅합니다.
    /// before_image는 fetch_page에서 미리 캡처됩니다.
    pub async fn unpin_page(&self, page_id: PageId, is_dirty: bool) -> Result<()> {
        // 현재 트랜잭션 ID 가져오기 (자동 WAL 로깅용)
        let current_tx = self.get_current_tx().await;

        // dirty 페이지이고 current_tx가 있으면 WAL 로깅
        let has_wal = self.page_wal.read().await.is_some();
        if is_dirty
            && has_wal
            && let Some(tx_id) = current_tx
        {
            // WAL에 after_image 로깅
            self.log_page_modification(page_id, tx_id).await?;
        }

        // 읽기 락으로 frame_id 조회
        let frame_id = {
            let page_table = self.page_table.read().await;

            match page_table.get(&page_id) {
                Some(&fid) => fid,
                None => {
                    return Err(GraphError::Storage {
                        message: "Page not in buffer pool".to_string(),
                    });
                }
            }
        };

        let page_ref = self.pages[frame_id].clone();
        let should_unpin = {
            let mut page = page_ref.lock().await;

            if page.pin_count > 0 {
                page.pin_count -= 1;
            }
            if is_dirty {
                page.is_dirty = true;
            }

            page.pin_count == 0
        };

        // Pin count가 0이 되면 LRU에 추가 (교체 가능)
        if should_unpin {
            let mut replacer = self.replacer.lock().await;
            replacer.unpin(frame_id);
        }

        Ok(())
    }

    /// 페이지 강제 디스크 쓰기
    ///
    /// 디스크 쓰기 전에 page_lsn을 헤더에 기록합니다.
    /// 이를 통해 복구 시 불필요한 REDO를 방지합니다.
    ///
    /// 주의: B+Tree 메타데이터 페이지는 SlottedPage와 다른 레이아웃을 사용하므로
    /// LSN 기록을 건너뜁니다 (offset 4에 version 필드가 있음).
    pub async fn flush_page(&self, page_id: PageId) -> Result<()> {
        /// B+Tree 메타데이터 매직 넘버 ("ZETA" little-endian)
        const BTREE_MAGIC: &[u8] = b"ATEZ";

        // 읽기 락으로 frame_id 조회
        let frame_id = {
            let page_table = self.page_table.read().await;

            match page_table.get(&page_id) {
                Some(&fid) => fid,
                None => return Ok(()), // 페이지가 없으면 아무것도 안함
            }
        };

        let page_ref = self.pages[frame_id].clone();
        let mut page = page_ref.lock().await;

        // B+Tree metadata 페이지 감지 (offset 0에 "ATEZ" magic)
        let is_btree_metadata = page.data.len() >= 4 && &page.data[0..4] == BTREE_MAGIC;

        if !is_btree_metadata {
            // 일반 SlottedPage만 LSN 기록 (ARIES WAL 규칙)
            let lsn = page.page_lsn;
            page.write_lsn_to_header(lsn);
        }

        self.disk_manager.write_page(page_id, &page).await?;
        page.is_dirty = false;

        Ok(())
    }

    /// 모든 더티 페이지를 디스크에 기록 (체크포인트)
    pub async fn flush_all_pages(&self) -> Result<()> {
        // 읽기 락으로 page_id 목록 수집
        let page_ids: Vec<PageId> = {
            let page_table = self.page_table.read().await;
            page_table.keys().cloned().collect()
        };

        for page_id in page_ids {
            self.flush_page(page_id).await?;
        }
        Ok(())
    }

    /// LRU 정책에 따른 Victim 선정 및 축출
    ///
    /// 동시성 개선: `&self`로 호출 (내부 락 사용)
    ///
    /// ## 재시도 로직
    /// 1. 1차 시도: LRU Replacer에서 victim 선정
    /// 2. 2차 시도: 모든 dirty 페이지 flush 후 재시도
    async fn evict(&self) -> Result<FrameId> {
        // 1차 시도: LRU Replacer에서 victim 선정
        match self.try_evict_victim().await {
            Ok(Some(frame_id)) => return Ok(frame_id),
            Ok(None) => {}
            Err(e) => return Err(e),
        }

        // 2차 시도: 모든 dirty 페이지 flush 후 재시도
        tracing::warn!("Buffer pool exhausted, flushing dirty pages and retrying");
        self.flush_all_pages().await?;

        // 재시도
        match self.try_evict_victim().await {
            Ok(Some(frame_id)) => Ok(frame_id),
            Ok(None) => Err(GraphError::BufferPoolExhausted),
            Err(e) => Err(e),
        }
    }

    /// Victim 선정 시도 (내부 헬퍼)
    ///
    /// Optimistic Retry 패턴을 사용하여 race condition을 방지합니다.
    async fn try_evict_victim(&self) -> Result<Option<FrameId>> {
        const MAX_RETRIES: usize = 3;

        for attempt in 0..MAX_RETRIES {
            // 1. LRU Replacer에서 victim 선정
            let frame_id = {
                let mut replacer = self.replacer.lock().await;
                match replacer.victim() {
                    Some(fid) => fid,
                    None => return Ok(None),
                }
            };

            // 2. 원자적 검색 + 제거 (쓰기 락)
            let page_id = {
                let mut page_table = self.page_table.write().await;

                let found = page_table
                    .iter()
                    .find(|&(_, &fid)| fid == frame_id)
                    .map(|(&pid, _)| pid);

                match found {
                    Some(pid) => {
                        page_table.remove(&pid);
                        pid
                    }
                    None => {
                        if attempt < MAX_RETRIES - 1 {
                            tracing::debug!(
                                "Frame {} already evicted by another thread, retry {}/{}",
                                frame_id,
                                attempt + 1,
                                MAX_RETRIES
                            );
                            continue;
                        }
                        return Ok(None);
                    }
                }
            };

            // 3. Dirty이면 WAL flush 후 디스크에 기록
            {
                let page = self.pages[frame_id].lock().await;
                if page.is_dirty {
                    // WAL flush (Write-Ahead Logging 규칙)
                    if let Some(ref page_wal) = self.page_wal().await {
                        page_wal.flush().map_err(|e| GraphError::System {
                            message: format!("WAL flush error: {}", e),
                        })?;
                    }
                    self.disk_manager.write_page(page_id, &page).await?;
                }
            }

            // 4. before_images에서 해당 page_id의 모든 항목 제거
            {
                let mut before_images = self.before_images.write().await;
                before_images.retain(|(pid, _), _| *pid != page_id);
            }

            return Ok(Some(frame_id));
        }

        Ok(None)
    }
}

impl std::fmt::Debug for BufferPoolManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let used_frames = self.page_table.try_read().map(|pt| pt.len()).unwrap_or(0);
        let free_frames = self.free_list.try_lock().map(|fl| fl.len()).unwrap_or(0);

        f.debug_struct("BufferPoolManager")
            .field("pool_size", &self.pool_size)
            .field("used_frames", &used_frames)
            .field("free_frames", &free_frames)
            .finish()
    }
}
