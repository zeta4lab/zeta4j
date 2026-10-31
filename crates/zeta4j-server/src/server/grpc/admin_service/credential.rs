// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! Credential lifecycle administration.

use std::collections::BTreeSet;

use tonic::{Request, Response, Status};
use zeta4j_cli_grpc::admin::{
    ApiKeyInfo, ApiKeyListResponse, CompromiseSigningKeyRequest, CreateApiKeyRequest,
    CredentialSessionInfo, CredentialSessionListResponse, Empty, IssuedApiKeyResponse,
    RevokeApiKeyRequest, RevokeCredentialSessionRequest, RotateApiKeyRequest, SigningKeyInfo,
    SigningKeyListResponse, StatusResponse,
};
use zeta4j_credential::{
    CredentialAudience, CredentialId, CredentialKind, CredentialScope, CredentialStatus,
    KeyVersion, SessionId, SessionStatus, SigningKeyStatus,
};

use super::AdminGrpcService;
use crate::server::system::{ApiKeyDescriptor, SecurityMutation, SessionDescriptor};

impl AdminGrpcService {
    pub async fn list_api_keys_impl(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<ApiKeyListResponse>, Status> {
        let api_keys = self
            .credential_security()?
            .list_api_keys()
            .map_err(credential_status)?
            .into_iter()
            .map(api_key_info)
            .collect();
        Ok(Response::new(ApiKeyListResponse { api_keys }))
    }

    pub async fn create_api_key_impl(
        &self,
        request: Request<CreateApiKeyRequest>,
    ) -> Result<Response<IssuedApiKeyResponse>, Status> {
        let request = request.into_inner();
        let audience = parse_audience(&request.audience)?;
        let scopes = parse_scopes(&request.scopes)?;
        let prepared = self
            .credential_security()?
            .prepare_api_key(&request.name, audience, scopes, request.expires_at)
            .map_err(credential_status)?;
        let (issued, principal, record, event) = prepared.into_parts();
        self.apply_security_mutation(SecurityMutation::InstallApiKey {
            principal,
            record,
            event,
        })?;
        let secret = issued.secret().expose().to_string();
        Ok(Response::new(IssuedApiKeyResponse {
            api_key: Some(api_key_info(issued.descriptor)),
            secret,
        }))
    }

    pub async fn rotate_api_key_impl(
        &self,
        request: Request<RotateApiKeyRequest>,
    ) -> Result<Response<IssuedApiKeyResponse>, Status> {
        let request = request.into_inner();
        let id = CredentialId::parse(&request.id).map_err(credential_status)?;
        let prepared = self
            .credential_security()?
            .prepare_api_key_rotation(&id, request.overlap_until, request.expires_at)
            .map_err(credential_status)?;
        let (issued, predecessor_id, successor, overlap_until, occurred_at, event) =
            prepared.into_parts();
        self.apply_security_mutation(SecurityMutation::RotateApiKey {
            predecessor_id,
            successor,
            overlap_until,
            occurred_at,
            event,
        })?;
        let secret = issued.secret().expose().to_string();
        Ok(Response::new(IssuedApiKeyResponse {
            api_key: Some(api_key_info(issued.descriptor)),
            secret,
        }))
    }

    pub async fn revoke_api_key_impl(
        &self,
        request: Request<RevokeApiKeyRequest>,
    ) -> Result<Response<ApiKeyInfo>, Status> {
        let id = CredentialId::parse(&request.into_inner().id).map_err(credential_status)?;
        let (occurred_at, event) = self
            .credential_security()?
            .prepare_api_key_revocation(&id)
            .map_err(credential_status)?;
        self.apply_security_mutation(SecurityMutation::RevokeApiKey {
            id: id.clone(),
            occurred_at,
            event,
        })?;
        let descriptor = self
            .credential_security()?
            .list_api_keys()
            .map_err(credential_status)?
            .into_iter()
            .find(|descriptor| descriptor.id == id)
            .ok_or_else(|| Status::internal("committed API key is missing"))?;
        Ok(Response::new(api_key_info(descriptor)))
    }

    pub async fn list_credential_sessions_impl(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<CredentialSessionListResponse>, Status> {
        let sessions = self
            .credential_security()?
            .list_all_sessions()
            .map_err(credential_status)?
            .into_iter()
            .map(session_info)
            .collect();
        Ok(Response::new(CredentialSessionListResponse { sessions }))
    }

    pub async fn revoke_credential_session_impl(
        &self,
        request: Request<RevokeCredentialSessionRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let id = SessionId::parse(&request.into_inner().id).map_err(credential_status)?;
        let (subject, reason, occurred_at, event) = self
            .credential_security()?
            .prepare_session_revocation(&id)
            .map_err(credential_status)?;
        self.apply_security_mutation(SecurityMutation::CloseSession {
            id,
            subject,
            reason,
            occurred_at,
            event,
        })?;
        Ok(Response::new(StatusResponse {
            success: true,
            message: "credential session revoked".to_string(),
        }))
    }

    pub async fn list_signing_keys_impl(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<SigningKeyListResponse>, Status> {
        let signing_keys = self
            .credential_security()?
            .signing_keys()
            .map_err(credential_status)?
            .into_iter()
            .map(signing_key_info)
            .collect();
        Ok(Response::new(SigningKeyListResponse { signing_keys }))
    }

    pub async fn compromise_signing_key_impl(
        &self,
        request: Request<CompromiseSigningKeyRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let version = KeyVersion::new(request.into_inner().version).map_err(credential_status)?;
        let mutation = self
            .credential_security()?
            .prepare_signing_key_compromise(version);
        self.apply_security_mutation(mutation)?;
        self.credential_security()?
            .synchronize_token_verifier(self.credential_tokens()?)
            .map_err(credential_status)?;
        Ok(Response::new(StatusResponse {
            success: true,
            message: "signing key compromised and dependent sessions revoked".to_string(),
        }))
    }
}

fn parse_audience(value: &str) -> Result<CredentialAudience, Status> {
    match value {
        "http" => Ok(CredentialAudience::Http),
        _ => Err(Status::invalid_argument(
            "API keys currently support the exact 'http' audience",
        )),
    }
}

fn parse_scopes(values: &[String]) -> Result<BTreeSet<CredentialScope>, Status> {
    if values.is_empty() {
        return Err(Status::invalid_argument(
            "at least one API key scope is required",
        ));
    }
    values
        .iter()
        .map(|value| match value.as_str() {
            "api" => Ok(CredentialScope::Api),
            "administration" => Ok(CredentialScope::Administration),
            _ => Err(Status::invalid_argument(
                "API key scope must be 'api' or 'administration'",
            )),
        })
        .collect()
}

fn api_key_info(descriptor: ApiKeyDescriptor) -> ApiKeyInfo {
    ApiKeyInfo {
        id: descriptor.id.as_str().to_string(),
        subject_id: descriptor.subject.as_str().to_string(),
        name: descriptor.name,
        audience: audience_name(descriptor.audience).to_string(),
        scopes: descriptor
            .scope
            .into_iter()
            .map(|scope| scope_name(scope).to_string())
            .collect(),
        created_at: descriptor.created_at,
        not_before: descriptor.not_before,
        expires_at: descriptor.expires_at,
        version: descriptor.version,
        status: credential_status_name(descriptor.status).to_string(),
        overlap_until: descriptor.overlap_until,
    }
}

fn session_info(descriptor: SessionDescriptor) -> CredentialSessionInfo {
    CredentialSessionInfo {
        id: descriptor.id.as_str().to_string(),
        subject_id: descriptor.subject.as_str().to_string(),
        credential_kind: credential_kind_name(descriptor.credential_kind).to_string(),
        created_at: descriptor.created_at,
        last_refreshed_at: descriptor.last_refreshed_at,
        absolute_expires_at: descriptor.absolute_expires_at,
        idle_expires_at: descriptor.idle_expires_at,
        security_generation: descriptor.security_generation,
        key_version: descriptor.key_version.get(),
        family_id: descriptor.family_id.as_str().to_string(),
        status: session_status_name(descriptor.status).to_string(),
    }
}

fn signing_key_info(key: zeta4j_credential::SigningVerificationKey) -> SigningKeyInfo {
    SigningKeyInfo {
        version: key.version.get(),
        key_id: key.key_id,
        status: signing_key_status_name(key.status).to_string(),
        prepared_at: key.prepared_at,
        activated_at: key.activated_at,
        verify_until: key.verify_until,
    }
}

const fn audience_name(audience: CredentialAudience) -> &'static str {
    match audience {
        CredentialAudience::Http => "http",
        CredentialAudience::Bolt => "bolt",
        CredentialAudience::Grpc => "grpc",
        CredentialAudience::Cli => "cli",
    }
}

const fn scope_name(scope: CredentialScope) -> &'static str {
    match scope {
        CredentialScope::Access => "access",
        CredentialScope::Refresh => "refresh",
        CredentialScope::Api => "api",
        CredentialScope::Administration => "administration",
    }
}

const fn credential_status_name(status: CredentialStatus) -> &'static str {
    match status {
        CredentialStatus::Active => "active",
        CredentialStatus::Suspended => "suspended",
        CredentialStatus::Rotating => "rotating",
        CredentialStatus::Revoked => "revoked",
        CredentialStatus::Expired => "expired",
    }
}

