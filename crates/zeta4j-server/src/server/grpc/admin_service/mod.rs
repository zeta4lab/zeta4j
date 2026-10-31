// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Admin gRPC Service Implementation
//!
//! admin.proto 기반의 gRPC 서비스 구현

mod backup;
mod credential;
mod database;
mod helpers;
mod import_export;
mod state;
mod system;
mod user;

// Re-exports
pub use backup::BackupStream;
pub use import_export::ExportStream;
pub use state::{AdminAppState, AdminGrpcService};

use tonic::{Request, Response, Status, Streaming};

use zeta4j_cli_grpc::admin::{
    ApiKeyInfo,
    ApiKeyListResponse,
    // Backup/Restore
    BackupRequest,
    CompromiseSigningKeyRequest,
    ConfigResponse,
    CreateApiKeyRequest,
    CreateDatabaseRequest,
    CreateUserRequest,
    CredentialSessionListResponse,
    // Database
    DatabaseListResponse,
    DatabaseRequest,
    DatabaseStatusResponse,
    DeleteUserRequest,
    DropDatabaseRequest,
    DropIndexRequest,
    // Common
    Empty,
    ExportRequest,
    // Import/Export
    ImportChunk,
    ImportResponse,
    IssuedApiKeyResponse,
    RestoreChunk,
    RestoreResponse,
    RevokeApiKeyRequest,
    RevokeCredentialSessionRequest,
    RotateApiKeyRequest,
    SetPasswordRequest,
    SigningKeyListResponse,
    StatusResponse,
    // System
    SystemStatusResponse,
    // User
    UserListResponse,
    VersionResponse,
    admin_service_server::AdminService,
};

#[tonic::async_trait]
impl AdminService for AdminGrpcService {
    // =========================================================================
    // Database Management
    // =========================================================================

    /// 데이터베이스 목록 조회
    async fn list_databases(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<DatabaseListResponse>, Status> {
        self.list_databases_impl(request).await
    }

    /// 데이터베이스 생성
    async fn create_database(
        &self,
        request: Request<CreateDatabaseRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.create_database_impl(request).await
    }

    /// 데이터베이스 삭제
    async fn drop_database(
        &self,
        request: Request<DropDatabaseRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.drop_database_impl(request).await
    }

    /// 데이터베이스 상태 조회
    async fn get_database_status(
        &self,
        request: Request<DatabaseRequest>,
    ) -> Result<Response<DatabaseStatusResponse>, Status> {
        self.get_database_status_impl(request).await
    }

    /// 데이터베이스 시작
    async fn start_database(
        &self,
        request: Request<DatabaseRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.start_database_impl(request).await
    }

    /// 데이터베이스 중지
    async fn stop_database(
        &self,
        request: Request<DatabaseRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.stop_database_impl(request).await
    }

    /// 풀텍스트 인덱스 삭제
    async fn drop_fulltext_index(
        &self,
        request: Request<DropIndexRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.drop_fulltext_index_impl(request).await
    }

    // =========================================================================
    // Backup/Restore (Streaming)
    // =========================================================================

    type BackupStream = BackupStream;

    /// 보호 백업 생성 (스트리밍)
    async fn backup(
        &self,
        request: Request<BackupRequest>,
    ) -> Result<Response<Self::BackupStream>, Status> {
        self.backup_impl(request).await
    }

    /// 보호 백업 복원 (클라이언트 스트리밍)
    async fn restore(
        &self,
        request: Request<Streaming<RestoreChunk>>,
    ) -> Result<Response<RestoreResponse>, Status> {
        self.restore_impl(request).await
    }

    // =========================================================================
    // User Management
    // =========================================================================

    /// 사용자 목록 조회
    async fn list_users(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<UserListResponse>, Status> {
        self.list_users_impl(request).await
    }

    /// 사용자 생성
    async fn create_user(
        &self,
        request: Request<CreateUserRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.create_user_impl(request).await
    }

    /// 사용자 삭제
    async fn delete_user(
        &self,
        request: Request<DeleteUserRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.delete_user_impl(request).await
    }

    /// 비밀번호 설정
    async fn set_initial_password(
        &self,
        request: Request<SetPasswordRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.set_initial_password_impl(request).await
    }

    async fn list_api_keys(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<ApiKeyListResponse>, Status> {
        self.list_api_keys_impl(request).await
    }

    async fn create_api_key(
        &self,
        request: Request<CreateApiKeyRequest>,
    ) -> Result<Response<IssuedApiKeyResponse>, Status> {
        self.create_api_key_impl(request).await
    }

    async fn rotate_api_key(
        &self,
        request: Request<RotateApiKeyRequest>,
    ) -> Result<Response<IssuedApiKeyResponse>, Status> {
        self.rotate_api_key_impl(request).await
    }

    async fn revoke_api_key(
        &self,
        request: Request<RevokeApiKeyRequest>,
    ) -> Result<Response<ApiKeyInfo>, Status> {
        self.revoke_api_key_impl(request).await
    }

    async fn list_credential_sessions(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<CredentialSessionListResponse>, Status> {
        self.list_credential_sessions_impl(request).await
    }

    async fn revoke_credential_session(
        &self,
        request: Request<RevokeCredentialSessionRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.revoke_credential_session_impl(request).await
    }

    async fn list_signing_keys(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<SigningKeyListResponse>, Status> {
        self.list_signing_keys_impl(request).await
    }

    async fn compromise_signing_key(
        &self,
        request: Request<CompromiseSigningKeyRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        self.compromise_signing_key_impl(request).await
    }

    // =========================================================================
    // System
    // =========================================================================

    /// 시스템 상태 조회
    async fn get_system_status(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<SystemStatusResponse>, Status> {
        self.get_system_status_impl(request).await
    }

    /// 설정 조회
    async fn get_config(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<ConfigResponse>, Status> {
        self.get_config_impl(request).await
    }

    /// 버전 정보 조회
    async fn get_version(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<VersionResponse>, Status> {
        self.get_version_impl(request).await
    }

    // =========================================================================
    // Import/Export (Streaming)
    // =========================================================================

    /// 데이터 가져오기 (클라이언트 스트리밍)
    async fn import(
        &self,
        request: Request<Streaming<ImportChunk>>,
    ) -> Result<Response<ImportResponse>, Status> {
        self.import_impl(request).await
    }

    type ExportStream = ExportStream;

    /// 데이터 내보내기 (서버 스트리밍)
    async fn export(
        &self,
        request: Request<ExportRequest>,
    ) -> Result<Response<Self::ExportStream>, Status> {
        self.export_impl(request).await
    }
}
