//! Opt-in same-origin UI host. No signing, key custody or password endpoints.
use crate::SharedNode;
use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
struct WebHost {
    root: Arc<PathBuf>,
    origin: String,
    host: String,
}

/// `origin` is derived by the CLI from the bound numeric loopback listener.
pub fn router(node: SharedNode, directory: &Path, origin: &str) -> Result<Router> {
    host(crate::api_router(node), directory, origin)
}

/// Reuse the exact same-origin asset/HTTP boundary in the native app host.
pub fn host(api: Router, directory: &Path, origin: &str) -> Result<Router> {
    orvexa_rpc::Client::new(origin)?;
    let root = directory
        .canonicalize()
        .context("build the web app first (scripts/build-web.ps1)")?;
    ensure!(
        root.join("index.html").is_file(),
        "web directory has no index.html; build web first"
    );
    let host = origin
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();
    let host = WebHost {
        root: Arc::new(root),
        origin: origin.trim_end_matches('/').into(),
        host,
    };
    let assets = Router::new().fallback(asset).with_state(host.clone());
    Ok(api
        .merge(assets)
        .layer(middleware::from_fn_with_state(host, guard)))
}

async fn guard(State(host): State<WebHost>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let valid_host = headers.get("host").and_then(|h| h.to_str().ok()) == Some(host.host.as_str());
    let valid_origin = headers
        .get("origin")
        .is_none_or(|o| o.to_str().ok() == Some(host.origin.as_str()));
    let cross_site = headers
        .get("sec-fetch-site")
        .is_some_and(|s| s == "cross-site");
    if !valid_host || !valid_origin || cross_site {
        return (
            StatusCode::FORBIDDEN,
            "Only this numeric loopback origin is allowed",
        )
            .into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("content-security-policy", HeaderValue::from_static("default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; img-src 'self' data: blob:; connect-src 'self'; worker-src 'self'; font-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

async fn asset(State(host): State<WebHost>, request: Request) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let path = request.uri().path();
    let relative = if path == "/" {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };
    if relative.contains("..") || relative.contains('\\') || relative.starts_with("v1/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let file = match host.root.join(relative).canonicalize() {
        Ok(p) if p.starts_with(host.root.as_ref()) && p.is_file() => p,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let content_type = match file.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("wasm") => "application/wasm",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let bytes = match std::fs::read(file) {
        Ok(b) if b.len() <= 8 * 1024 * 1024 => b,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let body = if request.method() == Method::HEAD {
        Body::empty()
    } else {
        Body::from(bytes)
    };
    ([("content-type", content_type)], body).into_response()
}
