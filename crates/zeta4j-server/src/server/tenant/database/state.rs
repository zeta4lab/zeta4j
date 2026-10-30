//! 데이터베이스 상태 타입.

use serde::{Deserialize, Serialize};

/// 데이터베이스 상태
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DatabaseState {
    /// 생성 중
    Creating,
    /// 시작 중
    Starting,
    /// 온라인 (사용 가능)
    Online,
    /// 중지 중
    Stopping,
    /// 오프라인
    Offline,
    /// 에러 상태
    Error(String),
    /// 삭제됨
    Dropped,
}

impl DatabaseState {
    /// 온라인 여부
    pub fn is_online(&self) -> bool {
        matches!(self, DatabaseState::Online)
    }

    /// 전환 중 여부
    pub fn is_transitioning(&self) -> bool {
        matches!(
            self,
            DatabaseState::Creating | DatabaseState::Starting | DatabaseState::Stopping
        )
    }

    /// 사용 가능 여부
    pub fn is_available(&self) -> bool {
        matches!(self, DatabaseState::Online)
    }
}

impl std::fmt::Display for DatabaseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseState::Creating => write!(f, "creating"),
            DatabaseState::Starting => write!(f, "starting"),
            DatabaseState::Online => write!(f, "online"),
            DatabaseState::Stopping => write!(f, "stopping"),
            DatabaseState::Offline => write!(f, "offline"),
            DatabaseState::Error(e) => write!(f, "error: {}", e),
            DatabaseState::Dropped => write!(f, "dropped"),
        }
    }
}
