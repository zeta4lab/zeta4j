// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 인덱스 관리 모듈
//!
//! Tantivy 인덱스의 생성, 읽기, 쓰기를 관리합니다.

pub mod reader;
pub mod schema;
pub mod writer;

use parking_lot::RwLock;
use redb::Database;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;
use tantivy::{Index, IndexReader as TantivyReader, ReloadPolicy};
use tracing::{debug, info};

use crate::error::{FulltextError, Result};
use crate::tokenizer::router::LanguageRouter;
use reader::IndexReader;
use schema::ChunkSchema;
use writer::IndexWriter;

/// 인덱스 상태
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexHealth {
    /// 정상 상태
    Healthy,
    /// 손상됨 (복구 필요)
    Corrupted {
        /// 손상 세부 정보
        details: String,
        /// 복구 가능 여부
        recoverable: bool,
    },
    /// 재구축 필요
    NeedsRebuild {
        /// 이유
        reason: String,
    },
}

/// 재시도 설정
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// 최대 재시도 횟수
    pub max_retries: u32,
    /// 초기 지연 시간 (밀리초)
    pub initial_delay_ms: u64,
    /// 최대 지연 시간 (밀리초)
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
        }
    }
}

impl RetryConfig {
    /// 최대 재시도 횟수 설정
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// 초기 지연 시간 설정
    pub fn with_initial_delay_ms(mut self, delay_ms: u64) -> Self {
        self.initial_delay_ms = delay_ms;
        self
    }

    /// 최대 지연 시간 설정
    pub fn with_max_delay_ms(mut self, delay_ms: u64) -> Self {
        self.max_delay_ms = delay_ms;
        self
    }

    /// 재시도 횟수에 따른 지연 시간 계산 (exponential backoff)
    pub fn delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        let delay = self
            .initial_delay_ms
            .saturating_mul(2u64.saturating_pow(attempt));
        std::time::Duration::from_millis(delay.min(self.max_delay_ms))
    }
}

/// 전문 검색 인덱스 설정
#[derive(Debug, Clone)]
pub struct FulltextConfig {
    /// 힙 메모리 크기 (bytes)
    pub heap_size: usize,
    /// 커밋 버퍼 크기
    pub commit_buffer_size: usize,
    /// 기본 언어 (자동 감지 실패 시)
    pub default_language: String,
    /// 언어 자동 감지 활성화
    pub auto_detect_language: bool,
    /// 자동 커밋 간격 (초, 0이면 비활성화)
    pub auto_commit_interval_secs: u64,
    /// 자동 커밋 문서 수 (0이면 비활성화)
    pub auto_commit_doc_count: usize,
}

impl Default for FulltextConfig {
    fn default() -> Self {
        Self {
            heap_size: 50_000_000, // 50MB
            commit_buffer_size: 1000,
            default_language: "en".to_string(),
            auto_detect_language: true,
            auto_commit_interval_secs: 30, // 30초마다 자동 커밋
            auto_commit_doc_count: 1000,   // 1000개 문서마다 자동 커밋
        }
    }
}

/// 시스템 메모리 기반 힙 크기 계산
///
/// # Returns
///
/// 사용 가능 메모리의 5% (최소 50MB, 최대 2GB)
pub fn calculate_heap_size() -> usize {
    const MIN_HEAP: usize = 50_000_000; // 50MB
    const MAX_HEAP: usize = 2_000_000_000; // 2GB
    const HEAP_PERCENTAGE: f64 = 0.05; // 5%

    // 시스템 메모리 정보 가져오기 시도
    #[cfg(target_os = "linux")]
    fn get_available_memory() -> Option<usize> {
        use std::fs;
        let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
        for line in meminfo.lines() {
            if line.starts_with("MemAvailable:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    return parts[1].parse::<usize>().ok().map(|kb| kb * 1024);
                }
            }
        }
        None
    }

    #[cfg(target_os = "macos")]
    fn get_available_memory() -> Option<usize> {
        // macOS에서는 sysctl 사용
        use std::process::Command;
        let output = Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.trim().parse::<usize>().ok()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn get_available_memory() -> Option<usize> {
        None
    }

    if let Some(available) = get_available_memory() {
        let calculated = (available as f64 * HEAP_PERCENTAGE) as usize;
        calculated.clamp(MIN_HEAP, MAX_HEAP)
    } else {
        MIN_HEAP
    }
}

