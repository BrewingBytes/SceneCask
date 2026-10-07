//! C03 error envelope: `{error:{code,message,fields?,requestId}}` with safe, fixed messages.
//! Messages never echo submitted data, protected strings, asset paths or configuration.

use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::collections::BTreeMap;
use uuid::Uuid;

tokio::task_local! {
    /// Set by the request-ID middleware for the lifetime of each request.
    static REQUEST_ID: Uuid;
}

/// Runs `future` with `id` as the request ID reported by every error envelope it produces.
pub async fn with_request_id<F: Future>(id: Uuid, future: F) -> F::Output {
    REQUEST_ID.scope(id, future).await
}

/// The current request ID, or a fresh one outside the request-ID middleware.
pub fn current_request_id() -> Uuid {
    REQUEST_ID
        .try_with(|id| *id)
        .unwrap_or_else(|_| Uuid::new_v4())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    AuthRequired,
    InvalidCredentials,
    EmailUnverified,
    CsrfFailed,
    SpoilerLocked,
    OperatorRequired,
    NotFound,
    RevisionConflict,
    PreviewStale,
    IdentityLinkRequired,
    IdentityInUse,
    LastSigninMethod,
    HandleTaken,
    TokenExpired,
    ActionExpired,
    ValidationError,
    InvalidEpisodeScope,
    RateLimited,
    ProviderUnavailable,
    ServiceUnavailable,
}

impl ErrorCode {
    /// The C03 status and contract message for each code.
    fn defaults(self) -> (StatusCode, &'static str) {
        use ErrorCode::*;
        match self {
            AuthRequired | InvalidCredentials => (StatusCode::UNAUTHORIZED, "Sign in to continue."),
            EmailUnverified => (StatusCode::FORBIDDEN, "Verify your email to continue."),
            CsrfFailed | SpoilerLocked | OperatorRequired => {
                (StatusCode::FORBIDDEN, "This request is not permitted.")
            }
            NotFound => (StatusCode::NOT_FOUND, "Resource not found."),
            RevisionConflict | PreviewStale | IdentityLinkRequired | IdentityInUse
            | LastSigninMethod | HandleTaken => (StatusCode::CONFLICT, "Refresh and try again."),
            TokenExpired | ActionExpired => (StatusCode::GONE, "This action has expired."),
            ValidationError | InvalidEpisodeScope => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "Check the highlighted fields.",
            ),
            RateLimited => (StatusCode::TOO_MANY_REQUESTS, "Try again later."),
            ProviderUnavailable => (
                StatusCode::BAD_GATEWAY,
                "Metadata is temporarily unavailable.",
            ),
            ServiceUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Service is temporarily unavailable.",
            ),
        }
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: ErrorCode,
    message: &'static str,
    fields: BTreeMap<&'static str, &'static str>,
    retry_after: Option<u64>,
}

impl ApiError {
    pub fn new(code: ErrorCode) -> Self {
        let (status, message) = code.defaults();
        Self {
            status,
            code,
            message,
            fields: BTreeMap::new(),
            retry_after: None,
        }
    }

    pub fn auth_required() -> Self {
        Self::new(ErrorCode::AuthRequired)
    }

    pub fn csrf_failed() -> Self {
        Self::new(ErrorCode::CsrfFailed)
    }

    pub fn not_found() -> Self {
        Self::new(ErrorCode::NotFound)
    }

    /// 400 for a body, cursor or header the server cannot parse.
    pub fn malformed() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: "Check the request format.",
            ..Self::new(ErrorCode::ValidationError)
        }
    }

    pub fn rate_limited(retry_after_secs: u64) -> Self {
        Self {
            retry_after: Some(retry_after_secs.max(1)),
            ..Self::new(ErrorCode::RateLimited)
        }
    }

    /// 503 for an unexpected server-side failure. Only the fixed `category` is logged.
    pub fn unavailable(category: &'static str) -> Self {
        tracing::error!(category, "request failed");
        Self::new(ErrorCode::ServiceUnavailable)
    }

    /// Adds a fixed field message; never pass submitted values.
    pub fn with_field(mut self, field: &'static str, message: &'static str) -> Self {
        self.fields.insert(field, message);
        self
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    error: Body<'a>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Body<'a> {
    code: ErrorCode,
    message: &'a str,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    fields: &'a BTreeMap<&'static str, &'static str>,
    request_id: Uuid,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Envelope {
            error: Body {
                code: self.code,
                message: self.message,
                fields: &self.fields,
                request_id: current_request_id(),
            },
        };
        let mut response = (self.status, Json(body)).into_response();
        if let Some(seconds) = self.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
        }
        response
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        // sqlx error text can contain connection details or row data; log only a category.
        Self::unavailable(crate::failure_kind(&error))
    }
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        match rejection {
            // Well-formed JSON with wrong types or unknown keys (C03 rejects unknown properties).
            JsonRejection::JsonDataError(_) => Self::new(ErrorCode::ValidationError),
            // Content type is enforced by the CSRF layer for mutations; anything else is malformed.
            _ => Self::malformed(),
        }
    }
}

/// Fallback for unknown routes so they use the envelope.
pub async fn not_found() -> ApiError {
    ApiError::not_found()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body(error: ApiError) -> (StatusCode, serde_json::Value, Option<HeaderValue>) {
        let response = with_request_id(Uuid::nil(), async { error.into_response() }).await;
        let status = response.status();
        let retry = response.headers().get(header::RETRY_AFTER).cloned();
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap(), retry)
    }

    #[tokio::test]
    async fn renders_contract_envelope_with_request_id() {
        let (status, json, _) = body(ApiError::new(ErrorCode::ValidationError).with_field(
            "handle",
            "Use 3–30 lowercase letters, numbers or underscores.",
        ))
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            json,
            serde_json::json!({"error": {
                "code": "VALIDATION_ERROR",
                "message": "Check the highlighted fields.",
                "fields": {"handle": "Use 3–30 lowercase letters, numbers or underscores."},
                "requestId": Uuid::nil(),
            }})
        );
    }

    #[tokio::test]
    async fn rate_limit_sets_retry_after_and_omits_fields() {
        let (status, json, retry) = body(ApiError::rate_limited(0)).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(retry.unwrap(), "1");
        assert_eq!(json["error"]["code"], "RATE_LIMITED");
        assert!(json["error"].get("fields").is_none());
    }

    #[tokio::test]
    async fn statuses_follow_c03() {
        for (code, status) in [
            (ErrorCode::AuthRequired, 401),
            (ErrorCode::EmailUnverified, 403),
            (ErrorCode::CsrfFailed, 403),
            (ErrorCode::OperatorRequired, 403),
            (ErrorCode::NotFound, 404),
            (ErrorCode::PreviewStale, 409),
            (ErrorCode::TokenExpired, 410),
            (ErrorCode::InvalidEpisodeScope, 422),
            (ErrorCode::ProviderUnavailable, 502),
            (ErrorCode::ServiceUnavailable, 503),
        ] {
            assert_eq!(ApiError::new(code).status().as_u16(), status);
        }
        assert_eq!(ApiError::malformed().status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn request_id_outside_scope_is_random_uuid() {
        assert_ne!(current_request_id(), current_request_id());
    }
}
