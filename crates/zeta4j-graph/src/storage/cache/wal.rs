//! PageWAL 통합 메서드
//!
//! Page-level WAL과의 통합을 위한 기능을 제공합니다.
//! - before/after 이미지 캡처
//! - Physiological Logging (변경된 영역만 기록)
//! - 트랜잭션별 before_image 관리

use std::sync::Arc;

use crate::error::{GraphError, Result};
use crate::transaction::lsn::LSN;
use crate::transaction::{PageWAL, TransactionId};

use super::pool::BufferPoolManager;
use crate::storage::disk::page::{PAGE_SIZE, Page, PageId};
use tokio::sync::Mutex;

impl BufferPoolManager {
    /// PageWAL 설정 (생성 후 설정)
    pub async fn set_page_wal(&self, page_wal: Arc<PageWAL>) {
        let mut lock = self.page_wal.write().await;
        *lock = Some(page_wal);
    }

    /// PageWAL 참조 반환
    pub async fn page_wal(&self) -> Option<Arc<PageWAL>> {
        self.page_wal.read().await.clone()
    }

    /// 페이지 수정 전 before image 저장
    ///
    /// 페이지를 수정하기 전에 호출하여 UNDO를 위한 원본 데이터를 저장합니다.
    /// 이미 저장된 before image가 있으면 무시합니다 (첫 수정만 기록).
    pub async fn capture_before_image(&self, page_id: PageId, tx_id: TransactionId) -> Result<()> {
        // PageWAL이 없으면 무시
        if self.page_wal.read().await.is_none() {
            return Ok(());
        }

        let key = (page_id, tx_id);

        // 이 (page_id, tx_id) 조합의 before image가 이미 있으면 무시
        {
            let before_images = self.before_images.read().await;
            if before_images.contains_key(&key) {
                return Ok(());
            }
        }

        // 페이지 데이터 복사
        let frame_id = {
            let page_table = self.page_table.read().await;
            match page_table.get(&page_id) {
                Some(&fid) => fid,
                None => return Ok(()), // 버퍼풀에 없으면 무시
            }
        };

        let page_ref = self.pages[frame_id].clone();
        let page = page_ref.lock().await;

        let mut before_image = [0u8; PAGE_SIZE];
        before_image.copy_from_slice(&page.data);

        drop(page);

        // before image 저장
        {
            let mut before_images = self.before_images.write().await;
            before_images.insert(key, before_image);
        }

        Ok(())
    }

    /// 페이지 수정 전 before image 저장 (데이터 직접 전달)
    ///
    /// fetch_page 내에서 호출되며, 이미 락을 잡고 있는 상태에서 사용합니다.
    pub(super) async fn capture_before_image_internal(
        &self,
        page_id: PageId,
        tx_id: TransactionId,
        data: &[u8; PAGE_SIZE],
    ) -> Result<()> {
        // PageWAL이 없으면 무시
        if self.page_wal.read().await.is_none() {
            return Ok(());
        }

        let key = (page_id, tx_id);

        // 이 (page_id, tx_id) 조합의 before image가 이미 있으면 무시
        {
            let before_images = self.before_images.read().await;
            if before_images.contains_key(&key) {
                return Ok(());
            }
        }

        // before image 저장
        let mut before_image = [0u8; PAGE_SIZE];
        before_image.copy_from_slice(data);

        {
            let mut before_images = self.before_images.write().await;
            before_images.insert(key, before_image);
        }

        Ok(())
    }

    /// 페이지 수정을 WAL에 기록 (Physiological Logging)
    ///
    /// 페이지가 dirty로 마킹될 때 호출합니다.
    /// before image와 현재 페이지 데이터를 비교하여 **변경된 영역만** WAL에 기록합니다.
    pub async fn log_page_modification(
        &self,
        page_id: PageId,
        tx_id: TransactionId,
    ) -> Result<Option<LSN>> {
        let wal_lock = self.page_wal.read().await;
        let page_wal = match wal_lock.as_ref() {
            Some(wal) => wal,
            None => return Ok(None),
        };

        let key = (page_id, tx_id);

        // before image 가져오기
        let before_image = {
            let before_images = self.before_images.read().await;
            match before_images.get(&key) {
                Some(img) => *img,
                None => return Ok(None), // before image 없으면 로깅 안 함
            }
        };

        // 현재 페이지 데이터 가져오기
        let frame_id = {
            let page_table = self.page_table.read().await;
            match page_table.get(&page_id) {
                Some(&fid) => fid,
                None => return Ok(None),
            }
        };

        let page_ref = self.pages[frame_id].clone();
        let page = page_ref.lock().await;

        // 변경된 영역 찾기 (Physiological Logging)
        let changed_regions = Self::find_changed_regions(&before_image, &page.data);

        if changed_regions.is_empty() {
            return Ok(None); // 변경 없음
        }

        // 각 변경 영역을 WAL에 기록
        let mut last_lsn = None;
        for (offset, length) in changed_regions {
            let before_slice = &before_image[offset..offset + length];
            let after_slice = &page.data[offset..offset + length];

            let lsn = page_wal
                .log_page_write(
                    tx_id,
                    self.storage_type,
                    page_id,
                    offset as u16,
                    before_slice,
                    after_slice,
                )
                .map_err(|e| GraphError::System {
                    message: format!("WAL error: {}", e),
                })?;
            last_lsn = Some(lsn);
        }

        Ok(last_lsn)
    }