impl FulltextConfig {
    /// 힙 크기 설정
    pub fn with_heap_size(mut self, heap_size: usize) -> Self {
        self.heap_size = heap_size;
        self
    }

    /// 동적 힙 크기 사용 (시스템 메모리 기반)
    ///
    /// 사용 가능 메모리의 5%를 힙으로 사용합니다.
    /// (최소 50MB, 최대 2GB)
    pub fn with_dynamic_heap(mut self) -> Self {
        self.heap_size = calculate_heap_size();
        self
    }

    /// 자동 커밋 간격 설정 (초)
    pub fn with_auto_commit_interval(mut self, secs: u64) -> Self {
        self.auto_commit_interval_secs = secs;
        self
    }

    /// 자동 커밋 문서 수 설정
    pub fn with_auto_commit_doc_count(mut self, count: usize) -> Self {
        self.auto_commit_doc_count = count;
        self
    }

    /// 자동 커밋 비활성화
    pub fn without_auto_commit(mut self) -> Self {
        self.auto_commit_interval_secs = 0;
        self.auto_commit_doc_count = 0;
        self
    }
}

/// 전문 검색 인덱스
pub struct FulltextIndex {
    index: Index,
    schema: ChunkSchema,
    config: FulltextConfig,
    writer: RwLock<Option<tantivy::IndexWriter>>,
    reader: Arc<TantivyReader>,
    language_router: LanguageRouter,
    /// 커밋되지 않은 문서 수
    pending_docs: AtomicUsize,
    /// 마지막 커밋 시간 (Unix epoch 초)
    last_commit_time: AtomicU64,
    /// 마지막 커밋 시점의 Instant (자동 커밋용)
    #[allow(dead_code)]
    last_commit_instant: RwLock<Instant>,
}

impl FulltextIndex {
    /// 새 redb-backed 인덱스 생성.
    pub fn create_in_redb(
        db: Arc<Database>,
        namespace: impl AsRef<[u8]>,
        config: FulltextConfig,
    ) -> Result<Self> {
        let schema = ChunkSchema::new();
        let directory = crate::storage::redb_directory::RedbDirectory::stage_empty(db, namespace)?;
        let index = Index::create(
            directory.clone(),
            schema.build(),
            tantivy::IndexSettings::default(),
        )?;
        directory.activate_standalone()?;
        Self::from_index(index, schema, config)
    }

    /// Prepare a new redb-backed index without publishing it to active readers.
    pub fn stage_new_in_redb(
        db: Arc<Database>,
        namespace: impl AsRef<[u8]>,
        config: FulltextConfig,
    ) -> Result<(Self, crate::FulltextGeneration)> {
        let schema = ChunkSchema::new();
        let directory = crate::storage::redb_directory::RedbDirectory::stage_empty(db, namespace)?;
        let generation = directory.staged_generation();
        let index = Index::create(directory, schema.build(), tantivy::IndexSettings::default())?;
        Ok((Self::from_index(index, schema, config)?, generation))
    }

    /// Clone the active manifest into a new generation for an integrated update.
    pub fn stage_from_active_in_redb(
        db: Arc<Database>,
        namespace: impl AsRef<[u8]>,
        config: FulltextConfig,
    ) -> Result<(Self, crate::FulltextGeneration)> {
        let directory =
            crate::storage::redb_directory::RedbDirectory::stage_from_active(db, namespace)?;
        let generation = directory.staged_generation();
        let index = Index::open(directory)?;
        let schema = ChunkSchema::from_tantivy_schema(index.schema())?;
        Ok((Self::from_index(index, schema, config)?, generation))
    }

    /// Open the active redb-backed generation for read-only search.
    /// Mutations must use a staged generation and publish through the logical
    /// database transaction coordinator.
    pub fn open_in_redb(
        db: Arc<Database>,
        namespace: impl AsRef<[u8]>,
        config: FulltextConfig,
    ) -> Result<Self> {
        let directory = crate::storage::redb_directory::RedbDirectory::open_active(db, namespace)?;
        let index = Index::open(directory)?;
        let schema = ChunkSchema::from_tantivy_schema(index.schema())?;
        Self::from_index_mode(index, schema, config, false)
    }

