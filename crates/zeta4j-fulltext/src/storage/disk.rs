// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 디스크 저장소
//!
//! Tantivy 인덱스의 디스크 영속화를 관리합니다.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use zeta4j_common::DEFAULT_DATABASE_NAME;

use crate::error::{FulltextError, Result};

/// 디스크 저장소 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskStorageConfig {
    /// 기본 디렉토리
    pub base_dir: PathBuf,
    /// 인덱스 디렉토리 이름
    pub index_dir: String,
    /// 메타데이터 파일 이름
    pub metadata_file: String,
    /// 자동 생성 여부
    pub auto_create: bool,
}

impl Default for DiskStorageConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from("~/.zeta4j/databases/fulltext"),
            index_dir: DEFAULT_DATABASE_NAME.to_string(),
            metadata_file: "metadata.json".to_string(),
            auto_create: true,
        }
    }
}

impl DiskStorageConfig {
    /// 새 설정 생성
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            ..Default::default()
        }
    }

    /// 인덱스 디렉토리 이름 설정
    pub fn with_index_dir(mut self, name: impl Into<String>) -> Self {
        self.index_dir = name.into();
        self
    }

    /// 전체 인덱스 경로 반환
    pub fn index_path(&self) -> PathBuf {
        self.base_dir.join(&self.index_dir)
    }

    /// 메타데이터 파일 경로 반환
    pub fn metadata_path(&self) -> PathBuf {
        self.index_path().join(&self.metadata_file)
    }
}

/// 인덱스 메타데이터
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    /// 인덱스 이름
    pub name: String,
    /// 버전
    pub version: String,
    /// 생성 시간
    pub created_at: u64,
    /// 마지막 업데이트 시간
    pub updated_at: u64,
    /// 문서 수 (마지막 커밋 시점)
    pub doc_count: u64,
    /// 설정
    pub config: IndexConfig,
    /// chunk_id 매핑 (v1.16+에서 추가)
    #[serde(default)]
    pub chunk_mappings: Option<ChunkMappings>,
}

/// 인덱스 설정 (메타데이터에 저장)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// 기본 언어
    pub default_language: String,
    /// 언어 자동 감지
    pub auto_detect_language: bool,
    /// BM25 k1
    pub bm25_k1: f64,
    /// BM25 b
    pub bm25_b: f64,
}

/// chunk_id → fulltext_id 매핑 (영속화용)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChunkMappings {
    /// chunk_id → fulltext_id 매핑
    pub chunk_to_fulltext: std::collections::HashMap<u64, u64>,
    /// 다음 fulltext_id
    pub next_fulltext_id: u64,
}

impl ChunkMappings {
    /// 새 ChunkMappings 생성
    pub fn new() -> Self {
        Self {
            chunk_to_fulltext: std::collections::HashMap::new(),
            next_fulltext_id: 1,
        }
    }

    /// chunk_id에 대한 fulltext_id 조회 또는 할당
    pub fn get_or_insert(&mut self, chunk_id: u64) -> u64 {
        if let Some(&fulltext_id) = self.chunk_to_fulltext.get(&chunk_id) {
            fulltext_id
        } else {
            let fulltext_id = self.next_fulltext_id;
            self.chunk_to_fulltext.insert(chunk_id, fulltext_id);
            self.next_fulltext_id += 1;
            fulltext_id
        }
    }

    /// chunk_id에 대한 fulltext_id 조회
    pub fn get(&self, chunk_id: u64) -> Option<u64> {
        self.chunk_to_fulltext.get(&chunk_id).copied()
    }

    /// 매핑 추가
    pub fn insert(&mut self, chunk_id: u64, fulltext_id: u64) {
        self.chunk_to_fulltext.insert(chunk_id, fulltext_id);
        if fulltext_id >= self.next_fulltext_id {
            self.next_fulltext_id = fulltext_id + 1;
        }
    }

    /// 매핑 삭제
    pub fn remove(&mut self, chunk_id: u64) -> Option<u64> {
        self.chunk_to_fulltext.remove(&chunk_id)
    }

    /// 매핑 개수
    pub fn len(&self) -> usize {
        self.chunk_to_fulltext.len()
    }

    /// 매핑이 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.chunk_to_fulltext.is_empty()
    }
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            default_language: "en".to_string(),
            auto_detect_language: true,
            bm25_k1: 1.2,
            bm25_b: 0.75,
        }
    }
}

/// 디스크 저장소
pub struct DiskStorage {
    config: DiskStorageConfig,
    metadata: Option<IndexMetadata>,
}

impl DiskStorage {
    /// 새 디스크 저장소 생성
    pub fn new(config: DiskStorageConfig) -> Result<Self> {
        let mut storage = Self {
            config,
            metadata: None,
        };

        storage.ensure_directory()?;
        storage.load_metadata()?;

        Ok(storage)
    }

    /// 기본 디렉토리로 생성
    pub fn with_base_dir(base_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::new(DiskStorageConfig::new(base_dir))
    }

