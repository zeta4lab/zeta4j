//! 데이터베이스 ID 타입.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 데이터베이스 고유 식별자
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DatabaseId(Uuid);

impl DatabaseId {
    /// 새 ID 생성
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// UUID에서 생성
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Canonical identity of the built-in default database.
    ///
    /// A fixed identity keeps the default database stable across catalog
    /// re-initialization.
    pub fn default_database() -> Self {
        Self(Uuid::from_u128(0x7a65_7461_3467_0000_0000_0000_0000_0001))
    }

    /// 내부 UUID 반환
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for DatabaseId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for DatabaseId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_database_identity_is_stable() {
        assert_eq!(
            DatabaseId::default_database(),
            DatabaseId::default_database()
        );
        assert_ne!(DatabaseId::default_database(), DatabaseId::new());
    }
}
