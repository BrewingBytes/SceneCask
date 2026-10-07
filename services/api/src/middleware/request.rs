//! Request IDs, private response headers and body-size limits.

use crate::{error, error::ApiError, middleware::Security};
use axum::{
    extract::{MatchedPath, Request, State},
    http::{HeaderName, HeaderValue, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::Instrument;
use uuid::Uuid;

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Assigns a server-generated UUID to each request. Client-supplied IDs are ignored so the value
/// can never carry secrets. Logs only method, route template and status: raw URIs can contain
/// tokens or search text.
pub async fn request_id(request: Request, next: Next) -> Response {
    let id = Uuid::new_v4();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", |path| path.as_str())
        .to_owned();
    let span = tracing::info_span!("request", request_id = %id, method = %request.method(), route);
    let mut response = error::with_request_id(id, next.run(request))
        .instrument(span.clone())
        .await;
    span.in_scope(|| tracing::info!(status = response.status().as_u16(), "request completed"));
    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        HeaderValue::from_str(&id.to_string()).expect("UUID is a valid header value"),
    );
    response
}

/// Every API response carries viewer state or session cookies, so none may enter shared caches.
pub async fn private_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

/// Rejects a declared oversized body before reading it. Streaming bodies without a length are
/// capped by `DefaultBodyLimit` in the extractors.
pub async fn limit_body(
    State(security): State<Security>,
    request: Request,
    next: Next,
) -> Response {
    let declared = request.headers().get(header::CONTENT_LENGTH).map(|value| {
        value
            .to_str()
            .ok()
            .and_then(|text| text.parse::<u64>().ok())
    });
    match declared {
        Some(None) => ApiError::malformed().into_response(),
        Some(Some(length)) if length > security.config.body_limit_bytes as u64 => {
            ApiError::malformed().into_response()
        }
        _ => next.run(request).await,
    }
}
