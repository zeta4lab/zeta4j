//! 공간 인덱스 관리자
//!
//! GraphEngine과 통합되어 공간 인덱스를 관리합니다.

use super::rtree::RTree;
use super::types::{BoundingBox, Point2D};
use crate::core::{NodeId, Value};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::collections::HashMap;

/// 공간 인덱스 관리자
///
/// 레이블과 프로퍼티 조합별로 R-Tree 인덱스를 관리합니다.
#[derive(Debug, Default)]
#[expect(clippy::type_complexity)]
pub struct SpatialIndexManager {
    /// 레이블 -> (프로퍼티명 -> R-Tree)
    indexes: RwLock<HashMap<String, HashMap<String, RTree>>>,
    /// 노드 ID -> (레이블, 프로퍼티명, 좌표) 역매핑 (삭제 시 사용)
    node_locations: RwLock<HashMap<NodeId, Vec<(String, String, Point2D)>>>,
}

impl SpatialIndexManager {
    /// Remove every derived spatial index and reverse mapping.
    pub fn clear(&self) {
        self.indexes.write().clear();
        self.node_locations.write().clear();
    }

    /// 새 공간 인덱스 관리자 생성
    pub fn new() -> Self {
        Self {
            indexes: RwLock::new(HashMap::new()),
            node_locations: RwLock::new(HashMap::new()),
        }
    }

    /// 공간 인덱스 생성
    pub fn create_index(&self, label: &str, property: &str) -> Result<(), String> {
        let mut indexes = self.indexes.write();

        let label_indexes = indexes.entry(label.to_string()).or_default();

        if label_indexes.contains_key(property) {
            return Err(format!(
                "Spatial index already exists for :{}({})",
                label, property
            ));
        }

        label_indexes.insert(property.to_string(), RTree::new());
        Ok(())
    }

    /// 공간 인덱스 삭제
    pub fn drop_index(&self, label: &str, property: &str) -> Result<(), String> {
        let mut indexes = self.indexes.write();

        if let Some(label_indexes) = indexes.get_mut(label)
            && label_indexes.remove(property).is_some()
        {
            if label_indexes.is_empty() {
                indexes.remove(label);
            }
            return Ok(());
        }

        Err(format!(
            "Spatial index not found for :{}({})",
            label, property
        ))
    }

    /// 인덱스 존재 여부 확인
    pub fn has_index(&self, label: &str, property: &str) -> bool {
        let indexes = self.indexes.read();
        indexes
            .get(label)
            .map(|props| props.contains_key(property))
            .unwrap_or(false)
    }

    /// 모든 공간 인덱스 목록 반환
    pub fn list_indexes(&self) -> Vec<(String, String)> {
        let indexes = self.indexes.read();
        let mut result = Vec::new();

        for (label, props) in indexes.iter() {
            for property in props.keys() {
                result.push((label.clone(), property.clone()));
            }
        }

        result
    }

    /// 노드 생성 시 인덱스 업데이트
    pub fn on_node_created(
        &self,
        node_id: NodeId,
        labels: &[String],
        properties: &IndexMap<String, Value>,
    ) {
        // 1단계: 어떤 인덱스에 삽입해야 하는지 파악하고 역매핑 업데이트
        let points_to_insert: Vec<(String, String, Point2D)> = {
            let indexes = self.indexes.read();
            let mut locations = self.node_locations.write();
            let mut result = Vec::new();

            for label in labels {
                if let Some(label_indexes) = indexes.get(label) {
                    for property in label_indexes.keys() {
                        if let Some(value) = properties.get(property)
                            && let Some(point) = Self::value_to_point(value)
                        {
                            // 역매핑에 추가
                            locations.entry(node_id).or_default().push((
                                label.clone(),
                                property.clone(),
                                point,
                            ));
                            result.push((label.clone(), property.clone(), point));
                        }
                    }
                }
            }

            result
        }; // 모든 lock 해제

        // 2단계: 실제 인덱스에 삽입 (write lock 필요)
        if !points_to_insert.is_empty() {
            let mut indexes = self.indexes.write();

            for (label, property, point) in points_to_insert {
                if let Some(label_indexes) = indexes.get_mut(&label)
                    && let Some(tree) = label_indexes.get_mut(&property)
                {
                    tree.insert_point(point, node_id);
                }
            }
        }
    }

    /// 노드 삭제 시 인덱스 업데이트
    pub fn on_node_deleted(&self, node_id: NodeId) {
        let mut indexes = self.indexes.write();
        let mut locations = self.node_locations.write();

        if let Some(node_locs) = locations.remove(&node_id) {
            for (label, property, _point) in node_locs {
                if let Some(label_indexes) = indexes.get_mut(&label)
                    && let Some(tree) = label_indexes.get_mut(&property)
                {
                    tree.delete(node_id);
                }
            }
        }
    }

