//! # Page-level WAL 모듈
//!
//! ARIES 기반 Page-level WAL을 제공합니다.
//!
//! ## 핵심 개념
//!
//! - **Physical Logging**: 페이지 변경의 물리적 이미지를 기록
//! - **LSN Tracking**: 각 WAL 레코드에 고유 LSN 할당
//! - **WAL Protocol**: `page_lsn <= flushed_lsn` 보장 후에만 페이지 flush
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::transaction::page_wal::PageWAL;
//!
//! let page_wal = PageWAL::open("data/wal.log")?;
//!
//! // 페이지 수정 로깅
//! let lsn = page_wal.log_page_write(tx_id, page_id, offset, &before, &after)?;
//!
//! // WAL flush (페이지 flush 전 필수)
//! let flushed = page_wal.flush()?;
//! ```

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use super::lsn::{FlushedLSNTracker, LSN, LSNGenerator};
use super::wal::{LogRecord, PageStorageType, SlotUndoOp, WalConfig};
use super::{Result, TransactionError, TransactionId};
use crate::storage::disk::page::PageId;
use crate::storage::disk::slotted_page::SlotId;

/// Page-level WAL 레코드 (LSN 포함)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WalRecord {
    /// WAL 레코드의 LSN
    pub lsn: LSN,
    /// 로그 레코드 내용
    pub record: LogRecord,
}

/// WAL 레코드 스트리밍 Iterator
///
/// 메모리 효율적으로 WAL 레코드를 순차적으로 읽습니다.
pub struct WalRecordIterator {
    /// 읽을 파일 목록 (오래된 순서)
    files: Vec<PathBuf>,
    /// 현재 파일 인덱스
    current_file_index: usize,
    /// 현재 파일의 BufReader
    current_reader: Option<std::io::BufReader<File>>,
    /// 읽은 레코드 수
    records_read: u64,
    /// 오류 발생 여부
    has_error: bool,
    /// 현재 파일의 데이터 읽은 끝 위치 (truncate 용)
    pub(crate) valid_offset: u64,
}

impl WalRecordIterator {
    /// 새 Iterator 생성
    fn new(files: Vec<PathBuf>) -> Result<Self> {
        let mut iter = Self {
            files,
            current_file_index: 0,
            current_reader: None,
            records_read: 0,
            has_error: false,
            valid_offset: 0,
        };

        // 첫 번째 파일 열기
        iter.open_next_file()?;

        Ok(iter)
    }

    /// 다음 파일 열기
    fn open_next_file(&mut self) -> Result<bool> {
        while self.current_file_index < self.files.len() {
            let path = &self.files[self.current_file_index];
            self.current_file_index += 1;

            if path.exists() {
                let file = File::open(path).map_err(TransactionError::IoError)?;
                self.current_reader = Some(std::io::BufReader::new(file));
                self.valid_offset = 0;
                return Ok(true);
            }
        }

        self.current_reader = None;
        Ok(false)
    }

    /// 현재 reader에서 레코드 읽기
    fn read_next_record(&mut self) -> Result<Option<WalRecord>> {
        let reader = match &mut self.current_reader {
            Some(r) => r,
            None => return Ok(None),
        };

        // 1. 길이 읽기 (u32)
        let mut len_bytes = [0u8; 4];
        match reader.read_exact(&mut len_bytes) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // 현재 파일 끝 - 다음 파일로 이동하거나 torn write
                // 끝이 torn write인지 (남은 파일이 없는지) 확인
                if self.current_file_index >= self.files.len() {
                    return Ok(None);
                }
                if self.open_next_file()? {
                    return self.read_next_record();
                } else {
                    return Ok(None);
                }
            }
            Err(e) => return Err(TransactionError::IoError(e)),
        }

        let len = u32::from_le_bytes(len_bytes) as usize;

        // 2. 데이터 읽기
        let mut buf = vec![0u8; len];
        if let Err(e) = reader.read_exact(&mut buf) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                // 데이터 부분 읽기 실패 -> Torn write (가장 마지막 파일일 경우)
                if self.current_file_index >= self.files.len() {
                    return Ok(None);
                }
            }
            return Err(TransactionError::IoError(e));
        }

        // 3. 역직렬화
        let record: WalRecord = match zeta4j_codec::deserialize(&buf) {
            Ok(r) => r,
            Err(e) => {
                // 마지막 파일에서만 DeserializationError 허용 (손상된 마지막 조각일 수 있음)
                if self.current_file_index >= self.files.len() {
                    return Ok(None);
                }
                return Err(TransactionError::DeserializationError(format!(
                    "Wal record: {}",
                    e
                )));
            }
        };

        self.records_read += 1;
        self.valid_offset += 4 + len as u64; // 길이 프리픽스 + 데이터
        Ok(Some(record))
    }

    /// 읽은 레코드 수 조회
    pub fn records_read(&self) -> u64 {
        self.records_read
    }
}

