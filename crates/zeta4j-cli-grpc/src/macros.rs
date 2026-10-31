// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

//! gRPC 클라이언트 매크로
//!
//! 반복적인 클라이언트 코드를 줄이기 위한 매크로 모음

/// gRPC 클라이언트 구조체 및 connect 메서드 생성 매크로
///
/// # 사용법
/// ```ignore
/// define_grpc_client!(
///     /// Admin 서비스 클라이언트
///     AdminClient,
///     crate::admin::admin_service_client::AdminServiceClient<tonic::transport::Channel>
/// );
/// ```
#[macro_export]
macro_rules! define_grpc_client {
    (
        $(#[$meta:meta])*
        $client_name:ident,
        $service_type:ty
    ) => {
        $(#[$meta])*
        pub struct $client_name {
            client: $service_type,
        }

        impl $client_name {
            /// 새 클라이언트 생성
            pub async fn connect(config: &$crate::GrpcClientConfig) -> Result<Self, $crate::GrpcClientError> {
                let channel = config.connect().await?;
                let channel = tonic::service::interceptor::InterceptedService::new(
                    channel,
                    $crate::client::BearerInterceptor::new(config.bearer_token.clone()),
                );
                let client = <$service_type>::new(channel);
                Ok(Self { client })
            }

            fn authenticated_request<T>(
                &self,
                message: T,
            ) -> Result<tonic::Request<T>, $crate::GrpcClientError> {
                Ok(tonic::Request::new(message))
            }
        }
    };
}

/// Empty 요청 → 응답 전체 반환 RPC 메서드 생성 매크로
#[macro_export]
macro_rules! impl_empty_rpc {
    (
        $(#[$meta:meta])*
        $fn_name:ident -> $response_type:ty,
        $rpc_method:ident,
        $empty_type:ty
    ) => {
        $(#[$meta])*
        pub async fn $fn_name(&mut self) -> Result<$response_type, $crate::GrpcClientError> {
            let request = self.authenticated_request(<$empty_type>::default())?;
            let response = self.client.$rpc_method(request).await?;
            Ok(response.into_inner())
        }
    };
}

/// Empty 요청 → 응답의 특정 필드 반환 RPC 메서드 생성 매크로
#[macro_export]
macro_rules! impl_empty_rpc_field {
    (
        $(#[$meta:meta])*
        $fn_name:ident -> $field_type:ty,
        $rpc_method:ident,
        $empty_type:ty,
        $field:ident
    ) => {
        $(#[$meta])*
        pub async fn $fn_name(&mut self) -> Result<$field_type, $crate::GrpcClientError> {
            let request = self.authenticated_request(<$empty_type>::default())?;
            let response = self.client.$rpc_method(request).await?;
            Ok(response.into_inner().$field)
        }
    };
}
