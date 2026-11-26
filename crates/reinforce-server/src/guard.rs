//! Request origin checks for a server that browsers on the same machine can reach.
//!
//! Single-auth mode has no login wall, so any web page the owner visits could
//! otherwise talk to the local server: a cross-site form can POST to it, and a
//! DNS-rebinding page can read and write everything under its own origin.
//! Rejecting foreign `Host` headers (single mode) and cross-origin unsafe
//! requests (both modes) closes both paths. Non-browser clients such as the
//! CLI or an MCP bridge send no `Origin` and are unaffected.

use axum::extract::{Request, State};
use axum::http::{Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::config::AuthMode;
use crate::error::ApiError;
use crate::state::AppState;

/// Hostname part of a `host[:port]` authority, keeping IPv6 brackets.
fn hostname(authority: &str) -> &str {
    if authority.starts_with('[') {
        return authority
            .find(']')
            .map_or(authority, |end| &authority[..=end]);
    }
    authority.split(':').next().unwrap_or(authority)
}

fn is_local_hostname(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "localhost"
        || name.ends_with(".localhost")
        || name == "[::1]"
        || name
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
}

fn host_is_allowed(host: &str, bind_host: &str) -> bool {
    let name = hostname(host);
    let bind = bind_host.trim_start_matches('[').trim_end_matches(']');
    is_local_hostname(name)
        || name
            .trim_start_matches('[')
            .trim_end_matches(']')
            .eq_ignore_ascii_case(bind)
}

/// Authority of an `Origin` header value such as `http://localhost:3000`.
fn origin_authority(origin: &str) -> Option<&str> {
    let (_, rest) = origin.split_once("://")?;
    Some(rest.split('/').next().unwrap_or(rest))
}

fn is_unsafe(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

pub async fn check_origin(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let host_value = request.headers().get(header::HOST).cloned();
    let host = host_value.as_ref().and_then(|value| value.to_str().ok());

    if state.auth_mode == AuthMode::Single {
        match host {
            Some(host) if host_is_allowed(host, &state.bind_host) => {}
            _ => {
                return ApiError::forbidden("a valid local Host header is required")
                    .into_response();
            }
        }
    }

    if is_unsafe(request.method()) {
        if let Some(origin) = request.headers().get(header::ORIGIN) {
            let same_origin = origin
                .to_str()
                .ok()
                .and_then(origin_authority)
                .zip(host)
                .is_some_and(|(origin, host)| origin.eq_ignore_ascii_case(host));
            if !same_origin {
                return ApiError::forbidden("cross-origin requests are not allowed")
                    .into_response();
            }
        }
    }

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hosts_are_allowed() {
        for host in [
            "localhost:3000",
            "127.0.0.1:3000",
            "127.0.0.2",
            "[::1]:3000",
            "app.localhost",
        ] {
            assert!(host_is_allowed(host, "127.0.0.1"), "{host}");
        }
    }

    #[test]
    fn bind_host_comparison_ignores_case() {
        assert!(host_is_allowed("LOCALHOST:3000", "localhost"));
        assert!(host_is_allowed("[::1]:3000", "::1"));
    }

    #[test]
    fn foreign_hosts_are_rejected_unless_bound_explicitly() {
        assert!(!host_is_allowed("attacker.example:3000", "127.0.0.1"));
        assert!(!host_is_allowed("192.168.1.20:3000", "127.0.0.1"));
        assert!(host_is_allowed("192.168.1.20:3000", "192.168.1.20"));
    }

    #[test]
    fn origin_authority_strips_scheme_and_path() {
        assert_eq!(
            origin_authority("http://localhost:3000"),
            Some("localhost:3000")
        );
        assert_eq!(origin_authority("null"), None);
    }
}
