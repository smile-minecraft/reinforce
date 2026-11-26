use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/dist"]
#[allow_missing = true]
struct WebAssets;

const FALLBACK_HTML: &str = r#"<!doctype html>
<html lang="zh-Hant">
<head><meta charset="utf-8"><title>Reinforce</title></head>
<body><h1>Reinforce</h1><p>The WebApp bundle is not built. Run the frontend build to populate <code>web/dist</code>.</p></body>
</html>
"#;

pub fn serve_spa(path: &str) -> Response {
    let requested = path.trim_start_matches('/');
    let candidate = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    let file = WebAssets::get(candidate).or_else(|| WebAssets::get("index.html"));
    let Some(file) = file else {
        return (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            )],
            FALLBACK_HTML,
        )
            .into_response();
    };
    let content_type = mime_for(candidate);
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(file.data.into_owned()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

fn mime_for(path: &str) -> HeaderValue {
    let mime = if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") || path.ends_with(".mjs") {
        "text/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".json") || path.ends_with(".map") {
        "application/json; charset=utf-8"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    };
    HeaderValue::from_static(mime)
}
