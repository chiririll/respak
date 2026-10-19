//! The browser-facing protection of a local server (docs/design/webui.md section 8).

use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;

/// What the server answers to.
#[derive(Debug, Clone)]
pub struct Guard {
    hosts: Vec<String>,
}

impl Guard {
    /// A guard for a server on `127.0.0.1:port`, reachable as `localhost` too.
    pub fn new(port: u16) -> Self {
        Self {
            hosts: vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")],
        }
    }

    /// Why `headers` of a `method` request are not acceptable, if they are not.
    fn refuse(&self, method: &Method, headers: &HeaderMap) -> Option<ApiError> {
        let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        // A foreign Host is a DNS rebinding attempt.
        match text("host") {
            Some(host) if self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) => {}
            _ => {
                return Some(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "Forbidden",
                    "unexpected Host header",
                ))
            }
        }
        if let Some(origin) = text("origin") {
            let own = self.hosts.iter().any(|h| origin == format!("http://{h}"));
            if !own {
                return Some(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "Forbidden",
                    "request from another origin",
                ));
            }
        }
        if let Some(site) = text("sec-fetch-site") {
            if !matches!(site, "same-origin" | "none") {
                return Some(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "Forbidden",
                    "cross-site request",
                ));
            }
        }
        if !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) {
            let json = text("content-type").is_some_and(|ct| {
                let essence = ct.split(';').next().unwrap_or("").trim();
                essence.eq_ignore_ascii_case("application/json")
            });
            if !json {
                return Some(ApiError::new(
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "UnsupportedMediaType",
                    "state-changing requests must be application/json",
                ));
            }
        }
        None
    }
}

/// The middleware: refuses what [`Guard::refuse`] finds.
pub async fn check(State(guard): State<Guard>, request: Request, next: Next) -> Response {
    match guard.refuse(request.method(), request.headers()) {
        Some(error) => error.into_response(),
        None => next.run(request).await,
    }
}
