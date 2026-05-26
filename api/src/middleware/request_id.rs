//! Attach a `request_id` to every request span. Lets you correlate
//! 5xx errors logged in `error.rs` with the original request.

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderValue, Response},
    middleware::Next,
};
use uuid::Uuid;

const HEADER: &str = "x-request-id";

pub async fn middleware(mut request: Request, next: Next) -> Response<Body> {
    let request_id = request
        .headers()
        .get(HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));

    let span = tracing::info_span!("request", request_id = %request_id);
    let _guard = span.enter();

    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert(HEADER, value);
    }
    response
}

#[derive(Debug, Clone)]
pub struct RequestId(pub String);