    /// before_image와 after_image를 비교하여 변경된 영역 찾기
    ///
    /// 반환값: (offset, length) 쌍의 벡터
    /// 연속된 변경 바이트를 하나의 영역으로 병합합니다.
    fn find_changed_regions(
        before: &[u8; PAGE_SIZE],
        after: &[u8; PAGE_SIZE],
    ) -> Vec<(usize, usize)> {
        let mut regions = Vec::new();
        let mut i = 0;

        while i < PAGE_SIZE {
            // 변경된 바이트 찾기
            if before[i] != after[i] {
                let start = i;
                // 연속된 변경 영역 끝까지 이동
                while i < PAGE_SIZE && before[i] != after[i] {
                    i += 1;
                }
                // 최소 병합 거리: 8바이트 이내의 동일 영역은 병합
                while i < PAGE_SIZE {
                    let mut gap = 0;
                    while i + gap < PAGE_SIZE && before[i + gap] == after[i + gap] {
                        gap += 1;
                        if gap > 8 {
                            break;
                        }
                    }
                    if gap > 8 || i + gap >= PAGE_SIZE {
                        break;
                    }
                    i += gap;
                    while i < PAGE_SIZE && before[i] != after[i] {
                        i += 1;
                    }
                }
                regions.push((start, i - start));
            } else {
                i += 1;
            }
        }

        regions
    }

    /// 트랜잭션 커밋 시 before images 정리
    pub async fn clear_before_images_for_tx(&self, tx_id: TransactionId) -> Result<()> {
        let mut before_images = self.before_images.write().await;

        // (page_id, tx_id) 키에서 tx_id가 일치하는 항목 제거
        before_images.retain(|(_, tid), _| *tid != tx_id);
        Ok(())
    }

    /// 페이지 수정 및 WAL 기록을 한 번에 수행
    pub async fn mark_dirty_and_log(
        &self,
        page_id: PageId,
        tx_id: TransactionId,
    ) -> Result<Option<LSN>> {
        // 1. before image 캡처
        self.capture_before_image(page_id, tx_id).await?;

        // 2. WAL 기록
        let lsn = self.log_page_modification(page_id, tx_id).await?;

        // 3. LSN 업데이트 및 dirty 마킹
        if let Some(lsn) = lsn {
            self.mark_dirty_with_lsn(page_id, lsn).await?;
        }

        Ok(lsn)
    }

    /// 트랜잭션 컨텍스트와 함께 페이지 고정 해제
    ///
    /// is_dirty가 true이면 WAL에 before/after 이미지를 기록합니다.
    pub async fn unpin_page_with_tx(
        &self,
        page_id: PageId,
        is_dirty: bool,
        tx_id: TransactionId,
    ) -> Result<()> {
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

        // Dirty이면 WAL 기록
        if is_dirty && let Err(e) = self.mark_dirty_and_log(page_id, tx_id).await {
            eprintln!(
                "[BufferPoolManager] WAL logging failed for page {}: {}",
                page_id, e
            );
        }

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

    /// 트랜잭션 컨텍스트와 함께 페이지 가져오기 (쓰기용)
    ///
    /// 페이지를 가져오고 before image를 자동으로 캡처합니다.
    pub async fn fetch_page_for_write(
        &self,
        page_id: PageId,
        tx_id: TransactionId,
    ) -> Result<Arc<Mutex<Page>>> {
        let page = self.fetch_page(page_id).await?;

        // before image 캡처
        self.capture_before_image(page_id, tx_id).await?;

        Ok(page)
    }
}