impl Iterator for WalRecordIterator {
    type Item = Result<WalRecord>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.has_error {
            return None;
        }

        match self.read_next_record() {
            Ok(Some(record)) => Some(Ok(record)),
            Ok(None) => None,
            Err(e) => {
                self.has_error = true;
                Some(Err(e))
            }
        }
    }
}

/// Page-level WAL Manager
///
/// ARIES 알고리즘 기반의 Page-level WAL을 관리합니다.
/// 파일 I/O, LSN 할당, flush 관리를 통합 제공합니다.
///
/// ## 역할
///
/// - 파일 I/O: WAL 레코드 직렬화 및 디스크 쓰기
/// - LSN 할당: 각 WAL 레코드에 고유 LSN 부여
/// - Flush 관리: flushedLSN 추적
/// - 로테이션: 파일 크기 제한 시 자동 로테이션
/// - WAL 규칙 검증: `page_lsn <= flushed_lsn` 확인
#[derive(Debug)]
pub struct PageWAL {
    /// 기본 로그 파일 경로
    log_path: PathBuf,
    /// 로그 쓰기용 버퍼
    writer: Mutex<BufWriter<File>>,
    /// 현재 로그 파일 크기 (바이트)
    current_size: AtomicU64,
    /// WAL 설정
    config: WalConfig,
    /// LSN 생성기
    lsn_gen: LSNGenerator,
    /// Flushed LSN 추적기
    flushed_lsn: FlushedLSNTracker,
}