    fn from_index(index: Index, schema: ChunkSchema, config: FulltextConfig) -> Result<Self> {
        Self::from_index_mode(index, schema, config, true)
    }

    fn from_index_mode(
        index: Index,
        schema: ChunkSchema,
        config: FulltextConfig,
        writable: bool,
    ) -> Result<Self> {
        Self::register_tokenizers(&index)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let writer = writable
            .then(|| index.writer(config.heap_size))
            .transpose()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        Ok(Self {
            index,
            schema,
            config,
            writer: RwLock::new(writer),
            reader: Arc::new(reader),
            language_router: LanguageRouter::new(),
            pending_docs: AtomicUsize::new(0),
            last_commit_time: AtomicU64::new(now),
            last_commit_instant: RwLock::new(Instant::now()),
        })
    }

    /// 새 인덱스 생성
    pub fn create<P: AsRef<Path>>(path: P, config: FulltextConfig) -> Result<Self> {
        let schema = ChunkSchema::new();
        let tantivy_schema = schema.build();

        // 디렉토리 생성
        std::fs::create_dir_all(path.as_ref())?;

        let index = Index::create_in_dir(path.as_ref(), tantivy_schema)?;

        // 토크나이저 등록
        Self::register_tokenizers(&index)?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let writer = index.writer(config.heap_size)?;

        info!("Created fulltext index at {:?}", path.as_ref());

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(Self {
            index,
            schema,
            config,
            writer: RwLock::new(Some(writer)),
            reader: Arc::new(reader),
            language_router: LanguageRouter::new(),
            pending_docs: AtomicUsize::new(0),
            last_commit_time: AtomicU64::new(now),
            last_commit_instant: RwLock::new(Instant::now()),
        })
    }

    /// 기존 인덱스 열기
    pub fn open<P: AsRef<Path>>(path: P, config: FulltextConfig) -> Result<Self> {
        let index = Index::open_in_dir(path.as_ref())?;
        let schema = ChunkSchema::from_tantivy_schema(index.schema())?;

        // 토크나이저 등록
        Self::register_tokenizers(&index)?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let writer = index.writer(config.heap_size)?;

        info!("Opened fulltext index at {:?}", path.as_ref());

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(Self {
            index,
            schema,
            config,
            writer: RwLock::new(Some(writer)),
            reader: Arc::new(reader),
            language_router: LanguageRouter::new(),
            pending_docs: AtomicUsize::new(0),
            last_commit_time: AtomicU64::new(now),
            last_commit_instant: RwLock::new(Instant::now()),
        })
    }

    /// 메모리 인덱스 생성 (테스트용)
    pub fn create_in_ram(config: FulltextConfig) -> Result<Self> {
        let schema = ChunkSchema::new();
        let tantivy_schema = schema.build();

        let index = Index::create_in_ram(tantivy_schema);

        // 토크나이저 등록
        Self::register_tokenizers(&index)?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        let writer = index.writer(config.heap_size)?;

        debug!("Created in-memory fulltext index");

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Ok(Self {
            index,
            schema,
            config,
            writer: RwLock::new(Some(writer)),
            reader: Arc::new(reader),
            language_router: LanguageRouter::new(),
            pending_docs: AtomicUsize::new(0),
            last_commit_time: AtomicU64::new(now),
            last_commit_instant: RwLock::new(Instant::now()),
        })
    }

    /// 토크나이저 등록
    fn register_tokenizers(index: &Index) -> Result<()> {
        let tokenizer_manager = index.tokenizers();

        // CJK 언어 코드는 형태소 분석 없이 Simple tokenizer로 색인한다.
        for language in ["ko", "ja", "zh"] {
            tokenizer_manager.register(language, tantivy::tokenizer::SimpleTokenizer::default());
        }

        // 기본 영어 토크나이저는 Tantivy에 내장됨

        Ok(())
    }

