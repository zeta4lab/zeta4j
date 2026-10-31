// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Lock 헬퍼 함수
//!
//! Mutex 및 RwLock 작업을 위한 에러 처리 헬퍼입니다.
//!
//! # 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::storage::util::lock_helpers::{lock_mutex, read_lock, write_lock};
//!
//! // Mutex 락
//! let guard = lock_mutex(&mutex, "page")?;
//!
//! // RwLock 읽기
//! let guard = read_lock(&rwlock, "page_table")?;
//!
//! // RwLock 쓰기
//! let guard = write_lock(&rwlock, "page_table")?;
//! ```

use crate::error::{GraphError, Result};
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Mutex 락 획득 헬퍼
///
/// `std::sync::Mutex`의 lock을 획득하고 에러를 `GraphError::Lock`으로 변환합니다.
#[inline]
pub fn lock_mutex<'a, T>(guard: &'a Mutex<T>, resource: &str) -> Result<MutexGuard<'a, T>> {
    guard.lock().map_err(|e| GraphError::Lock {
        resource: resource.to_string(),
        message: e.to_string(),
    })
}

/// Mutex 락 획득 헬퍼 (동적 리소스 이름)
///
/// 리소스 이름에 ID를 포함할 때 사용합니다.
#[inline]
pub fn lock_mutex_with_id<'a, T>(
    guard: &'a Mutex<T>,
    resource: &str,
    id: impl std::fmt::Display,
) -> Result<MutexGuard<'a, T>> {
    guard.lock().map_err(|e| GraphError::Lock {
        resource: format!("{}_{}", resource, id),
        message: e.to_string(),
    })
}

/// RwLock 읽기 락 획득 헬퍼
///
/// `std::sync::RwLock`의 read lock을 획득하고 에러를 `GraphError::Lock`으로 변환합니다.
#[inline]
pub fn read_lock<'a, T>(guard: &'a RwLock<T>, resource: &str) -> Result<RwLockReadGuard<'a, T>> {
    guard.read().map_err(|e| GraphError::Lock {
        resource: resource.to_string(),
        message: e.to_string(),
    })
}

/// RwLock 쓰기 락 획득 헬퍼
///
/// `std::sync::RwLock`의 write lock을 획득하고 에러를 `GraphError::Lock`으로 변환합니다.
#[inline]
pub fn write_lock<'a, T>(guard: &'a RwLock<T>, resource: &str) -> Result<RwLockWriteGuard<'a, T>> {
    guard.write().map_err(|e| GraphError::Lock {
        resource: resource.to_string(),
        message: e.to_string(),
    })
}

/// Arc<Mutex<T>>용 락 획득 헬퍼
///
/// `Arc<Mutex<T>>`의 lock을 획득합니다.
#[inline]
pub fn lock_arc_mutex<'a, T>(
    guard: &'a std::sync::Arc<Mutex<T>>,
    resource: &str,
) -> Result<MutexGuard<'a, T>> {
    guard.lock().map_err(|e| GraphError::Lock {
        resource: resource.to_string(),
        message: e.to_string(),
    })
}

/// Arc<Mutex<T>>용 락 획득 헬퍼 (동적 리소스 이름)
///
/// 리소스 이름에 ID를 포함할 때 사용합니다.
#[inline]
pub fn lock_arc_mutex_with_id<'a, T>(
    guard: &'a std::sync::Arc<Mutex<T>>,
    resource: &str,
    id: impl std::fmt::Display,
) -> Result<MutexGuard<'a, T>> {
    guard.lock().map_err(|e| GraphError::Lock {
        resource: format!("{}_{}", resource, id),
        message: e.to_string(),
    })
}
