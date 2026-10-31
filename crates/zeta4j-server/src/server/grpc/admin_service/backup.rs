// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Binary backup and atomic logical restore.

use std::pin::Pin;

use serde::{Deserialize, Serialize};
use tokio_stream::Stream;
use tonic::{Request, Response, Status, Streaming};

use zeta4j_cli_grpc::admin::{BackupProgress, BackupRequest, RestoreChunk, RestoreResponse};

use crate::server::tenant::TenantDatabaseSnapshot;

use super::{AdminAppState, AdminGrpcService};

const BACKUP_FORMAT_VERSION: u16 = 1;
const MAX_BACKUP_BYTES: usize = 1024 * 1024 * 1024;

/// Backup stream returned by the generated admin service.
pub type BackupStream = Pin<Box<dyn Stream<Item = Result<BackupProgress, Status>> + Send>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupBundle {
    version: u16,
    backup_id: String,
    database: String,
    generation: u64,
    source_edition: String,
    snapshot: TenantDatabaseSnapshot,
}

impl AdminGrpcService {
    /// Creates one binary backup of a logical database.
    pub async fn backup_impl(
        &self,
        request: Request<BackupRequest>,
    ) -> Result<Response<BackupStream>, Status> {
        let database = normalized_database(request.into_inner().database);
        let manager = self
            .state
            .db_manager()
            .ok_or_else(|| Status::unavailable("database manager is not available"))?;
        let engine = manager
            .get_engine(&database)
            .await
            .ok_or_else(|| Status::not_found("backup database was not found"))?;
        let total_nodes = engine
            .node_count()
            .await
            .map_err(|_| Status::internal("backup node count failed"))?
            as u64;
        let total_edges = engine.edge_count().await as u64;
        let bundle = create_bundle(&self.state, &database).await?;
        let job_id = uuid::Uuid::now_v7().to_string();
        let messages = vec![
            Ok(BackupProgress {
                job_id: job_id.clone(),
                status: "in_progress".to_string(),
                progress_percent: 0.0,
                nodes_dumped: 0,
                edges_dumped: 0,
                total_nodes,
                total_edges,
                data_chunk: None,
                message: Some("Creating backup".to_string()),
            }),
            Ok(BackupProgress {
                job_id,
                status: zeta4j_common::STATUS_COMPLETED.to_string(),
                progress_percent: 100.0,
                nodes_dumped: total_nodes,
                edges_dumped: total_edges,
                total_nodes,
                total_edges,
                data_chunk: Some(bundle),
                message: Some("Backup completed".to_string()),
            }),
        ];
        Ok(Response::new(Box::pin(tokio_stream::iter(messages))))
    }

    /// Validates the complete backup before applying one atomic canonical
    /// database replacement.
    pub async fn restore_impl(
        &self,
        request: Request<Streaming<RestoreChunk>>,
    ) -> Result<Response<RestoreResponse>, Status> {
        let generation_guard = request
            .extensions()
            .get::<crate::server::grpc::SecurityGenerationGuard>()
            .cloned()
            .ok_or_else(|| {
                crate::server::grpc::access_status(
                    crate::server::access::PublicError::PolicyInvariant,
                )
            })?;
        generation_guard
            .validate()
            .map_err(crate::server::grpc::access_status)?;
        let mut stream = request.into_inner();
        let start = std::time::Instant::now();
        let mut bundle = Vec::new();
        let mut database = String::new();
        let mut terminal = false;

        while let Some(chunk) = stream.message().await? {
            generation_guard
                .validate()
                .map_err(crate::server::grpc::access_status)?;
            let chunk_database = normalized_database(chunk.database);
            if database.is_empty() {
                database = chunk_database;
            } else if database != chunk_database {
                return Err(Status::invalid_argument(
                    "restore database changed between stream chunks",
                ));
            }
            if terminal {
                return Err(Status::invalid_argument(
                    "restore stream continued after its terminal chunk",
                ));
            }
            let new_len = bundle
                .len()
                .checked_add(chunk.data.len())
                .ok_or_else(|| Status::resource_exhausted("backup is too large"))?;
            if new_len > MAX_BACKUP_BYTES {
                return Err(Status::resource_exhausted("backup is too large"));
            }
            bundle.extend_from_slice(&chunk.data);
            terminal = chunk.is_last;
        }
        generation_guard
            .validate()
            .map_err(crate::server::grpc::access_status)?;
        if !terminal || bundle.is_empty() {
            return Err(Status::invalid_argument(
                "restore stream is missing a terminal backup",
            ));
        }

        // Every manifest and canonical snapshot invariant is checked
        // before a local write transaction opens.
        validate_bundle(&self.state, &database, &bundle).await?;
        apply_restore_bundle(&self.state, &database, &bundle).await?;
        let manager = self
            .state
            .db_manager()
            .ok_or_else(|| Status::unavailable("database manager is not available"))?;
        let engine = manager
            .get_engine(&database)
            .await
            .ok_or_else(|| Status::not_found("restore database was not found"))?;
        let nodes_loaded = engine
            .node_count()
            .await
            .map_err(|_| Status::internal("restored node count failed"))?
            as u64;
        let edges_loaded = engine.edge_count().await as u64;
        Ok(Response::new(RestoreResponse {
            success: true,
            message: format!("Restore completed: {nodes_loaded} nodes, {edges_loaded} edges"),
            nodes_loaded,
            edges_loaded,
            duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
        }))
    }
}

