//! Admin Service Client
//!
//! 데이터베이스 관리 서비스 클라이언트 (Base+)

use super::config::GrpcClientError;

define_grpc_client!(
    /// Admin 서비스 클라이언트
    AdminClient,
    crate::admin::admin_service_client::AdminServiceClient<super::AuthenticatedChannel>
);

impl AdminClient {
    impl_empty_rpc_field!(
        /// 데이터베이스 목록 조회
        list_databases -> Vec<crate::admin::DatabaseInfo>,
        list_databases,
        crate::admin::Empty,
        databases
    );

    impl_empty_rpc!(
        /// 시스템 상태 조회
        get_system_status -> crate::admin::SystemStatusResponse,
        get_system_status,
        crate::admin::Empty
    );

    impl_empty_rpc_field!(
        /// API key 목록 조회
        list_api_keys -> Vec<crate::admin::ApiKeyInfo>,
        list_api_keys,
        crate::admin::Empty,
        api_keys
    );

    impl_empty_rpc_field!(
        /// Credential session 목록 조회
        list_credential_sessions -> Vec<crate::admin::CredentialSessionInfo>,
        list_credential_sessions,
        crate::admin::Empty,
        sessions
    );

    impl_empty_rpc_field!(
        /// Credential signing key 목록 조회
        list_signing_keys -> Vec<crate::admin::SigningKeyInfo>,
        list_signing_keys,
        crate::admin::Empty,
        signing_keys
    );

    impl_empty_rpc!(
        /// 버전 조회
        get_version -> crate::admin::VersionResponse,
        get_version,
        crate::admin::Empty
    );

    impl_empty_rpc_field!(
        /// 사용자 목록 조회
        list_users -> Vec<crate::admin::UserInfo>,
        list_users,
        crate::admin::Empty,
        users
    );

    impl_empty_rpc!(
        /// 설정 조회
        get_config -> crate::admin::ConfigResponse,
        get_config,
        crate::admin::Empty
    );

    /// 데이터베이스 생성
    pub async fn create_database(
        &mut self,
        name: &str,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let request = crate::admin::CreateDatabaseRequest {
            name: name.to_string(),
            description: None,
        };
        let response = self.client.create_database(request).await?;
        Ok(response.into_inner())
    }

    /// 데이터베이스 삭제
    pub async fn drop_database(
        &mut self,
        name: &str,
        force: bool,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let request = crate::admin::DropDatabaseRequest {
            name: name.to_string(),
            force,
        };
        let response = self.client.drop_database(request).await?;
        Ok(response.into_inner())
    }

    pub async fn compromise_signing_key(
        &mut self,
        version: u64,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let response = self
            .client
            .compromise_signing_key(crate::admin::CompromiseSigningKeyRequest { version })
            .await?;
        Ok(response.into_inner())
    }

    /// 데이터베이스 상태 조회
    pub async fn get_database_status(
        &mut self,
        name: &str,
    ) -> Result<crate::admin::DatabaseStatusResponse, GrpcClientError> {
        let request = crate::admin::DatabaseRequest {
            name: name.to_string(),
        };
        let response = self.client.get_database_status(request).await?;
        Ok(response.into_inner())
    }

    /// 사용자 생성
    pub async fn create_user(
        &mut self,
        username: &str,
        password: &str,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let request = crate::admin::CreateUserRequest {
            username: username.to_string(),
            password: password.to_string(),
        };
        let response = self.client.create_user(request).await?;
        Ok(response.into_inner())
    }

    /// 사용자 삭제
    pub async fn delete_user(
        &mut self,
        username: &str,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let request = crate::admin::DeleteUserRequest {
            username: username.to_string(),
        };
        let response = self.client.delete_user(request).await?;
        Ok(response.into_inner())
    }

    /// 초기 비밀번호 설정
    pub async fn set_initial_password(
        &mut self,
        username: &str,
        password: &str,
        require_change: bool,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let request = crate::admin::SetPasswordRequest {
            username: username.to_string(),
            password: password.to_string(),
            require_change,
        };
        let response = self.client.set_initial_password(request).await?;
        Ok(response.into_inner())
    }

