//! 직렬화 트레이트
//!
//! Postcard 기반 직렬화/역직렬화를 위한 공통 트레이트입니다.

use crate::error::GraphError;
use serde::{Serialize, de::DeserializeOwned};

/// Postcard 직렬화/역직렬화 트레이트
///
/// Node, Edge 등 그래프 요소의 바이트 변환을 위한 공통 인터페이스를 제공합니다.
///
/// # Example
/// ```ignore
/// use zeta4j_graph::core::{Node, PostcardSerializable};
///
/// let node = Node::new(1);
/// let bytes = node.to_bytes()?;
/// let restored = Node::from_bytes(&bytes)?;
/// ```
pub trait PostcardSerializable: Serialize + DeserializeOwned + Sized {
    /// 바이트 배열로 직렬화
    fn to_bytes(&self) -> Result<Vec<u8>, GraphError> {
        zeta4j_codec::serialize(self).map_err(|e| GraphError::Serialization {
            message: e.to_string(),
            source: None,
        })
    }

    /// 바이트 배열에서 역직렬화
    fn from_bytes(bytes: &[u8]) -> Result<Self, GraphError> {
        zeta4j_codec::deserialize(bytes).map_err(|e| GraphError::Serialization {
            message: e.to_string(),
            source: None,
        })
    }
}
