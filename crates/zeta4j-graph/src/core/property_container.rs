//! PropertyContainer 트레이트
//!
//! Node와 Edge에서 공통으로 사용하는 속성 관리 인터페이스를 정의합니다.

use crate::core::{Property, Value};

/// 속성을 포함하는 요소를 위한 트레이트
///
/// Node와 Edge 모두 속성(properties)을 가지므로, 공통 인터페이스를 제공합니다.
pub trait PropertyContainer {
    /// 속성 객체의 불변 참조 반환
    fn properties(&self) -> &Property;

    /// 속성 객체의 가변 참조 반환
    fn properties_mut(&mut self) -> &mut Property;

    /// 속성 설정
    fn set_property<K: Into<String>, V: Into<Value>>(&mut self, key: K, value: V) {
        self.properties_mut().set(key, value);
    }

    /// 속성 조회
    fn get_property(&self, key: &str) -> Option<&Value> {
        self.properties().get(key)
    }

    /// 속성 제거
    fn remove_property(&mut self, key: &str) -> Option<Value> {
        self.properties_mut().remove(key)
    }

    /// 속성 존재 여부 확인
    fn has_property(&self, key: &str) -> bool {
        self.properties().contains_key(key)
    }

    /// 속성 개수 반환
    fn property_count(&self) -> usize {
        self.properties().len()
    }
}
