//! Same-origin, content-type and session-paired CSRF checks for every unsafe method (C03/C04).
//! A rejected request never reaches its handler, so no write occurs.

use crate::{error::ApiError, middleware::Security, modules::auth::session::RequestSession};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderName, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;

pub const CSRF_HEADER: HeaderName = HeaderName::from_static("x-csrf-token");

pub async fn protect(State(security): State<Security>, request: Request, next: Next) -> Response {
    if request.method().is_safe() {
        return next.run(request).await;
    }
    let headers = request.headers();
    let session = request.extensions().get::<RequestSession>();
    let permitted = same_origin(headers, &security.config.public_origin)
        && json_or_empty(headers)
        // Authenticated mutations require the token paired to the session. Anonymous mutations
        // (public auth endpoints, logged-out logout) rely on the Origin check per C03.
        && match session.and_then(RequestSession::current) {
            Some(current) => headers
                .get(CSRF_HEADER)
                .is_some_and(|value| bool::from(value.as_bytes().ct_eq(current.csrf_token().as_bytes()))),
            None => session.is_some(),
        };
    if permitted {
        next.run(request).await
    } else {
        ApiError::csrf_failed().into_response()
    }
}

/// `Origin` must equal the configured public origin exactly; absent or `null` fails.
fn same_origin(headers: &HeaderMap, public_origin: &str) -> bool {
    let mut origins = headers.get_all(header::ORIGIN).iter();
    matches!((origins.next(), origins.next()), (Some(origin), None) if origin.as_bytes() == public_origin.as_bytes())
}

/// A body must be declared `application/json`, which a cross-site form cannot send without a
/// preflight. A bodyless request may omit the content type.
fn json_or_empty(headers: &HeaderMap) -> bool {
    match headers.get(header::CONTENT_TYPE) {
        Some(value) => value.to_str().is_ok_and(|text| {
            let essence = text.split(';').next().unwrap_or_default().trim();
            essence.eq_ignore_ascii_case("application/json")
        }),
        None => {
            !headers.contains_key(header::TRANSFER_ENCODING)
                && headers
                    .get(header::CONTENT_LENGTH)
                    .is_none_or(|length| length.as_bytes() == b"0")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(HeaderName, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn origin_must_match_exactly_once() {
        let origin = "https://scenecask.example";
        assert!(same_origin(&headers(&[(header::ORIGIN, origin)]), origin));
        for bad in [
            "https://evil.example",
            "null",
            "https://scenecask.example.evil.example",
            "http://scenecask.example",
            "https://scenecask.example/",
        ] {
            assert!(!same_origin(&headers(&[(header::ORIGIN, bad)]), origin));
        }
        assert!(!same_origin(&HeaderMap::new(), origin));
        assert!(!same_origin(
            &headers(&[(header::ORIGIN, origin), (header::ORIGIN, origin)]),
            origin
        ));
    }

    #[test]
    fn body_must_be_json() {
        assert!(json_or_empty(&HeaderMap::new()));
        assert!(json_or_empty(&headers(&[(header::CONTENT_LENGTH, "0")])));
        assert!(json_or_empty(&headers(&[(
            header::CONTENT_TYPE,
            "application/json; charset=utf-8"
        )])));
        for content_type in [
            "text/plain",
            "application/x-www-form-urlencoded",
            "multipart/form-data; boundary=x",
            "application/jsonp",
        ] {
            assert!(!json_or_empty(&headers(&[(
                header::CONTENT_TYPE,
                content_type
            )])));
        }
        assert!(!json_or_empty(&headers(&[(header::CONTENT_LENGTH, "2")])));
        assert!(!json_or_empty(&headers(&[(
            header::TRANSFER_ENCODING,
            "chunked"
        )])));
    }
}