    /// IndexWriter 획득
    pub fn writer(&self) -> IndexWriter<'_> {
        IndexWriter::new(self)
    }

    /// IndexReader 획득
    pub fn reader(&self) -> IndexReader<'_> {
        IndexReader::new(self)
    }

    /// 스키마 반환
    pub fn schema(&self) -> &ChunkSchema {
        &self.schema
    }

    /// 설정 반환
    pub fn config(&self) -> &FulltextConfig {
        &self.config
    }

    /// 내부 Tantivy 인덱스 반환
    pub(crate) fn tantivy_index(&self) -> &Index {
        &self.index
    }

    /// 내부 Reader 반환
    pub(crate) fn tantivy_reader(&self) -> &TantivyReader {
        &self.reader
    }

    /// 내부 Writer 잠금 반환
    pub(crate) fn tantivy_writer(&self) -> &RwLock<Option<tantivy::IndexWriter>> {
        &self.writer
    }

    /// 언어 라우터 반환
    pub fn language_router(&self) -> &LanguageRouter {
        &self.language_router
    }

    /// 대기 중인 문서 수 증가
    pub(crate) fn increment_pending_docs(&self) {
        self.pending_docs.fetch_add(1, Ordering::Relaxed);
    }

    /// 커밋 추적 초기화
    fn reset_commit_tracking(&self) {
        self.pending_docs.store(0, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.last_commit_time.store(now, Ordering::Relaxed);
        *self.last_commit_instant.write() = Instant::now();
    }

    /// 자동 커밋 확인 및 실행
    ///
    /// 문서 수 또는 시간 기반으로 자동 커밋 조건을 확인합니다.
    /// 조건이 충족되면 커밋을 실행합니다.
    ///
    /// # Returns
    ///
    /// 커밋이 실행되었으면 `Ok(true)`, 아니면 `Ok(false)`
    pub fn maybe_auto_commit(&self) -> Result<bool> {
        let pending = self.pending_docs.load(Ordering::Relaxed);
        let last_commit = *self.last_commit_instant.read();
        let elapsed_secs = last_commit.elapsed().as_secs();

        // 문서 수 기반 자동 커밋
        let doc_count_trigger =
            self.config.auto_commit_doc_count > 0 && pending >= self.config.auto_commit_doc_count;

        // 시간 기반 자동 커밋
        let time_trigger = self.config.auto_commit_interval_secs > 0
            && elapsed_secs >= self.config.auto_commit_interval_secs
            && pending > 0;

        if doc_count_trigger || time_trigger {
            debug!(
                "Auto-commit triggered: pending={}, elapsed={}s (doc_trigger={}, time_trigger={})",
                pending, elapsed_secs, doc_count_trigger, time_trigger
            );
            self.commit()?;
            return Ok(true);
        }

        Ok(false)
    }

    /// 대기 중인 문서 수 반환
    pub fn pending_doc_count(&self) -> usize {
        self.pending_docs.load(Ordering::Relaxed)
    }

    /// 마지막 커밋 이후 경과 시간 (초)
    pub fn seconds_since_last_commit(&self) -> u64 {
        self.last_commit_instant.read().elapsed().as_secs()
    }

    /// 인덱스 커밋
    pub fn commit(&self) -> Result<()> {
        let mut writer_guard = self.writer.write();
        if let Some(ref mut writer) = *writer_guard {
            writer.commit()?;
            self.reader.reload()?;
            self.reset_commit_tracking();
            debug!("Index committed");
        }
        Ok(())
    }

    /// 인덱스 롤백
    pub fn rollback(&self) -> Result<()> {
        let mut writer_guard = self.writer.write();
        if let Some(ref mut writer) = *writer_guard {
            writer.rollback()?;
            self.reset_commit_tracking();
            debug!("Index rolled back");
        }
        Ok(())
    }

    /// 인덱스 상태 검증
    ///
    /// Tantivy 인덱스의 무결성을 검사하고 상태를 반환합니다.
    ///
    /// # Returns
    ///
    /// `IndexHealth::Healthy` - 정상
    /// `IndexHealth::Corrupted` - 손상됨 (세부 정보 포함)
    /// `IndexHealth::NeedsRebuild` - 재구축 필요
    pub fn verify(&self) -> IndexHealth {
        // 1. 스키마 필드 검증
        let schema = self.index.schema();
        let required_fields = ["chunk_id", "content"];
        for field_name in required_fields {
            if schema.get_field(field_name).is_err() {
                return IndexHealth::Corrupted {
                    details: format!("Required field '{}' not found in schema", field_name),
                    recoverable: false,
                };
            }
        }

        // 2. 세그먼트 검증
        let segment_metas = self.index.searchable_segment_metas();
        match segment_metas {
            Ok(metas) => {
                if metas.is_empty() {
                    // 빈 인덱스는 정상
                    return IndexHealth::Healthy;
                }

                // 각 세그먼트 검증
                for meta in &metas {
                    // 세그먼트 ID와 문서 수 확인
                    if meta.num_deleted_docs() > meta.num_docs() {
                        return IndexHealth::Corrupted {
                            details: format!(
                                "Segment {} has more deleted docs ({}) than total docs ({})",
                                meta.id().uuid_string(),
                                meta.num_deleted_docs(),
                                meta.num_docs()
                            ),
                            recoverable: true,
                        };
                    }
                }
            }
            Err(e) => {
                return IndexHealth::Corrupted {
                    details: format!("Failed to read segment metadata: {}", e),
                    recoverable: false,
                };
            }
        }

        // 3. Reader 검증 (searcher 획득 시도)
        match self.reader.searcher().segment_readers().len() {
            0 => {
                // 빈 인덱스
                IndexHealth::Healthy
            }
            _ => {
                // searcher가 정상 작동
                IndexHealth::Healthy
            }
        }
    }

    /// 인덱스 복구 시도
    ///
    /// 손상된 인덱스의 복구를 시도합니다.
    /// 복구 불가능한 경우 에러를 반환합니다.
    ///
    /// # 복구 전략
    ///
    /// 1. Writer 재생성
    /// 2. 커밋 롤백
    /// 3. Reader 리로드
    pub fn repair(&self) -> Result<()> {
        let health = self.verify();

        match health {
            IndexHealth::Healthy => {
                debug!("Index is healthy, no repair needed");
                Ok(())
            }
            IndexHealth::Corrupted {
                details,
                recoverable,
            } => {
                if !recoverable {
                    return Err(FulltextError::IndexCorrupted(format!(
                        "Index is corrupted and cannot be recovered: {}",
                        details
                    )));
                }

                info!("Attempting to repair index: {}", details);

                // 롤백 시도
                self.rollback()?;

                // Reader 리로드
                self.reader.reload()?;

                // 재검증
                let new_health = self.verify();
                if new_health != IndexHealth::Healthy {
                    return Err(FulltextError::RecoveryFailed(
                        "Repair attempt failed, index still corrupted".to_string(),
                    ));
                }

                info!("Index repair successful");
                Ok(())
            }
            IndexHealth::NeedsRebuild { reason } => Err(FulltextError::IndexCorrupted(format!(
                "Index needs rebuild: {}. Use rebuild() method.",
                reason
            ))),
        }
    }

    /// 인덱스 상태가 정상인지 확인
    pub fn is_healthy(&self) -> bool {
        matches!(self.verify(), IndexHealth::Healthy)
    }

    /// 재시도를 포함한 커밋
    ///
    /// Exponential backoff를 사용하여 커밋을 재시도합니다.
    pub fn commit_with_retry(&self, retry_config: &RetryConfig) -> Result<()> {
        let mut last_error = None;

        for attempt in 0..=retry_config.max_retries {
            match self.commit() {
                Ok(()) => {
                    if attempt > 0 {
                        debug!("Commit succeeded after {} retries", attempt);
                    }
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < retry_config.max_retries {
                        let delay = retry_config.delay_for_attempt(attempt);
                        tracing::warn!(
                            "Commit failed (attempt {}/{}), retrying in {:?}: {:?}",
                            attempt + 1,
                            retry_config.max_retries + 1,
                            delay,
                            last_error
                        );
                        std::thread::sleep(delay);
                    }
                }
            }
        }

        // 모든 재시도 실패
        Err(last_error.unwrap_or_else(|| {
            crate::error::FulltextError::Other("Commit failed after all retries".to_string())
        }))
    }
}

impl Drop for FulltextIndex {
    fn drop(&mut self) {
        if self.writer.read().is_none() {
            return;
        }
        // 재시도를 포함한 커밋 시도
        let retry_config = RetryConfig::default();
        if let Err(e) = self.commit_with_retry(&retry_config) {
            tracing::error!(
                "Failed to commit index on drop after {} retries: {}",
                retry_config.max_retries,
                e
            );
        }
        // Merge threads hold the directory; join them so the storage is released on drop.
        if let Some(writer) = self.writer.write().take()
            && let Err(e) = writer.wait_merging_threads()
        {
            tracing::error!("Failed to finish index merges on drop: {}", e);
        }
    }
}