const fn credential_kind_name(kind: CredentialKind) -> &'static str {
    match kind {
        CredentialKind::Password => "password",
        CredentialKind::AccessToken => "access_token",
        CredentialKind::RefreshToken => "refresh_token",
        CredentialKind::ApiKey => "api_key",
    }
}

const fn session_status_name(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "active",
        SessionStatus::LoggedOut => "logged_out",
        SessionStatus::FamilyReplay => "family_replay",
        SessionStatus::PrincipalChanged => "principal_changed",
        SessionStatus::Expired => "expired",
        SessionStatus::Aborted => "aborted",
        SessionStatus::Evicted => "evicted",
    }
}

const fn signing_key_status_name(status: SigningKeyStatus) -> &'static str {
    match status {
        SigningKeyStatus::Prepared => "prepared",
        SigningKeyStatus::Active => "active",
        SigningKeyStatus::VerifyOnly => "verify_only",
        SigningKeyStatus::Retired => "retired",
        SigningKeyStatus::Compromised => "compromised",
    }
}

fn credential_status(error: zeta4j_credential::CredentialFailure) -> Status {
    match error {
        zeta4j_credential::CredentialFailure::Internal => Status::internal(error.to_string()),
        zeta4j_credential::CredentialFailure::Throttled => {
            Status::resource_exhausted(error.to_string())
        }
        _ => Status::invalid_argument(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ring::{rand::SystemRandom, signature::Ed25519KeyPair};
    use tonic::Request;
    use zeta4j_cli_grpc::admin::{
        CreateApiKeyRequest, Empty, RevokeApiKeyRequest, RotateApiKeyRequest,
    };
    use zeta4j_credential::KeyVersion;

    use super::*;
    use crate::server::{
        auth::SignerReference,
        system::{
            CredentialRuntimePolicy, SystemDatabaseService, initialize_local_credential_runtime,
        },
    };

    #[tokio::test]
    async fn api_key_admin_returns_secret_once_and_persists_only_verifier()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let signer_path = directory.path().join("signer.pk8");
        let signer_bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| std::io::Error::other("failed to generate test signing key"))?;
        std::fs::write(&signer_path, signer_bytes.as_ref())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&signer_path, std::fs::Permissions::from_mode(0o600))?;
        }
        let system = Arc::new(SystemDatabaseService::open(
            directory.path().join("system"),
        )?);
        let signer = SignerReference::new(&signer_path, "test-key", KeyVersion::new(1)?)?;
        let (credentials, tokens) = initialize_local_credential_runtime(
            system.security_arc(),
            &signer,
            CredentialRuntimePolicy::default(),
            "test-node",
        )?;
        let service = AdminGrpcService::from_system_service(system)
            .with_credentials(credentials.clone(), tokens);
        let now = chrono::Utc::now().timestamp();
        let issued = service
            .create_api_key_impl(Request::new(CreateApiKeyRequest {
                name: "test-service".to_string(),
                audience: "http".to_string(),
                scopes: vec!["api".to_string()],
                expires_at: now + 3_600,
            }))
            .await?
            .into_inner();
        let metadata = issued
            .api_key
            .ok_or_else(|| std::io::Error::other("API key metadata missing"))?;
        assert!(issued.secret.starts_with("z4g_"));
        let stored = credentials.list_api_keys()?;
        assert_eq!(stored.len(), 1);
        assert!(!serde_json::to_string(&stored)?.contains(&issued.secret));

        let rotated = service
            .rotate_api_key_impl(Request::new(RotateApiKeyRequest {
                id: metadata.id.clone(),
                overlap_until: now + 60,
                expires_at: now + 7_200,
            }))
            .await?
            .into_inner();
        assert_ne!(rotated.secret, issued.secret);
        service
            .revoke_api_key_impl(Request::new(RevokeApiKeyRequest {
                id: rotated
                    .api_key
                    .ok_or_else(|| std::io::Error::other("rotated metadata missing"))?
                    .id,
            }))
            .await?;
        let listed = service
            .list_api_keys_impl(Request::new(Empty {}))
            .await?
            .into_inner();
        assert_eq!(listed.api_keys.len(), 2);
        assert!(listed.api_keys.iter().any(|key| key.status == "revoked"));
        Ok(())
    }
}