    /// 디렉토리 존재 확인 및 생성
    fn ensure_directory(&self) -> Result<()> {
        let path = self.config.index_path();

        if !path.exists() {
            if self.config.auto_create {
                fs::create_dir_all(&path)?;
                info!("Created index directory: {:?}", path);
            } else {
                return Err(FulltextError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Index directory does not exist: {:?}", path),
                )));
            }
        }

        Ok(())
    }

    /// 메타데이터 로드
    fn load_metadata(&mut self) -> Result<()> {
        let metadata_path = self.config.metadata_path();

        if metadata_path.exists() {
            let content = fs::read_to_string(&metadata_path)?;
            self.metadata = Some(serde_json::from_str(&content)?);
            debug!("Loaded metadata from {:?}", metadata_path);
        }

        Ok(())
    }

    /// 메타데이터 저장
    pub fn save_metadata(&self, metadata: &IndexMetadata) -> Result<()> {
        let metadata_path = self.config.metadata_path();

        // 임시 파일에 먼저 쓰기 (원자적 쓰기)
        let temp_path = metadata_path.with_extension("json.tmp");
        let content = serde_json::to_string_pretty(metadata)?;
        fs::write(&temp_path, content)?;

        // 원자적 이름 변경
        fs::rename(&temp_path, &metadata_path)?;

        debug!("Saved metadata to {:?}", metadata_path);
        Ok(())
    }

    /// 메타데이터 반환
    pub fn metadata(&self) -> Option<&IndexMetadata> {
        self.metadata.as_ref()
    }

    /// 인덱스 경로 반환
    pub fn index_path(&self) -> PathBuf {
        self.config.index_path()
    }

    /// 설정 반환
    pub fn config(&self) -> &DiskStorageConfig {
        &self.config
    }

    /// 인덱스 존재 확인
    pub fn index_exists(&self) -> bool {
        let path = self.config.index_path();
        path.exists() && path.is_dir() && self.config.metadata_path().exists()
    }

    /// 인덱스 삭제
    pub fn delete_index(&self) -> Result<()> {
        let path = self.config.index_path();

        if path.exists() {
            fs::remove_dir_all(&path)?;
            info!("Deleted index at {:?}", path);
        } else {
            warn!("Index directory does not exist: {:?}", path);
        }

        Ok(())
    }

    /// 인덱스 크기 계산 (바이트)
    pub fn index_size(&self) -> Result<u64> {
        let path = self.config.index_path();
        calculate_dir_size(&path)
    }

    /// chunk 매핑 파일 경로 반환
    fn chunk_mappings_path(&self) -> PathBuf {
        self.config.index_path().join("chunk_mappings.json")
    }

    /// chunk 매핑 저장
    pub fn save_chunk_mappings(&self, mappings: &ChunkMappings) -> Result<()> {
        let path = self.chunk_mappings_path();
        let temp_path = path.with_extension("json.tmp");
        let content = serde_json::to_string_pretty(mappings)?;
        fs::write(&temp_path, content)?;
        fs::rename(&temp_path, &path)?;
        debug!("Saved chunk mappings to {:?}", path);
        Ok(())
    }

    /// chunk 매핑 로드
    pub fn load_chunk_mappings(&self) -> Result<ChunkMappings> {
        let path = self.chunk_mappings_path();
        if path.exists() {
            let content = fs::read_to_string(&path)?;
            let mappings: ChunkMappings = serde_json::from_str(&content)?;
            debug!(
                "Loaded chunk mappings from {:?} ({} entries)",
                path,
                mappings.len()
            );
            Ok(mappings)
        } else {
            Ok(ChunkMappings::new())
        }
    }

    /// chunk 매핑 삭제
    pub fn delete_chunk_mappings(&self) -> Result<()> {
        let path = self.chunk_mappings_path();
        if path.exists() {
            fs::remove_file(&path)?;
            debug!("Deleted chunk mappings at {:?}", path);
        }
        Ok(())
    }

    /// chunk_id에 대한 fulltext_id 조회
    pub fn get_fulltext_id(&self, chunk_id: u64) -> Result<Option<u64>> {
        let mappings = self.load_chunk_mappings()?;
        Ok(mappings.get(chunk_id))
    }

    /// 단일 chunk 매핑 저장
    pub fn save_chunk_mapping(&self, chunk_id: u64, fulltext_id: u64) -> Result<()> {
        let mut mappings = self.load_chunk_mappings()?;
        mappings.insert(chunk_id, fulltext_id);
        self.save_chunk_mappings(&mappings)
    }

    /// 배치 chunk 매핑 저장
    pub fn save_chunk_mappings_batch(&self, batch: &[(u64, u64)]) -> Result<()> {
        let mut mappings = self.load_chunk_mappings()?;
        for &(chunk_id, fulltext_id) in batch {
            mappings.insert(chunk_id, fulltext_id);
        }
        self.save_chunk_mappings(&mappings)
    }
}

/// 디렉토리 크기 계산
fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut total_size = 0u64;

    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let entry_path = entry.path();

            if entry_path.is_file() {
                total_size += entry.metadata()?.len();
            } else if entry_path.is_dir() {
                total_size += calculate_dir_size(&entry_path)?;
            }
        }
    }

    Ok(total_size)
}

/// 현재 타임스탬프 반환 (초)
pub fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 새 인덱스 메타데이터 생성
pub fn create_index_metadata(name: &str) -> IndexMetadata {
    let now = current_timestamp();

    IndexMetadata {
        name: name.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: now,
        updated_at: now,
        doc_count: 0,
        config: IndexConfig::default(),
        chunk_mappings: None,
    }
}