    /// 노드 업데이트 시 인덱스 업데이트
    pub fn on_node_updated(
        &self,
        node_id: NodeId,
        labels: &[String],
        properties: &IndexMap<String, Value>,
    ) {
        // 기존 엔트리 삭제 후 새로 추가
        self.on_node_deleted(node_id);
        self.on_node_created(node_id, labels, properties);
    }

    /// 범위 내 노드 검색
    pub fn find_within_bbox(&self, label: &str, property: &str, bbox: &BoundingBox) -> Vec<NodeId> {
        let indexes = self.indexes.read();

        indexes
            .get(label)
            .and_then(|props| props.get(property))
            .map(|tree| tree.search(bbox))
            .unwrap_or_default()
    }

    /// 점 포함 노드 검색
    pub fn find_containing_point(
        &self,
        label: &str,
        property: &str,
        point: &Point2D,
    ) -> Vec<NodeId> {
        let indexes = self.indexes.read();

        indexes
            .get(label)
            .and_then(|props| props.get(property))
            .map(|tree| tree.search_point(point))
            .unwrap_or_default()
    }

    /// 거리 내 노드 검색
    pub fn find_within_distance(
        &self,
        label: &str,
        property: &str,
        center: &Point2D,
        distance: f64,
    ) -> Vec<(NodeId, f64)> {
        let indexes = self.indexes.read();

        indexes
            .get(label)
            .and_then(|props| props.get(property))
            .map(|tree| tree.search_within_distance(center, distance))
            .unwrap_or_default()
    }

    /// K-최근접 이웃 검색
    pub fn find_nearest(
        &self,
        label: &str,
        property: &str,
        point: &Point2D,
        k: usize,
    ) -> Vec<(NodeId, f64)> {
        let indexes = self.indexes.read();

        indexes
            .get(label)
            .and_then(|props| props.get(property))
            .map(|tree| tree.nearest(point, k))
            .unwrap_or_default()
    }

    /// 인덱스 통계 반환
    pub fn get_stats(&self, label: &str, property: &str) -> Option<SpatialIndexStats> {
        let indexes = self.indexes.read();

        indexes
            .get(label)
            .and_then(|props| props.get(property))
            .map(|tree| SpatialIndexStats {
                label: label.to_string(),
                property: property.to_string(),
                entry_count: tree.len(),
                height: tree.height(),
            })
    }

    pub fn memory_size(&self) -> usize {
        let indexes = self.indexes.read();
        let trees = indexes
            .iter()
            .map(|(label, properties)| {
                label.len().saturating_add(
                    properties
                        .iter()
                        .map(|(property, tree)| property.len().saturating_add(tree.memory_usage()))
                        .fold(0usize, usize::saturating_add),
                )
            })
            .fold(0usize, usize::saturating_add);
        let reverse = self
            .node_locations
            .read()
            .values()
            .map(|locations| {
                locations
                    .iter()
                    .map(|(label, property, _)| {
                        std::mem::size_of::<(String, String, Point2D)>()
                            .saturating_add(label.len())
                            .saturating_add(property.len())
                    })
                    .fold(0usize, usize::saturating_add)
            })
            .fold(0usize, usize::saturating_add);
        trees.saturating_add(reverse)
    }

    /// Value를 Point2D로 변환
    fn value_to_point(value: &Value) -> Option<Point2D> {
        match value {
            // Map 형식: {x: 1.0, y: 2.0} 또는 {latitude: ..., longitude: ...}
            Value::Map(map) => {
                // x, y 형식
                if let (Some(Value::Float(x)), Some(Value::Float(y))) = (map.get("x"), map.get("y"))
                {
                    return Some(Point2D::new(*x, *y));
                }
                if let (Some(Value::Integer(x)), Some(Value::Integer(y))) =
                    (map.get("x"), map.get("y"))
                {
                    return Some(Point2D::new(*x as f64, *y as f64));
                }

                // latitude, longitude 형식
                if let (Some(Value::Float(lat)), Some(Value::Float(lon))) =
                    (map.get("latitude"), map.get("longitude"))
                {
                    return Some(Point2D::new(*lon, *lat)); // x=lon, y=lat
                }
                if let (Some(Value::Integer(lat)), Some(Value::Integer(lon))) =
                    (map.get("latitude"), map.get("longitude"))
                {
                    return Some(Point2D::new(*lon as f64, *lat as f64));
                }

                None
            }
            // Array 형식: [x, y]
            Value::Array(arr) if arr.len() >= 2 => {
                let x = match &arr[0] {
                    Value::Float(f) => Some(*f),
                    Value::Integer(i) => Some(*i as f64),
                    _ => None,
                };
                let y = match &arr[1] {
                    Value::Float(f) => Some(*f),
                    Value::Integer(i) => Some(*i as f64),
                    _ => None,
                };

                match (x, y) {
                    (Some(x), Some(y)) => Some(Point2D::new(x, y)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// 공간 인덱스 통계
#[derive(Debug, Clone)]
pub struct SpatialIndexStats {
    pub label: String,
    pub property: String,
    pub entry_count: usize,
    pub height: usize,
}
