//! HTTP Middleware

#![allow(dead_code)]

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{HeaderValue, Request, Response, StatusCode};
use futures::future::BoxFuture;
use tower::{Layer, Service};

use super::config::RateLimitHttpConfig;

/// 요청 ID 레이어
#[derive(Clone, Default)]
pub struct RequestIdLayer;

impl RequestIdLayer {
    /// 새 레이어 생성
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for RequestIdLayer {
    type Service = RequestIdService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestIdService { inner }
    }
}

/// 요청 ID 서비스
#[derive(Clone)]
pub struct RequestIdService<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for RequestIdService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    ReqBody: Send + 'static,
    ResBody: Default + Send + 'static,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // 기존 요청 ID 확인 또는 새로 생성
        let request_id = req
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(canonical_request_id)
            .unwrap_or_else(generate_request_id);

        // 요청에 ID 저장
        req.extensions_mut().insert(RequestId(request_id.clone()));

        let mut inner = self.inner.clone();

        Box::pin(async move {
            let mut response = inner.call(req).await?;

            // 응답 헤더에 요청 ID 추가
            if let Ok(header_value) = HeaderValue::from_str(&request_id) {
                response.headers_mut().insert("x-request-id", header_value);
            }

            Ok(response)
        })
    }
}

const MAX_REQUEST_ID_LENGTH: usize = 128;

fn generate_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Accepts a client request ID only when it is short and uses safe characters.
fn canonical_request_id(value: &str) -> String {
    let valid = !value.is_empty()
        && value.len() <= MAX_REQUEST_ID_LENGTH
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-_:".contains(&byte));
    if valid {
        value.to_string()
    } else {
        generate_request_id()
    }
}

/// 요청 ID 확장
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

impl RequestId {
    /// ID 값 반환
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 요청 로깅 레이어
#[derive(Clone)]
pub struct RequestLoggingLayer {
    /// 요청 본문 로깅 여부
    pub include_body: bool,
}

impl RequestLoggingLayer {
    /// 새 레이어 생성
    pub fn new() -> Self {
        Self {
            include_body: false,
        }
    }

    /// 본문 포함 설정
    pub fn with_body(mut self) -> Self {
        self.include_body = true;
        self
    }
}

impl Default for RequestLoggingLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for RequestLoggingLayer {
    type Service = RequestLoggingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestLoggingService {
            inner,
            include_body: self.include_body,
        }
    }
}

/// 요청 로깅 서비스
#[derive(Clone)]
pub struct RequestLoggingService<S> {
    inner: S,
    #[expect(dead_code)]
    include_body: bool,
}

impl<S> Service<Request<Body>> for RequestLoggingService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let start = Instant::now();

        let request_id = req
            .extensions()
            .get::<RequestId>()
            .map(|id| id.0.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let mut inner = self.inner.clone();

        Box::pin(async move {
            let response = inner.call(req).await?;
            let duration = start.elapsed();
            let status = response.status();

            tracing::info!(
                request_id = %request_id,
                method = %method,
                uri = %uri,
                status = %status.as_u16(),
                duration_ms = %duration.as_millis(),
                "HTTP request"
            );

            Ok(response)
        })
    }
}

/// 요청 타이밍 정보
#[derive(Clone, Debug)]
pub struct RequestTiming {
    /// 요청 시작 시간
    pub start: Instant,
}

impl RequestTiming {
    /// 새 타이밍 생성
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// 경과 시간 (밀리초)
    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

impl Default for RequestTiming {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Rate Limiting Middleware
// ============================================================================

/// Token Bucket for rate limiting
#[derive(Debug)]
pub struct TokenBucket {
    /// 현재 토큰 수
    tokens: AtomicU64,
    /// 최대 토큰 수 (버스트 포함)
    max_tokens: u64,
    /// 윈도우당 리필 토큰 수
    refill_tokens: u64,
    /// 마지막 리필 시간
    last_refill: RwLock<Instant>,
    /// 윈도우 크기
    window_duration: Duration,
}

impl TokenBucket {
    /// 새 토큰 버킷 생성
    pub fn new(max_tokens: u64, refill_tokens: u64, window_duration: Duration) -> Self {
        Self {
            tokens: AtomicU64::new(max_tokens),
            max_tokens,
            refill_tokens,
            last_refill: RwLock::new(Instant::now()),
            window_duration,
        }
    }

