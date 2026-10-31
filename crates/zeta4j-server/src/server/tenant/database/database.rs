// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! 테넌트 데이터베이스 구현.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use zeta4j_graph::engine::GraphEngine;

use crate::server::tenant::namespace::TenantId;
use crate::server::tenant::resource::ResourceLimits;

use super::{
    DatabaseContext, DatabaseId, DatabaseMetadata, DatabaseState, DatabaseStatistics,
    TenantDatabaseConfig,
};

/// 테넌트 데이터베이스
///
/// 동시성 안전: 상태 전환 락을 사용하여 TOCTOU 취약점 방지
pub struct TenantDatabase {
    /// 데이터베이스 ID
    pub id: DatabaseId,
    /// 데이터베이스 이름
    pub name: String,
    /// 테넌트 ID
    pub tenant_id: Option<TenantId>,
    /// 데이터베이스 상태
    pub(super) state: RwLock<DatabaseState>,
    /// 상태 전환 락 (TOCTOU 방지)
    state_transition_lock: Mutex<()>,
    /// 그래프 엔진
    pub engine: Option<Arc<GraphEngine>>,
    /// 데이터 경로 (예: ~/.zeta4j/databases/zeta4j/)
    pub data_path: Option<PathBuf>,
    /// 논리 DB별 unified redb와 commit coordinator.
    pub storage_context: Option<Arc<DatabaseContext>>,
    /// 리소스 제한
    pub resource_limits: ResourceLimits,
    /// 데이터베이스 설정
    pub config: TenantDatabaseConfig,
    /// 통계
    pub stats: DatabaseStatistics,
    /// 메타데이터
    pub metadata: DatabaseMetadata,
}

impl TenantDatabase {
    /// 새 데이터베이스 생성
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: DatabaseId::new(),
            name: name.into(),
            tenant_id: None,
            state: RwLock::new(DatabaseState::Offline),
            state_transition_lock: Mutex::new(()),
            engine: None,
            data_path: None,
            storage_context: None,
            resource_limits: ResourceLimits::default(),
            config: TenantDatabaseConfig::default(),
            stats: DatabaseStatistics::new(),
            metadata: DatabaseMetadata::new(),
        }
    }

    /// 기본 데이터베이스 (zeta4j) 생성
    pub fn default_database() -> Self {
        let mut db = Self::new("zeta4j");
        db.id = DatabaseId::default_database();
        db.metadata = DatabaseMetadata::default_database();
        db.state = RwLock::new(DatabaseState::Online);
        db
    }

    /// 시스템 데이터베이스 생성
    pub fn system_database() -> Self {
        let mut db = Self::new("system");
        db.metadata = DatabaseMetadata::system_database();
        db.state = RwLock::new(DatabaseState::Online);
        db
    }

    /// 엔진 설정
    pub fn with_engine(mut self, engine: Arc<GraphEngine>) -> Self {
        self.engine = Some(engine);
        self
    }

    /// 데이터 경로 설정
    pub fn with_data_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.data_path = Some(path.into());
        self
    }

    /// 테넌트 설정
    pub fn with_tenant(mut self, tenant_id: TenantId) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    /// 리소스 제한 설정
    pub fn with_resource_limits(mut self, limits: ResourceLimits) -> Self {
        self.resource_limits = limits;
        self
    }

    /// 설정 적용
    pub fn with_config(mut self, config: TenantDatabaseConfig) -> Self {
        self.config = config;
        self
    }

    /// 현재 상태 조회
    pub async fn state(&self) -> DatabaseState {
        self.state.read().await.clone()
    }

    /// 상태 변경
    pub async fn set_state(&self, new_state: DatabaseState) {
        let mut state = self.state.write().await;
        *state = new_state;
    }

    /// 상태 전환 락 획득
    ///
    /// 동시성 안전: 상태 전환 작업 전에 이 락을 획득하여 TOCTOU 방지
    pub async fn acquire_state_transition_lock(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.state_transition_lock.lock().await
    }

    /// CAS(Compare-And-Swap) 패턴의 상태 변경
    ///
    /// 현재 상태가 expected_state와 일치하면 new_state로 변경하고 true 반환.
    /// 일치하지 않으면 false 반환 (동시 변경 감지).
    ///
    /// 주의: 이 메서드는 acquire_state_transition_lock() 락 내에서 호출해야 안전합니다.
    pub async fn try_set_state(
        &self,
        expected_state: DatabaseState,
        new_state: DatabaseState,
    ) -> bool {
        let mut state = self.state.write().await;
        if *state == expected_state {
            *state = new_state;
            true
        } else {
            false
        }
    }

    /// 온라인 여부
    pub async fn is_online(&self) -> bool {
        self.state.read().await.is_online()
    }

    /// 기본 데이터베이스 여부
    pub fn is_default(&self) -> bool {
        self.metadata.is_default
    }

    /// 시스템 데이터베이스 여부
    pub fn is_system(&self) -> bool {
        self.metadata.is_system
    }

    /// 엔진 참조 반환
    pub fn get_engine(&self) -> Option<&Arc<GraphEngine>> {
        self.engine.as_ref()
    }
}