impl PageWAL {
    /// 새 PageWAL 열기
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_config(path, WalConfig::default())
    }

    /// 설정과 함께 PageWAL 열기
    pub fn open_with_config<P: AsRef<Path>>(path: P, config: WalConfig) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        // 상위 디렉토리 생성
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        // 현재 파일 크기 확인
        let current_size = file.metadata()?.len();

        Ok(Self {
            log_path: path,
            writer: Mutex::new(BufWriter::new(file)),
            current_size: AtomicU64::new(current_size),
            config,
            lsn_gen: LSNGenerator::new(),
            flushed_lsn: FlushedLSNTracker::new(),
        })
    }

    /// 기존 WAL에서 복구하여 열기
    ///
    /// WAL을 스캔하여 마지막 LSN을 찾고 LSN 생성기를 복원합니다.
    pub fn open_with_recovery<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut wal = Self::open(path.as_ref())?;

        // WAL을 스캔하여 마지막 LSN 찾기
        let mut max_lsn = 0;
        let mut iterator = wal.iter()?;

        for result in iterator.by_ref() {
            {
                let wal_record = result?;
                if wal_record.lsn > max_lsn {
                    max_lsn = wal_record.lsn;
                }
            }
        }

        // 손상된 로그 파일 (Torn write) 부분 자르기 (iterator의 현재 파일이 마지막 파일일 경우)
        let valid_offset = iterator.valid_offset;

        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .open(path.as_ref())?;

        if valid_offset < file.metadata()?.len() {
            file.set_len(valid_offset)?;
            // 버퍼 라이터 재설정
            let file = OpenOptions::new()
                .append(true)
                .read(true)
                .open(path.as_ref())?;
            wal.writer = Mutex::new(BufWriter::new(file));
            wal.current_size.store(valid_offset, Ordering::Relaxed);
        }

        if max_lsn > 0 {
            wal.lsn_gen.restore(max_lsn);
        }

        Ok(wal)
    }

    // ==================== 파일 I/O ====================

    /// 로그 레코드 기록 (내부용)
    fn append(&self, lsn: LSN, record: &LogRecord) -> Result<()> {
        let wal_record = WalRecord {
            lsn,
            record: record.clone(),
        };
        // 직렬화 (Postcard 사용)
        let data = zeta4j_codec::serialize(&wal_record)
            .map_err(|e| TransactionError::SerializationError(e.to_string()))?;
        let record_size = 4 + data.len() as u64;

        // 로테이션 필요 여부 확인
        if self.config.rotation_enabled {
            let current = self.current_size.load(Ordering::Relaxed);
            if current + record_size > self.config.max_file_size {
                self.rotate()?;
            }
        }

        let mut writer = self
            .writer
            .lock()
            .map_err(|_| TransactionError::lock_poisoned("WAL writer"))?;

        // 길이 프리픽스 작성 (u32)
        let len = data.len() as u32;
        writer.write_all(&len.to_le_bytes())?;
        writer.write_all(&data)?;

        // 현재 크기 업데이트
        self.current_size.fetch_add(record_size, Ordering::Relaxed);

        Ok(())
    }

    /// 로그 파일 로테이션
    fn rotate(&self) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| TransactionError::lock_poisoned("WAL writer"))?;

        // 1. 현재 버퍼 플러시
        writer.flush()?;
        writer.get_ref().sync_all()?;

        // 2. 오래된 아카이브 파일 삭제
        self.cleanup_old_archives()?;

        // 3. 기존 아카이브 파일 번호 증가
        self.shift_archives()?;

        // 4. 현재 로그 파일을 .1로 이동
        let archive_path = self.get_archive_path(1);
        if self.log_path.exists() {
            std::fs::rename(&self.log_path, &archive_path)?;
        }

        // 5. 새 로그 파일 생성
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&self.log_path)?;

        *writer = BufWriter::new(file);
        self.current_size.store(0, Ordering::Relaxed);

        Ok(())
    }

    /// 아카이브 파일 경로 생성
    fn get_archive_path(&self, index: usize) -> PathBuf {
        let mut path = self.log_path.clone();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("wal.log");
        path.set_file_name(format!("{}.{}", file_name, index));
        path
    }

    /// 아카이브 파일 번호 이동 (1 → 2, 2 → 3, ...)
    fn shift_archives(&self) -> Result<()> {
        for i in (1..self.config.max_files).rev() {
            let old_path = self.get_archive_path(i);
            let new_path = self.get_archive_path(i + 1);
            if old_path.exists() {
                std::fs::rename(&old_path, &new_path)?;
            }
        }
        Ok(())
    }

    /// 오래된 아카이브 파일 삭제
    fn cleanup_old_archives(&self) -> Result<()> {
        for i in self.config.max_files.. {
            let path = self.get_archive_path(i);
            if path.exists() {
                std::fs::remove_file(&path)?;
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Iterator용 파일 목록 수집 (오래된 순서)
    fn collect_files_for_iteration(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();

        // 아카이브 파일들 (오래된 순서: .N, .N-1, ..., .1)
        for i in (1..=self.config.max_files).rev() {
            let path = self.get_archive_path(i);
            if path.exists() {
                files.push(path);
            }
        }

        // 현재 로그 파일
        if self.log_path.exists() {
            files.push(self.log_path.clone());
        }

        files
    }

    // ==================== Page-level 로깅 ====================

    /// 페이지 수정 로깅 (PageWrite)
    ///
    /// 페이지 변경을 WAL에 기록하고 LSN을 반환합니다.
    pub fn log_page_write(
        &self,
        tx_id: TransactionId,
        storage_type: PageStorageType,
        page_id: PageId,
        offset: u16,
        before: &[u8],
        after: &[u8],
    ) -> Result<LSN> {
        if before.len() != after.len() {
            return Err(TransactionError::InvalidArgument(
                "before and after images must have the same length".to_string(),
            ));
        }

        let lsn = self.lsn_gen.next();
        let length = before.len() as u16;

        let record = LogRecord::PageWrite {
            tx_id,
            storage_type,
            page_id,
            offset,
            length,
            before_image: before.to_vec(),
            after_image: after.to_vec(),
        };

        self.append(lsn, &record)?;

        Ok(lsn)
    }

    /// CLR (Compensation Log Record) 기록
    pub fn log_clr(
        &self,
        tx_id: TransactionId,
        storage_type: PageStorageType,
        undo_next_lsn: LSN,
        page_id: PageId,
        offset: u16,
        after: &[u8],
    ) -> Result<LSN> {
        let lsn = self.lsn_gen.next();
        let length = after.len() as u16;

        let record = LogRecord::CLR {
            tx_id,
            storage_type,
            undo_next_lsn,
            page_id,
            offset,
            length,
            after_image: after.to_vec(),
        };

        self.append(lsn, &record)?;

        Ok(lsn)
    }

    /// 트랜잭션 Begin 로깅
    pub fn log_begin(&self, tx_id: TransactionId) -> Result<LSN> {
        let lsn = self.lsn_gen.next();
        self.append(lsn, &LogRecord::Begin(tx_id))?;
        Ok(lsn)
    }

    /// 트랜잭션 Commit 로깅
    pub fn log_commit(&self, tx_id: TransactionId) -> Result<LSN> {
        let lsn = self.lsn_gen.next();
        self.append(lsn, &LogRecord::Commit(tx_id))?;
        Ok(lsn)
    }

    /// 트랜잭션 Abort 로깅
    pub fn log_abort(&self, tx_id: TransactionId) -> Result<LSN> {
        let lsn = self.lsn_gen.next();
        self.append(lsn, &LogRecord::Abort(tx_id))?;
        Ok(lsn)
    }

    /// BeginCheckpoint 로깅
    pub fn log_begin_checkpoint(&self) -> Result<LSN> {
        let lsn = self.lsn_gen.next();
        self.append(lsn, &LogRecord::BeginCheckpoint)?;
        Ok(lsn)
    }

    /// EndCheckpoint 로깅
    pub fn log_end_checkpoint(
        &self,
        active_transactions: Vec<(TransactionId, LSN)>,
        dirty_page_table: Vec<(PageStorageType, PageId, LSN)>,
    ) -> Result<LSN> {
        let lsn = self.lsn_gen.next();
        self.append(
            lsn,
            &LogRecord::EndCheckpoint {
                active_transactions,
                dirty_page_table,
            },
        )?;
        Ok(lsn)
    }

    // ==================== Slot-level 로깅 ====================

    /// 슬롯 삽입 로깅
    pub fn log_slot_insert(
        &self,
        tx_id: TransactionId,
        storage_type: PageStorageType,
        page_id: PageId,
        slot_id: SlotId,
        data: &[u8],
    ) -> Result<LSN> {
        let lsn = self.lsn_gen.next();

        let record = LogRecord::SlotInsert {
            tx_id,
            storage_type,
            page_id,
            slot_id,
            data: data.to_vec(),
        };

        self.append(lsn, &record)?;
        Ok(lsn)
    }

    /// 슬롯 삭제 로깅
    pub fn log_slot_delete(
        &self,
        tx_id: TransactionId,
        storage_type: PageStorageType,
        page_id: PageId,
        slot_id: SlotId,
        data: &[u8],
    ) -> Result<LSN> {
        let lsn = self.lsn_gen.next();

        let record = LogRecord::SlotDelete {
            tx_id,
            storage_type,
            page_id,
            slot_id,
            data: data.to_vec(),
        };

        self.append(lsn, &record)?;
        Ok(lsn)
    }

    /// 슬롯 업데이트 로깅
    pub fn log_slot_update(
        &self,
        tx_id: TransactionId,
        storage_type: PageStorageType,
        page_id: PageId,
        slot_id: SlotId,
        before: &[u8],
        after: &[u8],
    ) -> Result<LSN> {
        let lsn = self.lsn_gen.next();

        let record = LogRecord::SlotUpdate {
            tx_id,
            storage_type,
            page_id,
            slot_id,
            before_data: before.to_vec(),
            after_data: after.to_vec(),
        };

        self.append(lsn, &record)?;
        Ok(lsn)
    }

    /// Slot-level CLR 로깅
    pub fn log_slot_clr(
        &self,
        tx_id: TransactionId,
        storage_type: PageStorageType,
        undo_next_lsn: LSN,
        page_id: PageId,
        slot_id: SlotId,
        undo_op: SlotUndoOp,
    ) -> Result<LSN> {
        let lsn = self.lsn_gen.next();

        let record = LogRecord::SlotCLR {
            tx_id,
            storage_type,
            undo_next_lsn,
            page_id,
            slot_id,
            undo_op,
        };

        self.append(lsn, &record)?;
        Ok(lsn)
    }

    // ==================== Flush & LSN 관리 ====================

    /// WAL Flush
    ///
    /// 버퍼에 있는 모든 WAL 레코드를 디스크에 기록합니다.
    pub fn flush(&self) -> Result<LSN> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| TransactionError::lock_poisoned("WAL writer"))?;
        writer.flush()?;
        writer.get_ref().sync_all()?;

        // 마지막으로 발급된 LSN으로 flushedLSN 업데이트
        let flushed = self.lsn_gen.last_issued();
        self.flushed_lsn.update(flushed);

        Ok(flushed)
    }

    /// 현재 flushedLSN 조회
    #[inline]
    pub fn flushed_lsn(&self) -> LSN {
        self.flushed_lsn.get()
    }

    /// 다음에 발급될 LSN 조회
    #[inline]
    pub fn current_lsn(&self) -> LSN {
        self.lsn_gen.current()
    }

    /// 마지막으로 발급된 LSN 조회
    #[inline]
    pub fn last_lsn(&self) -> LSN {
        self.lsn_gen.last_issued()
    }

    /// 페이지 flush 가능 여부 확인 (WAL 규칙)
    #[inline]
    pub fn can_flush_page(&self, page_lsn: LSN) -> bool {
        self.flushed_lsn.can_flush_page(page_lsn)
    }

    /// LSN 생성기 복원 (Recovery 후)
    pub fn restore_lsn(&self, last_lsn: LSN) {
        self.lsn_gen.restore(last_lsn);
    }

    // ==================== 읽기 ====================

    /// WAL 레코드 Iterator
    pub fn iter(&self) -> Result<WalRecordIterator> {
        // 버퍼 플러시 (최신 데이터 보장)
        if let Err(e) = self.flush() {
            tracing::warn!(error = %e, "Flush failed before WAL iter() - stale data may be returned");
        }
        let files = self.collect_files_for_iteration();
        WalRecordIterator::new(files)
    }

    /// 모든 로그 읽기 (Recovery용)
    pub fn read_all(&self) -> Result<Vec<LogRecord>> {
        let mut records = Vec::new();
        for result in self.iter()? {
            records.push(result?.record);
        }
        Ok(records)
    }

    /// 특정 트랜잭션의 데이터 변경 로그 레코드만 수집 (UNDO용)
    ///
    /// # 최적화 (v1.11.0)
    ///
    /// 기존 `read_all()` 방식은 모든 WAL 레코드를 메모리에 로드하여
    /// O(WAL_size) 메모리를 소비했습니다. Iterator 방식으로 변경하여
    /// 필요한 레코드만 수집합니다.
    pub fn get_transaction_records(&self, tx_id: TransactionId) -> Result<Vec<LogRecord>> {
        let mut records = Vec::new();
        for result in self.iter()? {
            let record = result?.record;
            if record.transaction_id() == tx_id && record.is_data_modification() {
                records.push(record);
            }
        }
        Ok(records)
    }

    /// 현재 로그 파일 크기 조회
    pub fn current_file_size(&self) -> u64 {
        self.current_size.load(Ordering::Relaxed)
    }

    /// 모든 로그 파일 삭제 (현재 + 아카이브)
    pub fn truncate_all(&self) -> Result<()> {
        // 1. 현재 파일 truncate
        {
            let mut writer = self
                .writer
                .lock()
                .map_err(|_| TransactionError::lock_poisoned("WAL writer"))?;

            let file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&self.log_path)?;

            *writer = BufWriter::new(file);
            self.current_size.store(0, Ordering::Relaxed);
        }

        // 2. 모든 아카이브 파일 삭제
        for i in 1..=self.config.max_files {
            let path = self.get_archive_path(i);
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
        }

        Ok(())
    }

    /// WAL 설정 조회
    pub fn config(&self) -> &WalConfig {
        &self.config
    }
}

