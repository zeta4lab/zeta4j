//! Tantivy 스키마 정의
//!
//! 청크 문서를 인덱싱하기 위한 스키마를 정의합니다.

use tantivy::schema::{
    FAST, Field, INDEXED, IndexRecordOption, STORED, STRING, Schema, TEXT, TextFieldIndexing,
    TextOptions,
};

use crate::error::{FulltextError, Result};

/// 청크 스키마 필드
#[derive(Debug, Clone)]
pub struct ChunkSchema {
    /// 청크 ID (u64)
    pub chunk_id: Option<Field>,
    /// 본문 내용 (기본 토크나이저)
    pub content: Option<Field>,
    /// 한국어 본문 (한국어 토크나이저)
    pub content_ko: Option<Field>,
    /// 영어 본문 (영어 스테머)
    pub content_en: Option<Field>,
    /// 일본어 본문 (일본어 토크나이저)
    pub content_ja: Option<Field>,
    /// 중국어 본문 (중국어 토크나이저)
    pub content_zh: Option<Field>,
    /// 문서 ID
    pub document_id: Option<Field>,
    /// 파일 경로
    pub file_path: Option<Field>,
    /// 청크 위치 (문서 내 순서)
    pub position: Option<Field>,
    /// 시작 오프셋
    pub start_offset: Option<Field>,
    /// 종료 오프셋
    pub end_offset: Option<Field>,
    /// 노드 라벨 (필터링용)
    pub labels: Option<Field>,
    /// 테넌트 ID (멀티테넌트)
    pub tenant_id: Option<Field>,
    /// 인덱싱 시간
    pub indexed_at: Option<Field>,
    /// 컨텐츠 해시 (변경 감지용)
    pub content_hash: Option<Field>,
    /// 감지된 언어
    pub detected_lang: Option<Field>,
    /// Serialized canonical entity reference.
    pub entity_ref: Option<Field>,

    /// 내부 Tantivy 스키마
    #[allow(dead_code)]
    tantivy_schema: Option<Schema>,
}

impl ChunkSchema {
    /// 새 스키마 생성 (필드 포함)
    pub fn new() -> Self {
        let mut builder = Schema::builder();

        // 필수 필드
        let chunk_id = builder.add_u64_field("chunk_id", INDEXED | STORED | FAST);
        let content = builder.add_text_field("content", TEXT | STORED);

        // 언어별 content 필드
        let ko_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("ko")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let content_ko = builder.add_text_field("content_ko", ko_options);

        let en_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("en_stem")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let content_en = builder.add_text_field("content_en", en_options);

        // 일본어 content 필드
        let ja_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("ja")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let content_ja = builder.add_text_field("content_ja", ja_options);

        // 중국어 content 필드
        let zh_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("zh")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        let content_zh = builder.add_text_field("content_zh", zh_options);

        // 메타데이터 필드
        let document_id = builder.add_u64_field("document_id", INDEXED | STORED);
        let file_path = builder.add_text_field("file_path", STRING | STORED);
        let position = builder.add_u64_field("position", STORED);
        let start_offset = builder.add_u64_field("start_offset", STORED);
        let end_offset = builder.add_u64_field("end_offset", STORED);

        // 필터링 필드
        let labels = builder.add_text_field("labels", STRING);
        let tenant_id = builder.add_text_field("tenant_id", STRING);
        let indexed_at = builder.add_u64_field("indexed_at", FAST);
        let content_hash = builder.add_bytes_field("content_hash", STORED);
        let detected_lang = builder.add_text_field("detected_lang", STRING | STORED);
        let entity_ref = builder.add_text_field("entity_ref", STORED);

        let tantivy_schema = builder.build();

        Self {
            chunk_id: Some(chunk_id),
            content: Some(content),
            content_ko: Some(content_ko),
            content_en: Some(content_en),
            content_ja: Some(content_ja),
            content_zh: Some(content_zh),
            document_id: Some(document_id),
            file_path: Some(file_path),
            position: Some(position),
            start_offset: Some(start_offset),
            end_offset: Some(end_offset),
            labels: Some(labels),
            tenant_id: Some(tenant_id),
            indexed_at: Some(indexed_at),
            content_hash: Some(content_hash),
            detected_lang: Some(detected_lang),
            entity_ref: Some(entity_ref),
            tantivy_schema: Some(tantivy_schema),
        }
    }

