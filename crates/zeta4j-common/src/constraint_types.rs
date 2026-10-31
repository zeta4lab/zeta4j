// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 제약조건 관련 공유 타입
//!
//! 여러 크레이트에서 공유하는 제약조건 관련 enum 타입들을 정의합니다.

use serde::{Deserialize, Serialize};

/// 연쇄 동작
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CascadeAction {
    /// 연쇄 삭제
    Delete,
    /// NULL로 설정
    SetNull,
    /// 기본값으로 설정
    SetDefault,
    /// 제한 (삭제 불가)
    Restrict,
}

/// 참조 무결성 동작
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferentialAction {
    /// 동작 없음
    NoAction,
    /// 연쇄 동작
    Cascade,
    /// NULL로 설정
    SetNull,
    /// 기본값으로 설정
    SetDefault,
    /// 제한
    Restrict,
}
