//! 인덱스 Writer
//!
//! 문서를 인덱스에 추가, 삭제, 업데이트하는 기능을 제공합니다.

use std::time::{SystemTime, UNIX_EPOCH};
use tantivy::doc;
use tracing::debug;
use zeta4j_common::CanonicalEntityRef;

use super::FulltextIndex;
use crate::error::{FulltextError, Result};
use crate::tokenizer::router::DetectedLanguage;

/// 청크 문서
#[derive(Debug, Clone)]
pub struct ChunkDocument {
    /// 청크 ID
    pub chunk_id: u64,
    /// 본문 내용
    pub content: String,
    /// 문서 ID
    pub document_id: Option<u64>,
    /// 파일 경로
    pub file_path: Option<String>,
    /// 위치
    pub position: Option<u64>,
    /// 시작 오프셋
    pub start_offset: Option<u64>,
    /// 종료 오프셋
    pub end_offset: Option<u64>,
    /// 라벨
    pub labels: Option<Vec<String>>,
    /// 테넌트 ID
    pub tenant_id: Option<String>,
    /// 컨텐츠 해시
    pub content_hash: Option<Vec<u8>>,
    /// Canonical entity indexed by this derived document.
    pub entity: CanonicalEntityRef,
}

impl ChunkDocument {
    /// 새 청크 문서 생성
    pub fn new(chunk_id: u64, content: impl Into<String>, entity: CanonicalEntityRef) -> Self {
        Self {
            chunk_id,
            content: content.into(),
            document_id: None,
            file_path: None,
            position: None,
            start_offset: None,
            end_offset: None,
            labels: None,
            tenant_id: None,
            content_hash: None,
            entity,
        }
    }

    /// 문서 ID 설정
    pub fn with_document_id(mut self, document_id: u64) -> Self {
        self.document_id = Some(document_id);
        self
    }

    /// 파일 경로 설정
    pub fn with_file_path(mut self, file_path: impl Into<String>) -> Self {
        self.file_path = Some(file_path.into());
        self
    }

    /// 위치 정보 설정
    pub fn with_position(mut self, position: u64, start: u64, end: u64) -> Self {
        self.position = Some(position);
        self.start_offset = Some(start);
        self.end_offset = Some(end);
        self
    }

    /// 라벨 설정
    pub fn with_labels(mut self, labels: Vec<String>) -> Self {
        self.labels = Some(labels);
        self
    }

    /// 테넌트 ID 설정
    pub fn with_tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    /// 컨텐츠 해시 설정
    pub fn with_content_hash(mut self, hash: Vec<u8>) -> Self {
        self.content_hash = Some(hash);
        self
    }
}

/// 인덱스 Writer
pub struct IndexWriter<'a> {
    index: &'a FulltextIndex,
}

impl<'a> IndexWriter<'a> {
    /// 새 Writer 생성
    pub(crate) fn new(index: &'a FulltextIndex) -> Self {
        Self { index }
    }