    /// 토큰 소비 시도
    pub fn try_consume(&self) -> bool {
        self.refill();

        loop {
            let current = self.tokens.load(Ordering::Acquire);
            if current == 0 {
                return false;
            }

            if self
                .tokens
                .compare_exchange_weak(current, current - 1, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    /// 토큰 리필
    fn refill(&self) {
        let now = Instant::now();
        let mut last_refill = self.last_refill.write().unwrap_or_else(|e| e.into_inner());

        let elapsed = now.duration_since(*last_refill);
        if elapsed >= self.window_duration {
            // 윈도우가 지나면 토큰 리필
            let windows_passed = elapsed.as_millis() / self.window_duration.as_millis();
            let tokens_to_add = (windows_passed as u64) * self.refill_tokens;

            let current = self.tokens.load(Ordering::Acquire);
            let new_tokens = (current + tokens_to_add).min(self.max_tokens);
            self.tokens.store(new_tokens, Ordering::Release);

            *last_refill = now;
        }
    }

    /// 남은 토큰 수
    pub fn remaining(&self) -> u64 {
        self.refill();
        self.tokens.load(Ordering::Acquire)
    }

    /// 리셋까지 남은 시간 (초)
    pub fn reset_after_secs(&self) -> u64 {
        let last = *self.last_refill.read().unwrap_or_else(|e| e.into_inner());
        let elapsed = Instant::now().duration_since(last);
        if elapsed >= self.window_duration {
            0
        } else {
            (self.window_duration - elapsed).as_secs()
        }
    }
}

/// Rate Limiter 상태 (IP별 토큰 버킷 관리)
#[derive(Clone)]
pub struct RateLimiterState {
    buckets: Arc<RwLock<HashMap<IpAddr, Arc<TokenBucket>>>>,
    anonymous_bucket: Arc<TokenBucket>,
    config: RateLimitHttpConfig,
}

impl RateLimiterState {
    /// 새 Rate Limiter 상태 생성
    pub fn new(config: RateLimitHttpConfig) -> Self {
        let anonymous_bucket = Arc::new(TokenBucket::new(
            config.requests_per_window + config.burst_size,
            config.requests_per_window,
            config.window_duration,
        ));
        Self {
            buckets: Arc::new(RwLock::new(HashMap::new())),
            anonymous_bucket,
            config,
        }
    }

    /// IP에 대한 토큰 버킷 가져오기 (없으면 생성)
    pub fn get_bucket(&self, ip: IpAddr) -> Arc<TokenBucket> {
        // 읽기 락으로 먼저 확인
        {
            let buckets = self.buckets.read().unwrap_or_else(|e| e.into_inner());
            if let Some(bucket) = buckets.get(&ip) {
                return bucket.clone();
            }
        }

        // 없으면 쓰기 락으로 생성
        let mut buckets = self.buckets.write().unwrap_or_else(|e| e.into_inner());
        if buckets.len() >= self.config.max_client_buckets {
            buckets.retain(|_, bucket| {
                bucket
                    .last_refill
                    .read()
                    .unwrap_or_else(|error| error.into_inner())
                    .elapsed()
                    < Duration::from_secs(600)
            });
        }
        if buckets.len() >= self.config.max_client_buckets {
            return self.anonymous_bucket.clone();
        }
        buckets
            .entry(ip)
            .or_insert_with(|| {
                Arc::new(TokenBucket::new(
                    self.config.requests_per_window + self.config.burst_size,
                    self.config.requests_per_window,
                    self.config.window_duration,
                ))
            })
            .clone()
    }

    fn is_trusted_proxy(&self, ip: &IpAddr) -> bool {
        let ip = ip.to_string();
        self.config
            .trusted_proxy_ips
            .iter()
            .any(|trusted| trusted == &ip)
    }

    /// 제외 경로인지 확인
    pub fn is_excluded_path(&self, path: &str) -> bool {
        self.config
            .excluded_paths
            .iter()
            .any(|p| path.starts_with(p))
    }

    /// 헤더 포함 여부
    pub fn include_headers(&self) -> bool {
        self.config.include_headers
    }

    /// 오래된 버킷 정리 (메모리 관리)
    pub fn cleanup_old_buckets(&self) {
        let mut buckets = self.buckets.write().unwrap_or_else(|e| e.into_inner());
        // 10분 이상 사용하지 않은 버킷 제거
        buckets.retain(|_, bucket| {
            let last = *bucket
                .last_refill
                .read()
                .unwrap_or_else(|error| error.into_inner());
            last.elapsed() < Duration::from_secs(600)
        });
    }
}

/// Rate Limit 레이어
#[derive(Clone)]
pub struct RateLimitLayer {
    state: RateLimiterState,
}

impl RateLimitLayer {
    /// 새 레이어 생성
    pub fn new(config: RateLimitHttpConfig) -> Self {
        Self {
            state: RateLimiterState::new(config),
        }
    }

    /// 상태 참조
    pub fn state(&self) -> &RateLimiterState {
        &self.state
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            state: self.state.clone(),
        }
    }
}

/// Rate Limit 서비스
#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    state: RateLimiterState,
}

impl<S> Service<Request<Body>> for RateLimitService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let state = self.state.clone();
        let path = req.uri().path().to_string();

        // 클라이언트 IP 추출
        let client_ip = extract_client_ip(&req, &state);

        // 제외 경로 확인
        if state.is_excluded_path(&path) {
            let mut inner = self.inner.clone();
            return Box::pin(async move { inner.call(req).await });
        }

