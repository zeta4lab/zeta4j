//! User Management 메서드들.

use chrono::Utc;
use tonic::{Request, Response, Status};

use zeta4j_cli_grpc::admin::{
    CreateUserRequest, DeleteUserRequest, Empty, SetPasswordRequest, StatusResponse, UserInfo,
    UserListResponse,
};

use super::AdminGrpcService;
use crate::server::auth::hash_password;
use crate::server::system::{SecurityMutation, User};

impl AdminGrpcService {
    /// 사용자 목록 조회
    pub async fn list_users_impl(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<UserListResponse>, Status> {
        let security = self.state.system_service().security();

        let users = security
            .list_users()
            .map_err(|e| Status::internal(e.to_string()))?
            .into_iter()
            .map(|u| UserInfo {
                username: u.username,
                status: if u.suspended {
                    "disabled".to_string()
                } else {
                    "active".to_string()
                },
                created_at: u.created_at.to_rfc3339(),
                last_login: None,
            })
            .collect();

        Ok(Response::new(UserListResponse { users }))
    }

    /// 사용자 생성
    pub async fn create_user_impl(
        &self,
        request: Request<CreateUserRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();

        if req.username.is_empty() {
            return Err(Status::invalid_argument("Username cannot be empty"));
        }

        let security = self.state.system_service().security();
        match security.get_user(&req.username) {
            Ok(Some(_)) => {
                return Ok(Response::new(StatusResponse {
                    success: false,
                    message: format!(
                        "Failed to create user: User already exists: {}",
                        req.username
                    ),
                }));
            }
            Ok(None) => {}
            Err(error) => return Err(Status::internal(error.to_string())),
        }
        let password_hash = hash_password(&req.password)
            .map_err(|error| Status::internal(format!("Password hash error: {error}")))?;
        let user = User::new(&req.username, password_hash);
        self.apply_security_mutation(SecurityMutation::CreateUser { user })?;
        Ok(Response::new(StatusResponse {
            success: true,
            message: format!("User '{}' created", req.username),
        }))
    }

    /// 사용자 삭제
    pub async fn delete_user_impl(
        &self,
        request: Request<DeleteUserRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();

        let security = self.state.system_service().security();
        match security.get_user(&req.username) {
            Ok(None) => {
                return Ok(Response::new(StatusResponse {
                    success: false,
                    message: format!("Failed to delete user: User not found: {}", req.username),
                }));
            }
            Ok(Some(_)) => {}
            Err(error) => return Err(Status::internal(error.to_string())),
        }
        self.apply_security_mutation(SecurityMutation::DeleteUser {
            username: req.username.clone(),
            occurred_at: Utc::now(),
        })?;
        Ok(Response::new(StatusResponse {
            success: true,
            message: format!("User '{}' deleted", req.username),
        }))
    }

    /// 비밀번호 설정
    pub async fn set_initial_password_impl(
        &self,
        request: Request<SetPasswordRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let req = request.into_inner();

        let security = self.state.system_service().security();
        let Some(_) = security
            .get_user(&req.username)
            .map_err(|error| Status::internal(error.to_string()))?
        else {
            return Ok(Response::new(StatusResponse {
                success: false,
                message: format!("Failed to set password: User not found: {}", req.username),
            }));
        };
        let password_hash = hash_password(&req.password)
            .map_err(|error| Status::internal(format!("Password hash error: {error}")))?;
        self.apply_security_mutation(SecurityMutation::SetUserPassword {
            username: req.username.clone(),
            password_hash,
            require_change: req.require_change,
            updated_at: Utc::now(),
        })?;
        Ok(Response::new(StatusResponse {
            success: true,
            message: format!("Password set for user '{}'", req.username),
        }))
    }
}