async fn create_bundle(state: &AdminAppState, database: &str) -> Result<Vec<u8>, Status> {
    let manager = state
        .db_manager()
        .ok_or_else(|| Status::unavailable("database manager is not available"))?;
    let snapshot = manager
        .snapshot_database(database)
        .await
        .map_err(|_| Status::internal("canonical backup snapshot failed"))?;
    let generation = u64::try_from(chrono::Utc::now().timestamp_millis())
        .map_err(|_| Status::internal("backup generation is invalid"))?;
    let bundle = BackupBundle {
        version: BACKUP_FORMAT_VERSION,
        backup_id: uuid::Uuid::now_v7().to_string(),
        database: database.to_string(),
        generation,
        source_edition: state.edition().as_str().to_string(),
        snapshot,
    };
    zeta4j_codec::serialize_bounded::<MAX_BACKUP_BYTES, _>(&bundle)
        .map_err(|_| Status::internal("backup bundle serialization failed"))
}

async fn validate_bundle(
    state: &AdminAppState,
    expected_database: &str,
    bytes: &[u8],
) -> Result<BackupBundle, Status> {
    let bundle = decode_backup_bundle(bytes)?;
    if bundle.version != BACKUP_FORMAT_VERSION
        || bundle.database != expected_database
        || bundle.source_edition != state.edition().as_str()
        || bundle.snapshot.name != expected_database
    {
        return Err(Status::failed_precondition(
            "backup manifest is incompatible with this restore target",
        ));
    }
    Ok(bundle)
}

fn decode_backup_bundle(bytes: &[u8]) -> Result<BackupBundle, Status> {
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(Status::resource_exhausted("backup is too large"));
    }
    zeta4j_codec::deserialize_bounded::<MAX_BACKUP_BYTES, _>(bytes)
        .map_err(|_| Status::invalid_argument("backup format is invalid"))
}

pub(super) async fn apply_restore_bundle(
    state: &AdminAppState,
    database: &str,
    bytes: &[u8],
) -> Result<(), Status> {
    let bundle = validate_bundle(state, database, bytes).await?;
    state
        .db_manager()
        .ok_or_else(|| Status::unavailable("database manager is not available"))?
        .restore_database(database, &bundle.snapshot)
        .await
        .map_err(|_| Status::failed_precondition("restore was rejected before publish"))
}

fn normalized_database(database: String) -> String {
    if database.is_empty() {
        "zeta4j".to_string()
    } else {
        database
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malicious_backup_length_is_rejected_by_bounded_decode() {
        let mut malicious = BACKUP_FORMAT_VERSION.to_le_bytes().to_vec();
        malicious.extend_from_slice(&u64::MAX.to_le_bytes());

        let error = decode_backup_bundle(&malicious).err();
        assert!(error.is_some_and(|status| status.code() == tonic::Code::InvalidArgument));
    }

    #[test]
    fn backup_bundle_rejects_trailing_content() {
        let mut bytes = BACKUP_FORMAT_VERSION.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 1, 2, 3]);
        bytes.push(0);

        assert!(decode_backup_bundle(&bytes).is_err());
    }
}
