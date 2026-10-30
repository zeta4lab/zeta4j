//! 공간 인덱스 기본 타입 정의
//!
//! Point2D, Point3D, BoundingBox, BoundingBox3D 등 공간 데이터 타입을 정의합니다.

use serde::{Deserialize, Serialize};

/// 2D 좌표점
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

impl Point2D {
    /// 새 점 생성
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// 두 점 사이의 유클리드 거리
    pub fn distance(&self, other: &Point2D) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// 두 점 사이의 거리 제곱 (sqrt 생략으로 성능 최적화)
    pub fn distance_squared(&self, other: &Point2D) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx * dx + dy * dy
    }
}

/// 최소 경계 사각형 (Minimum Bounding Rectangle)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox {
    /// 좌하단 좌표
    pub min: Point2D,
    /// 우상단 좌표
    pub max: Point2D,
}

impl BoundingBox {
    /// 새 경계 사각형 생성
    pub fn new(min: Point2D, max: Point2D) -> Self {
        Self { min, max }
    }

    /// 좌표로 직접 생성
    pub fn from_coords(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            min: Point2D::new(min_x, min_y),
            max: Point2D::new(max_x, max_y),
        }
    }

    /// 단일 점에서 생성
    pub fn from_point(point: Point2D) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    /// 너비
    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    /// 높이
    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    /// 면적
    pub fn area(&self) -> f64 {
        self.width() * self.height()
    }

    /// 둘레
    pub fn perimeter(&self) -> f64 {
        2.0 * (self.width() + self.height())
    }

    /// 중심점
    pub fn center(&self) -> Point2D {
        Point2D::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }

    /// 점 포함 여부
    pub fn contains_point(&self, point: &Point2D) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// 다른 경계 사각형 완전 포함 여부
    pub fn contains_bbox(&self, other: &BoundingBox) -> bool {
        self.min.x <= other.min.x
            && self.max.x >= other.max.x
            && self.min.y <= other.min.y
            && self.max.y >= other.max.y
    }

    /// 다른 경계 사각형과 교차 여부
    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }

    /// 두 경계 사각형 병합 (Union)
    pub fn merge(&self, other: &BoundingBox) -> BoundingBox {
        BoundingBox {
            min: Point2D::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Point2D::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    /// 점을 포함하도록 확장
    pub fn extend_point(&self, point: &Point2D) -> BoundingBox {
        BoundingBox {
            min: Point2D::new(self.min.x.min(point.x), self.min.y.min(point.y)),
            max: Point2D::new(self.max.x.max(point.x), self.max.y.max(point.y)),
        }
    }

    /// 병합 시 면적 증가량 (Enlargement)
    /// R-Tree 삽입 시 최적 노드 선택에 사용
    pub fn enlargement(&self, other: &BoundingBox) -> f64 {
        self.merge(other).area() - self.area()
    }

    /// 점까지의 최소 거리
    pub fn min_distance_to_point(&self, point: &Point2D) -> f64 {
        let dx = if point.x < self.min.x {
            self.min.x - point.x
        } else if point.x > self.max.x {
            point.x - self.max.x
        } else {
            0.0
        };

        let dy = if point.y < self.min.y {
            self.min.y - point.y
        } else if point.y > self.max.y {
            point.y - self.max.y
        } else {
            0.0
        };

        (dx * dx + dy * dy).sqrt()
    }

    /// 점까지의 최소 거리 제곱 (성능 최적화)
    pub fn min_distance_squared_to_point(&self, point: &Point2D) -> f64 {
        let dx = if point.x < self.min.x {
            self.min.x - point.x
        } else if point.x > self.max.x {
            point.x - self.max.x
        } else {
            0.0
        };

        let dy = if point.y < self.min.y {
            self.min.y - point.y
        } else if point.y > self.max.y {
            point.y - self.max.y
        } else {
            0.0
        };

        dx * dx + dy * dy
    }

    /// 유효한 경계 사각형인지 확인
    pub fn is_valid(&self) -> bool {
        self.min.x <= self.max.x && self.min.y <= self.max.y
    }

    /// 두 경계 사각형의 교차 영역 면적 (Overlap Area)
    /// R*-Tree ChooseSubtree에서 사용
    pub fn overlap_area(&self, other: &BoundingBox) -> f64 {
        let x_overlap = (self.max.x.min(other.max.x) - self.min.x.max(other.min.x)).max(0.0);
        let y_overlap = (self.max.y.min(other.max.y) - self.min.y.max(other.min.y)).max(0.0);
        x_overlap * y_overlap
    }

    /// margin 계산 (둘레의 절반) - R*-Tree Split에서 사용
    pub fn margin(&self) -> f64 {
        self.width() + self.height()
    }
}

// ==================== 3D 공간 타입 ====================

/// 3D 좌표점
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Point3D {
    /// 새 3D 점 생성
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// 두 점 사이의 유클리드 거리
    pub fn distance(&self, other: &Point3D) -> f64 {
        self.distance_squared(other).sqrt()
    }

    /// 두 점 사이의 거리 제곱 (sqrt 생략으로 성능 최적화)
    pub fn distance_squared(&self, other: &Point3D) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx * dx + dy * dy + dz * dz
    }

    /// 2D 점으로 변환 (z 좌표 무시)
    pub fn to_2d(&self) -> Point2D {
        Point2D::new(self.x, self.y)
    }
}