/// PageWAL Builder (설정 구성용)
pub struct PageWALBuilder {
    config: WalConfig,
    start_lsn: Option<LSN>,
}

impl PageWALBuilder {
    /// 새 Builder 생성
    pub fn new() -> Self {
        Self {
            config: WalConfig::default(),
            start_lsn: None,
        }
    }

    /// WAL 설정 지정
    pub fn config(mut self, config: WalConfig) -> Self {
        self.config = config;
        self
    }

    /// 시작 LSN 지정 (Recovery 시)
    pub fn start_lsn(mut self, lsn: LSN) -> Self {
        self.start_lsn = Some(lsn);
        self
    }

    /// 로테이션 비활성화
    pub fn no_rotation(mut self) -> Self {
        self.config.rotation_enabled = false;
        self
    }

    /// 최대 파일 크기 설정
    pub fn max_file_size(mut self, size: u64) -> Self {
        self.config.max_file_size = size;
        self
    }

    /// PageWAL 생성
    pub fn open<P: AsRef<Path>>(self, path: P) -> Result<PageWAL> {
        let path = path.as_ref().to_path_buf();

        // 상위 디렉토리 생성
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        let current_size = file.metadata()?.len();

        let lsn_gen = match self.start_lsn {
            Some(lsn) => LSNGenerator::with_start(lsn),
            None => LSNGenerator::new(),
        };

        Ok(PageWAL {
            log_path: path,
            writer: Mutex::new(BufWriter::new(file)),
            current_size: AtomicU64::new(current_size),
            config: self.config,
            lsn_gen,
            flushed_lsn: FlushedLSNTracker::new(),
        })
    }
}

impl Default for PageWALBuilder {
    fn default() -> Self {
        Self::new()
    }
}
