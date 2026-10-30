use super::page::{PAGE_SIZE, Page, PageId};
use crate::error::{GraphError, Result};
use std::path::Path;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

/// 디스크 관리자: 데이터베이스 파일 I/O 담당
pub struct DiskManager {
    file: Mutex<File>,
    #[allow(dead_code)] // 디버깅/로깅용 보존
    path: String,
    #[allow(dead_code)] // 통계용 보존
    num_writes: u64,
}

impl std::fmt::Debug for DiskManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiskManager")
            .field("path", &self.path)
            .field("num_writes", &self.num_writes)
            .finish()
    }
}

impl DiskManager {
    pub async fn new(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.as_ref())
            .await?;

        Ok(Self {
            file: Mutex::new(file),
            path: path.as_ref().to_string_lossy().to_string(),
            num_writes: 0,
        })
    }

    /// 페이지 쓰기
    pub async fn write_page(&self, page_id: PageId, page: &Page) -> Result<()> {
        let mut file = self.file.lock().await;

        let offset = page_id as u64 * PAGE_SIZE as u64;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        file.write_all(&page.data).await?;
        file.flush().await?;

        Ok(())
    }

    /// 페이지 읽기
    pub async fn read_page(&self, page_id: PageId) -> Result<Page> {
        let mut file = self.file.lock().await;

        let offset = page_id as u64 * PAGE_SIZE as u64;
        let file_len = file.metadata().await?.len();

        // 파일 크기가 부족하면 확장 (WAL 복구 시나리오)
        // - 새 페이지 할당 후 디스크에 flush되기 전 크래시 발생 시
        // - WAL에는 해당 페이지 작업이 기록되어 있으나 실제 파일에는 없음
        if offset >= file_len {
            let required_len = offset + PAGE_SIZE as u64;
            file.set_len(required_len).await?;

            // 빈 페이지 반환
            let mut page = Page::new();
            page.id = Some(page_id);
            return Ok(page);
        }

        file.seek(std::io::SeekFrom::Start(offset)).await?;

        let mut page = Page::new();
        page.id = Some(page_id);
        file.read_exact(&mut page.data).await?;

        Ok(page)
    }

    /// 새 페이지 할당
    pub async fn allocate_page(&self) -> Result<PageId> {
        let mut file = self.file.lock().await;
        let len = file.metadata().await?.len();

        if len % PAGE_SIZE as u64 != 0 {
            return Err(GraphError::Storage {
                message: "Corrupted DB file: length not multiple of page size".to_string(),
            });
        }

        let page_id = (len / PAGE_SIZE as u64) as PageId;

        // 파일 크기 확보 (제일 끝으로 이동 후 쓰기)
        file.seek(std::io::SeekFrom::Start(len)).await?;
        file.write_all(&[0; PAGE_SIZE]).await?;
        file.flush().await?;

        Ok(page_id)
    }

    pub async fn file_size(&self) -> Result<u64> {
        let file = self.file.lock().await;
        Ok(file.metadata().await?.len())
    }
}
