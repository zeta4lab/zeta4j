// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Secure command history for the Cypher shell.
//!
//! AES-256-GCM 암호화 히스토리 저장
//!
//! # 보안 기능
//!
//! - 비밀번호 포함 쿼리 자동 필터링
//! - AES-256-GCM 암호화 저장
//! - 키 파일 별도 관리 (0600 권한)
//!
//! # 파일 구조
//!
//! - `~/.zeta4j/history.enc`: 암호화된 히스토리
//! - `~/.zeta4j/history.key`: 암호화 키 (첫 실행 시 자동 생성)

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;

/// 히스토리 최대 크기
const MAX_HISTORY_SIZE: usize = 1000;

/// Nonce 크기 (12 bytes for AES-GCM)
const NONCE_SIZE: usize = 12;

/// 민감한 패턴 (필터링 대상)
const SENSITIVE_PATTERNS: &[&str] = &[
    "password",
    "PASSWORD",
    "secret",
    "SECRET",
    "token",
    "TOKEN",
    "credential",
    "CREDENTIAL",
    "api_key",
    "API_KEY",
    "apikey",
    "APIKEY",
];

/// 암호화 히스토리 관리자
pub struct SecureHistory {
    /// 히스토리 항목
    entries: Vec<String>,
    /// 암호화 키
    encryption_key: Option<[u8; 32]>,
    /// 히스토리 파일 경로
    file_path: PathBuf,
    /// 키 파일 경로
    key_path: PathBuf,
    /// 암호화 활성화 여부
    encryption_enabled: bool,
    /// 민감 정보 필터링 활성화
    filter_sensitive: bool,
    /// 최대 크기
    max_size: usize,
    /// 현재 위치 (탐색용)
    position: usize,
}

impl SecureHistory {
    /// 새 히스토리 생성
    ///
    /// # Arguments
    /// - `history_dir`: 히스토리 디렉토리 (기본: `~/.zeta4j`)
    pub fn new(history_dir: &Path) -> Self {
        let file_path = history_dir.join("history.enc");
        let key_path = history_dir.join("history.key");

        Self {
            entries: Vec::new(),
            encryption_key: None,
            file_path,
            key_path,
            encryption_enabled: true,
            filter_sensitive: true,
            max_size: MAX_HISTORY_SIZE,
            position: 0,
        }
    }

    /// 암호화 비활성화 (테스트/개발용)
    pub fn without_encryption(mut self) -> Self {
        self.encryption_enabled = false;
        self
    }

    /// 민감 정보 필터링 비활성화
    pub fn without_filter(mut self) -> Self {
        self.filter_sensitive = false;
        self
    }

    /// 최대 크기 설정
    pub fn with_max_size(mut self, max_size: usize) -> Self {
        self.max_size = max_size;
        self
    }

