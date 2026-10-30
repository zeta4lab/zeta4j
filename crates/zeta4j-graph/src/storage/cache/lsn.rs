//! LSN (Log Sequence Number) 지원 메서드
//!
//! Page-level WAL을 위한 LSN 추적 기능을 제공합니다.

use crate::error::{GraphError, Result};
use crate::storage::disk::page::PageId;
use crate::transaction::PageStorageType;
use crate::transaction::lsn::LSN;

use super::pool::BufferPoolManager;
use super::types::BufferPoolStats;

impl BufferPoolManager {
    /// 페이지를 dirty로 마킹하고 LSN 업데이트 (Page-level WAL용)
    ///
    /// 페이지 수정 시 WAL 기록 후 이 메서드를 호출합니다.
    /// 처음 dirty가 될 때 rec_lsn을 설정합니다.
    pub async fn mark_dirty_with_lsn(&self, page_id: PageId, lsn: LSN) -> Result<()> {
        let frame_id = {
            let page_table = self.page_table.read().await;

            match page_table.get(&page_id) {
                Some(&fid) => fid,
                None => {
                    return Err(GraphError::Storage {
                        message: format!("Page {} not in buffer pool", page_id),
                    });
                }
            }
        };

        let page_ref = self.pages[frame_id].clone();
        let mut page = page_ref.lock().await;

        page.mark_dirty_with_lsn(lsn);

        Ok(())
    }

    /// WAL 규칙에 따라 페이지 flush 가능 여부 확인
    ///
    /// ARIES WAL 규칙: `page_lsn <= flushed_lsn` 이면 flush 가능
    pub async fn can_flush_page(&self, page_id: PageId, flushed_lsn: LSN) -> Result<bool> {
        let frame_id = {
            let page_table = self.page_table.read().await;

            match page_table.get(&page_id) {
                Some(&fid) => fid,
                None => return Ok(true), // 버퍼풀에 없으면 flush 필요 없음
            }
        };

        let page_ref = self.pages[frame_id].clone();
        let page = page_ref.lock().await;

        Ok(page.page_lsn <= flushed_lsn)
    }

    /// WAL 규칙을 확인하고 페이지 flush (Page-level WAL용)
    ///
    /// WAL 규칙을 위반하면 에러를 반환합니다.
    pub async fn flush_page_with_wal_check(&self, page_id: PageId, flushed_lsn: LSN) -> Result<()> {
        let frame_id = {
            let page_table = self.page_table.read().await;

            match page_table.get(&page_id) {
                Some(&fid) => fid,
                None => return Ok(()),
            }
        };

        let page_ref = self.pages[frame_id].clone();
        let mut page = page_ref.lock().await;

        // WAL 규칙 확인
        if page.page_lsn > flushed_lsn {
            return Err(GraphError::Storage {
                message: format!(
                    "WAL rule violation: page_lsn({}) > flushed_lsn({})",
                    page.page_lsn, flushed_lsn
                ),
            });
        }

        if page.is_dirty {
            // 디스크 쓰기 전 헤더에 LSN 기록
            let lsn = page.page_lsn;
            page.write_lsn_to_header(lsn);
            self.disk_manager.write_page(page_id, &page).await?;
            page.mark_clean();
        }

        Ok(())
    }

    /// 모든 dirty 페이지 목록 반환 (Checkpoint용)
    ///
    /// 반환값: (storage_type, page_id, rec_lsn) 튜플 목록
    /// rec_lsn은 해당 페이지가 dirty 상태가 된 첫 번째 LSN
    pub async fn get_dirty_pages(&self) -> Result<Vec<(PageStorageType, PageId, LSN)>> {
        let page_table = self.page_table.read().await;

        let mut dirty_pages = Vec::new();

        for (&page_id, &frame_id) in page_table.iter() {
            let page_ref = self.pages[frame_id].clone();
            let page = page_ref.lock().await;

            if page.is_dirty
                && let Some(rec_lsn) = page.rec_lsn
            {
                dirty_pages.push((self.storage_type, page_id, rec_lsn));
            }
        }

        Ok(dirty_pages)
    }

    /// 페이지의 현재 LSN 조회
    pub async fn get_page_lsn(&self, page_id: PageId) -> Result<LSN> {
        let frame_id = {
            let page_table = self.page_table.read().await;

            match page_table.get(&page_id) {
                Some(&fid) => fid,
                None => {
                    return Err(GraphError::Storage {
                        message: format!("Page {} not in buffer pool", page_id),
                    });
                }
            }
        };

        let page_ref = self.pages[frame_id].clone();
        let page = page_ref.lock().await;

        Ok(page.page_lsn)
    }

    /// 버퍼 풀 통계 정보
    pub async fn stats(&self) -> BufferPoolStats {
        let mut pinned_count = 0;
        for p in &self.pages {
            if p.lock().await.pin_count > 0 {
                pinned_count += 1;
            }
        }

        let used_frames = self.page_table.read().await.len();
        let free_frames = self.free_list.lock().await.len();
        let evictable_frames = self.replacer.lock().await.size();

        BufferPoolStats {
            pool_size: self.pool_size,
            used_frames,
            free_frames,
            pinned_frames: pinned_count,
            evictable_frames,
        }
    }

    /// 버퍼 풀 사용률 반환 (0.0 ~ 1.0)
    pub async fn usage_ratio(&self) -> f64 {
        let stats = self.stats().await;
        if stats.pool_size == 0 {
            return 1.0;
        }
        stats.used_frames as f64 / stats.pool_size as f64
    }

    /// 버퍼 풀 압박 상태 확인
    ///
    /// 사용률이 threshold를 초과하면 true를 반환합니다.
    /// threshold 기본값 권장: 0.8 (80%)
    pub async fn is_under_pressure(&self, threshold: f64) -> bool {
        self.usage_ratio().await > threshold
    }

    /// 버퍼 풀 크기 반환
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }
}