        // Rate Limit 확인
        {
            let bucket =
                client_ip.map_or_else(|| state.anonymous_bucket.clone(), |ip| state.get_bucket(ip));

            if !bucket.try_consume() {
                // Rate Limit 초과
                let remaining = bucket.remaining();
                let reset_after = bucket.reset_after_secs();
                let include_headers = state.include_headers();

                tracing::warn!(
                    client_ip = %client_ip.map_or_else(|| "anonymous".to_string(), |ip| ip.to_string()),
                    path = %path,
                    remaining = remaining,
                    reset_after_secs = reset_after,
                    "Rate limit exceeded"
                );

                return Box::pin(async move {
                    let mut response =
                        Response::new(Body::from("Rate limit exceeded. Please try again later."));
                    *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;

                    if include_headers {
                        let headers = response.headers_mut();
                        headers.insert("X-RateLimit-Remaining", HeaderValue::from_static("0"));
                        if let Ok(value) = HeaderValue::from_str(&reset_after.to_string()) {
                            headers.insert("X-RateLimit-Reset", value.clone());
                            headers.insert("Retry-After", value);
                        }
                    }

                    Ok(response)
                });
            }

            // 요청 허용, 헤더 추가
            let remaining = bucket.remaining();
            let reset_after = bucket.reset_after_secs();
            let include_headers = state.include_headers();
            let mut inner = self.inner.clone();

            Box::pin(async move {
                let mut response = inner.call(req).await?;

                if include_headers {
                    let headers = response.headers_mut();
                    if let Ok(value) = HeaderValue::from_str(&remaining.to_string()) {
                        headers.insert("X-RateLimit-Remaining", value);
                    }
                    if let Ok(value) = HeaderValue::from_str(&reset_after.to_string()) {
                        headers.insert("X-RateLimit-Reset", value);
                    }
                }

                Ok(response)
            })
        }
    }
}

/// 요청에서 클라이언트 IP 추출
fn extract_client_ip<B>(req: &Request<B>, state: &RateLimiterState) -> Option<IpAddr> {
    let peer = req
        .extensions()
        .get::<std::net::SocketAddr>()
        .map(std::net::SocketAddr::ip);
    let peer = peer?;
    if !state.is_trusted_proxy(&peer) {
        return Some(peer);
    }
    let forwarded = req.headers().get("x-forwarded-for")?.to_str().ok()?;
    if forwarded.contains(',') {
        return None;
    }
    forwarded.trim().parse::<IpAddr>().ok()
}

/// Rate Limit 정보 (응답 헤더용)
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    /// 남은 요청 수
    pub remaining: u64,
    /// 리셋까지 남은 시간 (초)
    pub reset_after_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_id_accepts_only_canonical_values() {
        assert_eq!(
            canonical_request_id("client:request-123").as_str(),
            "client:request-123"
        );
        assert_ne!(
            canonical_request_id("contains space").as_str(),
            "contains space"
        );
        let too_long = "x".repeat(MAX_REQUEST_ID_LENGTH + 1);
        assert_ne!(canonical_request_id(&too_long), too_long);
    }

    #[test]
    fn forwarded_address_requires_an_exact_trusted_peer() -> Result<(), Box<dyn std::error::Error>>
    {
        let state = RateLimiterState::new(RateLimitHttpConfig {
            trusted_proxy_ips: vec!["10.0.0.2".to_string()],
            ..RateLimitHttpConfig::default()
        });
        let mut untrusted = Request::new(());
        untrusted
            .extensions_mut()
            .insert("10.0.0.3:9044".parse::<std::net::SocketAddr>()?);
        untrusted
            .headers_mut()
            .insert("x-forwarded-for", "203.0.113.9".parse()?);
        assert_eq!(
            extract_client_ip(&untrusted, &state),
            Some("10.0.0.3".parse()?)
        );

        let mut trusted = Request::new(());
        trusted
            .extensions_mut()
            .insert("10.0.0.2:9044".parse::<std::net::SocketAddr>()?);
        trusted
            .headers_mut()
            .insert("x-forwarded-for", "203.0.113.9".parse()?);
        assert_eq!(
            extract_client_ip(&trusted, &state),
            Some("203.0.113.9".parse()?)
        );
        trusted
            .headers_mut()
            .insert("x-forwarded-for", "203.0.113.9, 10.0.0.4".parse()?);
        assert_eq!(extract_client_ip(&trusted, &state), None);
        Ok(())
    }

    #[test]
    fn missing_addresses_and_bucket_spray_are_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let state = RateLimiterState::new(RateLimitHttpConfig {
            max_client_buckets: 2,
            ..RateLimitHttpConfig::default()
        });
        assert_eq!(extract_client_ip(&Request::new(()), &state), None);
        let first = state.get_bucket("192.0.2.1".parse()?);
        let second = state.get_bucket("192.0.2.2".parse()?);
        let overflow = state.get_bucket("192.0.2.3".parse()?);
        assert!(!Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&overflow, &state.anonymous_bucket));
        assert_eq!(
            state
                .buckets
                .read()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
            2
        );
        Ok(())
    }
}
