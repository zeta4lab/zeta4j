//! 공간 인덱스 모듈
//!
//! R*-Tree 기반 2D/3D 공간 인덱싱을 제공합니다.
//!
//! ## 주요 타입
//!
//! ### 2D 공간 인덱스
//! - `Point2D`: 2D 좌표점
//! - `BoundingBox`: 최소 경계 사각형 (MBR)
//! - `RTree`: 2D R*-Tree 자료구조
//!
//! ### 3D 공간 인덱스
//! - `Point3D`: 3D 좌표점
//! - `BoundingBox3D`: 최소 경계 직육면체 (MBB)
//! - `RTree3D`: 3D R*-Tree 자료구조
//!
//! - `SpatialIndexManager`: 공간 인덱스 관리자
//!
//! ## 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_graph::index::spatial::{RTree, Point2D, BoundingBox};
//! use zeta4j_graph::index::spatial::{RTree3D, Point3D, BoundingBox3D};
//!
//! // 2D R*-Tree
//! let mut tree = RTree::new();
//! tree.insert_point(Point2D::new(5.0, 5.0), 1);
//!
//! // 3D R*-Tree
//! let mut tree3d = RTree3D::new();
//! tree3d.insert_point(Point3D::new(5.0, 5.0, 5.0), 1);
//!
//! // 범위 검색
//! let query = BoundingBox::from_coords(0.0, 0.0, 7.0, 7.0);
//! let results = tree.search(&query);
//!
//! // K-최근접 이웃
//! let nearest = tree.nearest(&Point2D::new(6.0, 6.0), 1);
//! ```

pub mod manager;
pub mod node;
pub mod rtree;
pub mod rtree3d;
pub mod types;

// 2D 타입
pub use node::{MAX_ENTRIES, MIN_ENTRIES, RTreeEntry, RTreeNode};
pub use rtree::RTree;
pub use types::{BoundingBox, Point2D};

// 3D 타입
pub use rtree3d::{MAX_ENTRIES_3D, MIN_ENTRIES_3D, RTree3D, RTree3DEntry, RTree3DNode};
pub use types::{BoundingBox3D, Point3D};

pub use manager::SpatialIndexManager;
