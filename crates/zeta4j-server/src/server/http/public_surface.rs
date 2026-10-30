//! Public HTTP routes that skip authentication. Every other route requires an
//! authenticated active principal.

use axum::http::Method;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicHttpSurface {
    pub method: &'static str,
    pub path: &'static str,
    pub action: &'static str,
    pub test_id: &'static str,
}

pub static PUBLIC_HTTP_SURFACES: &[PublicHttpSurface] = &[
    PublicHttpSurface {
        method: "GET",
        path: "/health",
        action: "http.public.health",
        test_id: "SEC-PUBLIC-HEALTH",
    },
    PublicHttpSurface {
        method: "GET",
        path: "/ready",
        action: "http.public.ready",
        test_id: "SEC-PUBLIC-READY",
    },
    PublicHttpSurface {
        method: "POST",
        path: "/auth/login",
        action: "http.public.auth-login",
        test_id: "SEC-PUBLIC-LOGIN",
    },
    PublicHttpSurface {
        method: "POST",
        path: "/auth/bootstrap/complete",
        action: "http.public.bootstrap-complete",
        test_id: "SEC-PUBLIC-BOOTSTRAP",
    },
    PublicHttpSurface {
        method: "POST",
        path: "/auth/refresh",
        action: "http.public.auth-refresh",
        test_id: "SEC-PUBLIC-REFRESH",
    },
    PublicHttpSurface {
        method: "POST",
        path: "/auth/logout",
        action: "http.public.auth-logout",
        test_id: "SEC-PUBLIC-LOGOUT",
    },
    PublicHttpSurface {
        method: "GET",
        path: "/auth/whoami",
        action: "http.public.auth-whoami",
        test_id: "SEC-PUBLIC-WHOAMI",
    },
    PublicHttpSurface {
        method: "GET",
        path: "/auth/sessions",
        action: "http.public.auth-sessions",
        test_id: "SEC-PUBLIC-SESSIONS",
    },
    PublicHttpSurface {
        method: "POST",
        path: "/auth/revoke-session",
        action: "http.public.auth-revoke-session",
        test_id: "SEC-PUBLIC-REVOKE-SESSION",
    },
];

pub fn is_public_http_surface(method: &Method, path: &str) -> bool {
    PUBLIC_HTTP_SURFACES
        .iter()
        .any(|surface| surface.method == method.as_str() && surface.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_registered_routes_are_public() {
        assert!(is_public_http_surface(&Method::GET, "/health"));
        assert!(is_public_http_surface(&Method::POST, "/auth/login"));
        assert!(!is_public_http_surface(&Method::POST, "/health"));
        assert!(!is_public_http_surface(
            &Method::POST,
            "/db/zeta4j/tx/commit"
        ));
        assert!(!is_public_http_surface(&Method::GET, "/auth/login/extra"));
    }
}