    /// Tantivy 스키마 빌드
    pub fn build(&self) -> Schema {
        // 이미 tantivy_schema가 있으면 클론 반환
        if let Some(ref schema) = self.tantivy_schema {
            return schema.clone();
        }

        // 없으면 새로 빌드
        let mut builder = Schema::builder();

        // 필수 필드
        builder.add_u64_field("chunk_id", INDEXED | STORED | FAST);
        builder.add_text_field("content", TEXT | STORED);

        // 언어별 content 필드
        let ko_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("ko")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        builder.add_text_field("content_ko", ko_options);

        let en_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("en_stem")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        builder.add_text_field("content_en", en_options);

        // 일본어 content 필드
        let ja_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("ja")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        builder.add_text_field("content_ja", ja_options);

        // 중국어 content 필드
        let zh_options = TextOptions::default()
            .set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("zh")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            )
            .set_stored();
        builder.add_text_field("content_zh", zh_options);

        // 메타데이터 필드
        builder.add_u64_field("document_id", INDEXED | STORED);
        builder.add_text_field("file_path", STRING | STORED);
        builder.add_u64_field("position", STORED);
        builder.add_u64_field("start_offset", STORED);
        builder.add_u64_field("end_offset", STORED);

        // 필터링 필드
        builder.add_text_field("labels", STRING);
        builder.add_text_field("tenant_id", STRING);
        builder.add_u64_field("indexed_at", FAST);
        builder.add_bytes_field("content_hash", STORED);
        builder.add_text_field("detected_lang", STRING | STORED);
        builder.add_text_field("entity_ref", STORED);

        builder.build()
    }

    /// 기존 Tantivy 스키마에서 ChunkSchema 생성
    pub fn from_tantivy_schema(schema: Schema) -> Result<Self> {
        let get_field = |name: &str| -> Result<Field> {
            schema
                .get_field(name)
                .map_err(|_| FulltextError::FieldNotFound(name.to_string()))
        };

        Ok(Self {
            chunk_id: get_field("chunk_id").ok(),
            content: get_field("content").ok(),
            content_ko: get_field("content_ko").ok(),
            content_en: get_field("content_en").ok(),
            content_ja: get_field("content_ja").ok(),
            content_zh: get_field("content_zh").ok(),
            document_id: get_field("document_id").ok(),
            file_path: get_field("file_path").ok(),
            position: get_field("position").ok(),
            start_offset: get_field("start_offset").ok(),
            end_offset: get_field("end_offset").ok(),
            labels: get_field("labels").ok(),
            tenant_id: get_field("tenant_id").ok(),
            indexed_at: get_field("indexed_at").ok(),
            content_hash: get_field("content_hash").ok(),
            detected_lang: get_field("detected_lang").ok(),
            entity_ref: get_field("entity_ref").ok(),
            tantivy_schema: Some(schema),
        })
    }

    /// 필드 이름으로 필드 가져오기
    pub fn get_field(&self, name: &str) -> Option<Field> {
        match name {
            "chunk_id" => self.chunk_id,
            "content" => self.content,
            "content_ko" => self.content_ko,
            "content_en" => self.content_en,
            "content_ja" => self.content_ja,
            "content_zh" => self.content_zh,
            "document_id" => self.document_id,
            "file_path" => self.file_path,
            "position" => self.position,
            "start_offset" => self.start_offset,
            "end_offset" => self.end_offset,
            "labels" => self.labels,
            "tenant_id" => self.tenant_id,
            "indexed_at" => self.indexed_at,
            "content_hash" => self.content_hash,
            "detected_lang" => self.detected_lang,
            "entity_ref" => self.entity_ref,
            _ => None,
        }
    }
}

impl Default for ChunkSchema {
    fn default() -> Self {
        Self::new()
    }
}