    /// 청크 문서 추가
    pub fn add_document(&self, doc: &ChunkDocument) -> Result<()> {
        doc.entity
            .validate()
            .map_err(|message| FulltextError::InvalidConfig(message.to_string()))?;
        let schema = self.index.tantivy_index().schema();

        // 필드 가져오기
        let chunk_id_field = schema
            .get_field("chunk_id")
            .map_err(|_| FulltextError::FieldNotFound("chunk_id".to_string()))?;
        let content_field = schema
            .get_field("content")
            .map_err(|_| FulltextError::FieldNotFound("content".to_string()))?;
        let content_ko_field = schema
            .get_field("content_ko")
            .map_err(|_| FulltextError::FieldNotFound("content_ko".to_string()))?;
        let content_en_field = schema
            .get_field("content_en")
            .map_err(|_| FulltextError::FieldNotFound("content_en".to_string()))?;
        let detected_lang_field = schema
            .get_field("detected_lang")
            .map_err(|_| FulltextError::FieldNotFound("detected_lang".to_string()))?;
        let indexed_at_field = schema
            .get_field("indexed_at")
            .map_err(|_| FulltextError::FieldNotFound("indexed_at".to_string()))?;

        // 언어 감지
        let lang = self.index.language_router().detect_language(&doc.content);

        // 기본 문서 생성
        let mut tantivy_doc = doc!(
            chunk_id_field => doc.chunk_id,
            content_field => doc.content.clone(),
            detected_lang_field => format!("{:?}", lang),
            indexed_at_field => current_timestamp()
        );

        // 언어별 필드 가져오기 (일본어/중국어)
        let content_ja_field = schema
            .get_field("content_ja")
            .map_err(|_| FulltextError::FieldNotFound("content_ja".to_string()))?;
        let content_zh_field = schema
            .get_field("content_zh")
            .map_err(|_| FulltextError::FieldNotFound("content_zh".to_string()))?;

        // 언어별 필드에 추가
        match lang {
            DetectedLanguage::Korean => {
                tantivy_doc.add_text(content_ko_field, &doc.content);
            }
            DetectedLanguage::English => {
                tantivy_doc.add_text(content_en_field, &doc.content);
            }
            DetectedLanguage::Japanese => {
                tantivy_doc.add_text(content_ja_field, &doc.content);
            }
            DetectedLanguage::Chinese => {
                tantivy_doc.add_text(content_zh_field, &doc.content);
            }
            DetectedLanguage::Unknown => {
                // Unknown은 기본 content 필드만 사용
            }
        }

        // 옵션 필드 추가
        if let Some(document_id) = doc.document_id {
            let field = schema
                .get_field("document_id")
                .map_err(|_| FulltextError::FieldNotFound("document_id".to_string()))?;
            tantivy_doc.add_u64(field, document_id);
        }

        if let Some(ref file_path) = doc.file_path {
            let field = schema
                .get_field("file_path")
                .map_err(|_| FulltextError::FieldNotFound("file_path".to_string()))?;
            tantivy_doc.add_text(field, file_path);
        }

        if let Some(position) = doc.position {
            let field = schema
                .get_field("position")
                .map_err(|_| FulltextError::FieldNotFound("position".to_string()))?;
            tantivy_doc.add_u64(field, position);
        }

        if let Some(start) = doc.start_offset {
            let field = schema
                .get_field("start_offset")
                .map_err(|_| FulltextError::FieldNotFound("start_offset".to_string()))?;
            tantivy_doc.add_u64(field, start);
        }

        if let Some(end) = doc.end_offset {
            let field = schema
                .get_field("end_offset")
                .map_err(|_| FulltextError::FieldNotFound("end_offset".to_string()))?;
            tantivy_doc.add_u64(field, end);
        }

        if let Some(ref labels) = doc.labels {
            let field = schema
                .get_field("labels")
                .map_err(|_| FulltextError::FieldNotFound("labels".to_string()))?;
            for label in labels {
                tantivy_doc.add_text(field, label);
            }
        }

        if let Some(ref tenant_id) = doc.tenant_id {
            let field = schema
                .get_field("tenant_id")
                .map_err(|_| FulltextError::FieldNotFound("tenant_id".to_string()))?;
            tantivy_doc.add_text(field, tenant_id);
        }

        if let Some(ref hash) = doc.content_hash {
            let field = schema
                .get_field("content_hash")
                .map_err(|_| FulltextError::FieldNotFound("content_hash".to_string()))?;
            tantivy_doc.add_bytes(field, hash);
        }

        let entity_ref_field = schema
            .get_field("entity_ref")
            .map_err(|_| FulltextError::FieldNotFound("entity_ref".to_string()))?;
        tantivy_doc.add_text(entity_ref_field, serde_json::to_string(&doc.entity)?);

        // Writer에 문서 추가
        let mut writer_guard = self.index.tantivy_writer().write();
        let writer = writer_guard.as_mut().ok_or_else(|| {
            FulltextError::Other("active fulltext generation is read-only".to_string())
        })?;
        writer.add_document(tantivy_doc)?;
        debug!("Added document with chunk_id: {}", doc.chunk_id);
        drop(writer_guard);

        // 대기 문서 수 증가 및 자동 커밋 확인
        self.index.increment_pending_docs();
        let _ = self.index.maybe_auto_commit();

        Ok(())
    }

    /// Add a document whose identity is explicitly supplied by the caller.
    pub fn add_simple(
        &self,
        chunk_id: u64,
        content: &str,
        entity: CanonicalEntityRef,
    ) -> Result<()> {
        self.add_document(&ChunkDocument::new(chunk_id, content, entity))
    }

    /// 청크 ID로 문서 삭제
    pub fn delete_by_chunk_id(&self, chunk_id: u64) -> Result<()> {
        let schema = self.index.tantivy_index().schema();
        let chunk_id_field = schema
            .get_field("chunk_id")
            .map_err(|_| FulltextError::FieldNotFound("chunk_id".to_string()))?;

        let term = tantivy::Term::from_field_u64(chunk_id_field, chunk_id);

        let mut writer_guard = self.index.tantivy_writer().write();
        let writer = writer_guard.as_mut().ok_or_else(|| {
            FulltextError::Other("active fulltext generation is read-only".to_string())
        })?;
        writer.delete_term(term);
        debug!("Deleted document with chunk_id: {}", chunk_id);

        Ok(())
    }

    /// 문서 ID로 관련 청크 모두 삭제
    pub fn delete_by_document_id(&self, document_id: u64) -> Result<()> {
        let schema = self.index.tantivy_index().schema();
        let document_id_field = schema
            .get_field("document_id")
            .map_err(|_| FulltextError::FieldNotFound("document_id".to_string()))?;

        let term = tantivy::Term::from_field_u64(document_id_field, document_id);

        let mut writer_guard = self.index.tantivy_writer().write();
        let writer = writer_guard.as_mut().ok_or_else(|| {
            FulltextError::Other("active fulltext generation is read-only".to_string())
        })?;
        writer.delete_term(term);
        debug!("Deleted documents with document_id: {}", document_id);

        Ok(())
    }

    /// 모든 문서 삭제
    pub fn delete_all(&self) -> Result<()> {
        let mut writer_guard = self.index.tantivy_writer().write();
        let writer = writer_guard.as_mut().ok_or_else(|| {
            FulltextError::Other("active fulltext generation is read-only".to_string())
        })?;
        writer.delete_all_documents()?;
        debug!("Deleted all documents");
        Ok(())
    }

    /// 커밋
    pub fn commit(&self) -> Result<()> {
        self.index.commit()
    }
}

/// 현재 타임스탬프 반환 (밀리초)
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
