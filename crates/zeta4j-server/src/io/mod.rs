//! 데이터 Import/Export 모듈
//!
//! v0.39.0: JSON/CSV 포맷 지원
//! v0.51.0: GraphML 포맷 지원
//! v0.53.0: 배치 로딩 지원
//! v1.42.0: 대용량 파일 스트리밍 지원 (NDJSON, 진행 콜백)
//! v1.56.0: GEXF 포맷 지원
//! v1.57.0: gzip/lz4 압축 지원
//!
//! # 사용 예시
//!
//! ```rust,ignore
//! use zeta4j_server::io::{JsonExporter, JsonImporter, CsvExporter, CsvImporter};
//! use zeta4j_server::io::{GraphMLExporter, GraphMLImporter};
//! use zeta4j_server::io::{BatchBuilder, BatchLoader, BatchOptions, BatchStats};
//! use zeta4j_server::io::streaming::{StreamingCsvExporter, NdjsonExporter, NdjsonImporter, StreamingOptions};
//!
//! // JSON으로 노드 내보내기
//! JsonExporter::export_nodes(&engine, Some("Person"), "nodes.json")?;
//!
//! // JSON에서 노드 가져오기
//! JsonImporter::import_nodes(&engine, "nodes.json")?;
//!
//! // CSV로 내보내기
//! CsvExporter::export_nodes(&engine, Some("Person"), "nodes.csv", CsvOptions::default())?;
//!
//! // GraphML로 내보내기
//! GraphMLExporter::export_graph(&engine, "graph.graphml")?;
//!
//! // GraphML에서 가져오기
//! GraphMLImporter::import_graph(&engine, "graph.graphml")?;
//!
//! // 배치 로딩 (스트리밍)
//! let mut batch = BatchBuilder::new(&engine);
//! batch.add_node(vec!["Person".into()], props);
//! batch.finish()?;
//!
//! // 배치 로딩 (파일)
//! let loader = BatchLoader::new(&engine);
//! loader.load_nodes_from_csv("nodes.csv")?;
//!
//! // 스트리밍 CSV Export (메모리 효율적)
//! let options = StreamingOptions::new()
//!     .with_progress_callback(|processed, total| println!("Progress: {}/{:?}", processed, total));
//! StreamingCsvExporter::export_nodes(&engine, None, "nodes.csv", options)?;
//!
//! // NDJSON (Newline Delimited JSON) Import/Export
//! NdjsonExporter::export_nodes(&engine, None, "nodes.ndjson", StreamingOptions::default())?;
//! NdjsonImporter::import_nodes(&engine, "nodes.ndjson", StreamingOptions::default())?;
//! ```

pub mod api_error;
pub mod batch;
pub mod compress;
pub mod csv;
pub mod format;
pub mod gexf;
pub mod graphml;
pub mod json;
pub mod state;
pub mod streaming;

pub use api_error::RouterApiError;
pub use batch::{
    BatchBuilder,
    BatchLoader,
    BatchOptions,
    BatchStats,
    PendingEdge,
    PendingNode,
    load_edges_from_csv_async,
    // Async batch loading (v1.78.0)
    load_nodes_from_csv_async,
};
pub use compress::{
    CompressedReader, CompressedWriter, CompressionFormat, CompressionOptions, CompressionStats,
    GzipLevel, Lz4Level, compress_data, compress_file, decompress_data, decompress_file,
};
pub use csv::{CsvExporter, CsvImporter, CsvOptions};
pub use format::ValueConverter;
pub use gexf::{
    GexfAttribute, GexfExportStats, GexfExporter, GexfImportStats, GexfImporter, GexfType,
};
pub use graphml::{
    ExportStats, GraphMLExporter, GraphMLImporter, GraphMLKey, GraphMLType, ImportStats, KeyTarget,
};
pub use json::{EdgeJson, GraphJson, GraphMetadata, JsonExporter, JsonImporter, NodeJson};
pub use state::{StateManager, load_json, load_json_or_default, save_json};
pub use streaming::{
    NdjsonEdge, NdjsonExporter, NdjsonImporter, NdjsonNode, ProgressCallback, StreamingCsvExporter,
    StreamingEdgeIterator, StreamingNodeIterator, StreamingOptions,
};