    pub async fn create_api_key(
        &mut self,
        name: &str,
        audience: &str,
        scopes: Vec<String>,
        expires_at: i64,
    ) -> Result<crate::admin::IssuedApiKeyResponse, GrpcClientError> {
        let response = self
            .client
            .create_api_key(crate::admin::CreateApiKeyRequest {
                name: name.to_string(),
                audience: audience.to_string(),
                scopes,
                expires_at,
            })
            .await?;
        Ok(response.into_inner())
    }

    pub async fn rotate_api_key(
        &mut self,
        id: &str,
        overlap_until: i64,
        expires_at: i64,
    ) -> Result<crate::admin::IssuedApiKeyResponse, GrpcClientError> {
        let response = self
            .client
            .rotate_api_key(crate::admin::RotateApiKeyRequest {
                id: id.to_string(),
                overlap_until,
                expires_at,
            })
            .await?;
        Ok(response.into_inner())
    }

    pub async fn revoke_api_key(
        &mut self,
        id: &str,
    ) -> Result<crate::admin::ApiKeyInfo, GrpcClientError> {
        let response = self
            .client
            .revoke_api_key(crate::admin::RevokeApiKeyRequest { id: id.to_string() })
            .await?;
        Ok(response.into_inner())
    }

    pub async fn revoke_credential_session(
        &mut self,
        id: &str,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let response = self
            .client
            .revoke_credential_session(crate::admin::RevokeCredentialSessionRequest {
                id: id.to_string(),
            })
            .await?;
        Ok(response.into_inner())
    }

    /// 데이터베이스 시작
    pub async fn start_database(
        &mut self,
        name: &str,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let request = crate::admin::DatabaseRequest {
            name: name.to_string(),
        };
        let response = self.client.start_database(request).await?;
        Ok(response.into_inner())
    }

    /// 데이터베이스 중지
    pub async fn stop_database(
        &mut self,
        name: &str,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let request = crate::admin::DatabaseRequest {
            name: name.to_string(),
        };
        let response = self.client.stop_database(request).await?;
        Ok(response.into_inner())
    }

    /// 보호 백업 생성 (스트리밍)
    pub async fn backup(
        &mut self,
        database: &str,
    ) -> Result<tonic::Streaming<crate::admin::BackupProgress>, GrpcClientError> {
        let request = crate::admin::BackupRequest {
            database: database.to_string(),
        };
        let response = self.client.backup(request).await?;
        Ok(response.into_inner())
    }

    /// 보호 백업 복원 (스트리밍)
    pub async fn restore(
        &mut self,
        chunks: impl futures::Stream<Item = crate::admin::RestoreChunk> + Send + 'static,
    ) -> Result<crate::admin::RestoreResponse, GrpcClientError> {
        let response = self.client.restore(chunks).await?;
        Ok(response.into_inner())
    }

    /// 데이터 익스포트 (스트리밍)
    pub async fn export(
        &mut self,
        format: &str,
        data_type: &str,
        label: Option<&str>,
        delimiter: Option<&str>,
    ) -> Result<tonic::Streaming<crate::admin::ExportChunk>, GrpcClientError> {
        let request = crate::admin::ExportRequest {
            format: format.to_string(),
            data_type: data_type.to_string(),
            label: label.map(|s| s.to_string()),
            delimiter: delimiter.map(|s| s.to_string()),
        };
        let response = self.client.export(request).await?;
        Ok(response.into_inner())
    }

    /// 데이터 임포트 (스트리밍)
    pub async fn import(
        &mut self,
        chunks: impl futures::Stream<Item = crate::admin::ImportChunk> + Send + 'static,
    ) -> Result<crate::admin::ImportResponse, GrpcClientError> {
        let response = self.client.import(chunks).await?;
        Ok(response.into_inner())
    }

    /// 풀텍스트 인덱스 삭제
    pub async fn drop_fulltext_index(
        &mut self,
        database: &str,
        name: &str,
    ) -> Result<crate::admin::StatusResponse, GrpcClientError> {
        let request = crate::admin::DropIndexRequest {
            name: name.to_string(),
            database: database.to_string(),
        };
        let response = self.client.drop_fulltext_index(request).await?;
        Ok(response.into_inner())
    }
}