    /// 파일에서 히스토리 로드
    pub fn load(&mut self) -> Result<(), HistoryError> {
        // 디렉토리 생성
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| HistoryError::Io(e.to_string()))?;
        }

        // 파일이 없으면 빈 히스토리로 시작
        if !self.file_path.exists() {
            return Ok(());
        }

        if self.encryption_enabled {
            self.load_encrypted()?;
        } else {
            self.load_plain()?;
        }

        self.position = self.entries.len();
        Ok(())
    }

    /// 암호화된 히스토리 로드
    fn load_encrypted(&mut self) -> Result<(), HistoryError> {
        // 키 로드 또는 생성
        self.ensure_key()?;

        let key = self.encryption_key.ok_or(HistoryError::NoEncryptionKey)?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| HistoryError::Encryption(e.to_string()))?;

        // 암호화된 파일 읽기
        let encrypted = fs::read(&self.file_path).map_err(|e| HistoryError::Io(e.to_string()))?;

        if encrypted.len() < NONCE_SIZE {
            // 파일이 너무 작음 - 새로 시작
            self.entries.clear();
            return Ok(());
        }

        // Nonce 추출
        let (nonce_bytes, ciphertext) = encrypted.split_at(NONCE_SIZE);
        let nonce = Nonce::from_slice(nonce_bytes);

        // 복호화
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| HistoryError::DecryptionFailed)?;

        // 파싱
        let content =
            String::from_utf8(plaintext).map_err(|e| HistoryError::Encoding(e.to_string()))?;

        self.entries = content.lines().map(|s| s.to_string()).collect();

        Ok(())
    }

    /// 평문 히스토리 로드
    fn load_plain(&mut self) -> Result<(), HistoryError> {
        let file = File::open(&self.file_path).map_err(|e| HistoryError::Io(e.to_string()))?;
        let reader = BufReader::new(file);

        self.entries = reader.lines().map_while(Result::ok).collect();

        Ok(())
    }

    /// 파일에 히스토리 저장
    pub fn save(&self) -> Result<(), HistoryError> {
        // 디렉토리 생성
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| HistoryError::Io(e.to_string()))?;
        }

        if self.encryption_enabled {
            self.save_encrypted()?;
        } else {
            self.save_plain()?;
        }

        Ok(())
    }

    /// 암호화하여 저장
    fn save_encrypted(&self) -> Result<(), HistoryError> {
        let key = self.encryption_key.ok_or(HistoryError::NoEncryptionKey)?;
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| HistoryError::Encryption(e.to_string()))?;

        // 랜덤 Nonce 생성
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // 내용 직렬화
        let content = self.entries.join("\n");

        // 암호화
        let ciphertext = cipher
            .encrypt(nonce, content.as_bytes())
            .map_err(|e| HistoryError::Encryption(e.to_string()))?;

        // Nonce + Ciphertext 저장
        let mut output = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        fs::write(&self.file_path, output).map_err(|e| HistoryError::Io(e.to_string()))?;

        Ok(())
    }

    /// 평문으로 저장
    fn save_plain(&self) -> Result<(), HistoryError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.file_path)
            .map_err(|e| HistoryError::Io(e.to_string()))?;

        for entry in &self.entries {
            writeln!(file, "{}", entry).map_err(|e| HistoryError::Io(e.to_string()))?;
        }

        Ok(())
    }

    /// 암호화 키 확보 (로드 또는 생성)
    fn ensure_key(&mut self) -> Result<(), HistoryError> {
        if self.encryption_key.is_some() {
            return Ok(());
        }

        if self.key_path.exists() {
            // 키 파일 로드
            let key_bytes =
                fs::read(&self.key_path).map_err(|e| HistoryError::Io(e.to_string()))?;

            if key_bytes.len() != 32 {
                return Err(HistoryError::InvalidKey);
            }

            let mut key = [0u8; 32];
            key.copy_from_slice(&key_bytes);
            self.encryption_key = Some(key);
        } else {
            // 새 키 생성
            let mut key = [0u8; 32];
            rand::rng().fill_bytes(&mut key);

            // 키 파일 저장 (Unix에서 권한 설정)
            self.save_key_file(&key)?;

            self.encryption_key = Some(key);
        }

        Ok(())
    }

    /// 키 파일 저장 (보안 권한 적용)
    fn save_key_file(&self, key: &[u8; 32]) -> Result<(), HistoryError> {
        // 디렉토리 생성
        if let Some(parent) = self.key_path.parent() {
            fs::create_dir_all(parent).map_err(|e| HistoryError::Io(e.to_string()))?;
        }

        fs::write(&self.key_path, key).map_err(|e| HistoryError::Io(e.to_string()))?;

        // Unix: 0600 권한 설정
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(&self.key_path, perms)
                .map_err(|e| HistoryError::Io(e.to_string()))?;
        }

        Ok(())
    }

    /// 항목 추가
    pub fn add(&mut self, entry: &str) {
        let entry = entry.trim();

        // 빈 항목 무시
        if entry.is_empty() {
            return;
        }

        // 민감 정보 필터링
        if self.filter_sensitive && self.contains_sensitive(entry) {
            return;
        }

        // 중복 제거 (마지막 항목과 동일하면 추가 안 함)
        if self.entries.last().map(|s| s.as_str()) == Some(entry) {
            return;
        }

        self.entries.push(entry.to_string());

        // 최대 크기 제한
        while self.entries.len() > self.max_size {
            self.entries.remove(0);
        }

        self.position = self.entries.len();
    }

    /// 민감 정보 포함 여부 확인
    fn contains_sensitive(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        SENSITIVE_PATTERNS
            .iter()
            .any(|p| lower.contains(&p.to_lowercase()))
    }

    /// 항목 수 반환
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 마지막 N개 항목 반환
    pub fn last_n(&self, n: usize) -> Vec<&str> {
        let start = if self.entries.len() > n {
            self.entries.len() - n
        } else {
            0
        };
        self.entries[start..].iter().map(|s| s.as_str()).collect()
    }

    /// 모든 항목 반환
    pub fn all(&self) -> Vec<&str> {
        self.entries.iter().map(|s| s.as_str()).collect()
    }

    /// 이전 항목으로 이동 (Up 키)
    pub fn prev(&mut self) -> Option<&str> {
        if self.position > 0 {
            self.position -= 1;
            Some(&self.entries[self.position])
        } else {
            None
        }
    }

    /// 다음 항목으로 이동 (Down 키)
    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&str> {
        if self.position < self.entries.len() {
            self.position += 1;
            if self.position < self.entries.len() {
                Some(&self.entries[self.position])
            } else {
                None // 현재 입력으로 복귀
            }
        } else {
            None
        }
    }

    /// 위치 초기화 (새 입력 시작)
    pub fn reset_position(&mut self) {
        self.position = self.entries.len();
    }

    /// 검색 (Ctrl+R)
    pub fn search(&self, query: &str) -> Vec<(usize, &str)> {
        let query_lower = query.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.to_lowercase().contains(&query_lower))
            .map(|(i, e)| (i, e.as_str()))
            .collect()
    }

    /// 히스토리 지우기
    pub fn clear(&mut self) {
        self.entries.clear();
        self.position = 0;
    }
}

/// 히스토리 에러
#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    /// I/O 에러
    #[error("I/O error: {0}")]
    Io(String),

    /// 암호화 에러
    #[error("Encryption error: {0}")]
    Encryption(String),

    /// 복호화 실패
    #[error("Failed to decrypt history (corrupted or wrong key)")]
    DecryptionFailed,

    /// 인코딩 에러
    #[error("Encoding error: {0}")]
    Encoding(String),

    /// 암호화 키 없음
    #[error("No encryption key available")]
    NoEncryptionKey,

    /// 잘못된 키
    #[error("Invalid encryption key")]
    InvalidKey,
}