/// 3D 최소 경계 직육면체 (Minimum Bounding Box)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundingBox3D {
    /// 최소 좌표 (좌하단 앞)
    pub min: Point3D,
    /// 최대 좌표 (우상단 뒤)
    pub max: Point3D,
}

impl BoundingBox3D {
    /// 새 3D 경계 직육면체 생성
    pub fn new(min: Point3D, max: Point3D) -> Self {
        Self { min, max }
    }

    /// 좌표로 직접 생성
    pub fn from_coords(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        Self {
            min: Point3D::new(min_x, min_y, min_z),
            max: Point3D::new(max_x, max_y, max_z),
        }
    }

    /// 단일 점에서 생성
    pub fn from_point(point: Point3D) -> Self {
        Self {
            min: point,
            max: point,
        }
    }

    /// X축 너비
    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    /// Y축 높이
    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }

    /// Z축 깊이
    pub fn depth(&self) -> f64 {
        self.max.z - self.min.z
    }

    /// 부피
    pub fn volume(&self) -> f64 {
        self.width() * self.height() * self.depth()
    }

    /// 표면적
    pub fn surface_area(&self) -> f64 {
        let w = self.width();
        let h = self.height();
        let d = self.depth();
        2.0 * (w * h + h * d + d * w)
    }

    /// 중심점
    pub fn center(&self) -> Point3D {
        Point3D::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
            (self.min.z + self.max.z) / 2.0,
        )
    }

    /// 점 포함 여부
    pub fn contains_point(&self, point: &Point3D) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    /// 다른 경계 직육면체 완전 포함 여부
    pub fn contains_bbox(&self, other: &BoundingBox3D) -> bool {
        self.min.x <= other.min.x
            && self.max.x >= other.max.x
            && self.min.y <= other.min.y
            && self.max.y >= other.max.y
            && self.min.z <= other.min.z
            && self.max.z >= other.max.z
    }

    /// 다른 경계 직육면체와 교차 여부
    pub fn intersects(&self, other: &BoundingBox3D) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }

    /// 두 경계 직육면체 병합 (Union)
    pub fn merge(&self, other: &BoundingBox3D) -> BoundingBox3D {
        BoundingBox3D {
            min: Point3D::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            max: Point3D::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            ),
        }
    }

    /// 점을 포함하도록 확장
    pub fn extend_point(&self, point: &Point3D) -> BoundingBox3D {
        BoundingBox3D {
            min: Point3D::new(
                self.min.x.min(point.x),
                self.min.y.min(point.y),
                self.min.z.min(point.z),
            ),
            max: Point3D::new(
                self.max.x.max(point.x),
                self.max.y.max(point.y),
                self.max.z.max(point.z),
            ),
        }
    }

    /// 병합 시 부피 증가량 (Enlargement)
    pub fn enlargement(&self, other: &BoundingBox3D) -> f64 {
        self.merge(other).volume() - self.volume()
    }

    /// 점까지의 최소 거리
    pub fn min_distance_to_point(&self, point: &Point3D) -> f64 {
        self.min_distance_squared_to_point(point).sqrt()
    }

    /// 점까지의 최소 거리 제곱 (성능 최적화)
    pub fn min_distance_squared_to_point(&self, point: &Point3D) -> f64 {
        let dx = if point.x < self.min.x {
            self.min.x - point.x
        } else if point.x > self.max.x {
            point.x - self.max.x
        } else {
            0.0
        };

        let dy = if point.y < self.min.y {
            self.min.y - point.y
        } else if point.y > self.max.y {
            point.y - self.max.y
        } else {
            0.0
        };

        let dz = if point.z < self.min.z {
            self.min.z - point.z
        } else if point.z > self.max.z {
            point.z - self.max.z
        } else {
            0.0
        };

        dx * dx + dy * dy + dz * dz
    }

    /// 두 경계 직육면체의 교차 영역 부피 (Overlap Volume)
    pub fn overlap_volume(&self, other: &BoundingBox3D) -> f64 {
        let x_overlap = (self.max.x.min(other.max.x) - self.min.x.max(other.min.x)).max(0.0);
        let y_overlap = (self.max.y.min(other.max.y) - self.min.y.max(other.min.y)).max(0.0);
        let z_overlap = (self.max.z.min(other.max.z) - self.min.z.max(other.min.z)).max(0.0);
        x_overlap * y_overlap * z_overlap
    }

    /// margin 계산 (표면적의 일부) - R*-Tree Split에서 사용
    pub fn margin(&self) -> f64 {
        self.width() + self.height() + self.depth()
    }

    /// 유효한 경계 직육면체인지 확인
    pub fn is_valid(&self) -> bool {
        self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z
    }

    /// 2D 경계 사각형으로 변환 (z 좌표 무시)
    pub fn to_2d(&self) -> BoundingBox {
        BoundingBox::new(self.min.to_2d(), self.max.to_2d())
    }
}
