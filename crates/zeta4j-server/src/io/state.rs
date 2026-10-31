// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 상태 관리 및 JSON I/O 모듈
//!
//! 라우터 AppState에서 공통으로 사용하는 상태 관리 패턴과 JSON 파일 I/O를 제공합니다.
//!
//! # StateManager 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::io::StateManager;
//!
//! struct MyData { name: String }
//!
//! let manager = StateManager::<MyData>::new();
//! manager.insert("id1".to_string(), MyData { name: "test".into() });
//! let data = manager.get("id1");
//! ```
//!
//! # JSON I/O 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::io::{save_json, load_json};
//! use std::path::Path;
//!
//! #[derive(Serialize, Deserialize)]
//! struct Config { name: String }
//!
//! // 저장
//! save_json(Path::new("config.json"), &Config { name: "test".into() })?;
//!
//! // 로드
//! let config: Config = load_json(Path::new("config.json"))?;
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use parking_lot::RwLock;
use serde::{Serialize, de::DeserializeOwned};

// ============================================================================
// JSON I/O Helpers
// ============================================================================

/// JSON 파일 저장
///
/// 데이터를 pretty-print JSON 형식으로 파일에 저장합니다.
///
/// # Arguments
/// * `path` - 저장할 파일 경로
/// * `data` - 직렬화할 데이터
///
/// # Errors
/// 직렬화 또는 파일 쓰기 실패 시 에러 메시지 반환
pub fn save_json<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(data)
        .map_err(|e| format!("Failed to serialize JSON: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    Ok(())
}

/// JSON 파일 로드
///
/// 파일에서 JSON을 읽어 역직렬화합니다.
///
/// # Arguments
/// * `path` - 로드할 파일 경로
///
/// # Errors
/// 파일 읽기 또는 파싱 실패 시 에러 메시지 반환
pub fn load_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let json = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&json).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// JSON 파일 로드 (파일 없으면 기본값)
///
/// 파일이 존재하지 않으면 기본값을 반환합니다.
///
/// # Arguments
/// * `path` - 로드할 파일 경로
///
/// # Errors
/// 파일이 존재하지만 파싱 실패 시 에러 메시지 반환
pub fn load_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    if !path.exists() {
        return Ok(T::default());
    }
    load_json(path)
}

// ============================================================================
// StateManager
// ============================================================================

/// 상태 관리자
///
/// `RwLock<HashMap<String, T>>` 패턴을 캡슐화하여 일관된 CRUD 인터페이스를 제공합니다.
pub struct StateManager<T> {
    data: RwLock<HashMap<String, T>>,
}

impl<T> StateManager<T> {
    /// 빈 StateManager 생성
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    /// 초기 데이터로 StateManager 생성
    pub fn with_data(data: HashMap<String, T>) -> Self {
        Self {
            data: RwLock::new(data),
        }
    }

    /// 항목 개수 반환
    pub fn count(&self) -> usize {
        self.data.read().len()
    }

    /// 비어있는지 확인
    pub fn is_empty(&self) -> bool {
        self.data.read().is_empty()
    }

    /// 키 존재 여부 확인
    pub fn contains(&self, id: &str) -> bool {
        self.data.read().contains_key(id)
    }

    /// 모든 키 목록 반환
    pub fn keys(&self) -> Vec<String> {
        self.data.read().keys().cloned().collect()
    }

    /// 항목 삽입 또는 업데이트
    pub fn insert(&self, id: String, value: T) -> Option<T> {
        self.data.write().insert(id, value)
    }

    /// 항목 삭제
    pub fn remove(&self, id: &str) -> Option<T> {
        self.data.write().remove(id)
    }

    /// 모든 항목 삭제
    pub fn clear(&self) {
        self.data.write().clear();
    }

    /// 읽기 잠금으로 내부 데이터 접근
    ///
    /// 복잡한 읽기 작업이 필요한 경우 사용
    pub fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&HashMap<String, T>) -> R,
    {
        f(&self.data.read())
    }

    /// 쓰기 잠금으로 내부 데이터 접근
    ///
    /// 복잡한 쓰기 작업이 필요한 경우 사용
    pub fn write<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut HashMap<String, T>) -> R,
    {
        f(&mut self.data.write())
    }
}

impl<T: Clone> StateManager<T> {
    /// 항목 조회 (복제)
    pub fn get(&self, id: &str) -> Option<T> {
        self.data.read().get(id).cloned()
    }

    /// 모든 값 반환 (복제)
    pub fn values(&self) -> Vec<T> {
        self.data.read().values().cloned().collect()
    }

    /// 모든 항목 반환 (복제)
    pub fn entries(&self) -> Vec<(String, T)> {
        self.data
            .read()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

impl<T> Default for StateManager<T> {
    fn default() -> Self {
        Self::new()
    }
}
