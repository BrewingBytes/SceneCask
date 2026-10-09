//! Cookie names, parsing and Set-Cookie values (C04). Cookie values are opaque secrets; never
//! log them.

use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue, header};

use super::secret::Secret;
use crate::middleware::CookieMode;

#[derive(Clone, Copy)]
pub enum Kind {
    /// Authenticated session ID.
    Session,
    /// Anonymous CSRF bootstrap secret; not an authenticated session.
    AnonymousCsrf,
    /// Google sign-in state, binding a callback to the browser that started the flow.
    GoogleFlow,
}

pub fn name(kind: Kind, mode: CookieMode) -> &'static str {
    match (kind, mode) {
        (Kind::Session, CookieMode::Secure) => "__Host-scenecask",
        (Kind::Session, CookieMode::LocalhostDevelopment) => "scenecask",
        (Kind::AnonymousCsrf, CookieMode::Secure) => "__Host-scenecask-csrf",
        (Kind::AnonymousCsrf, CookieMode::LocalhostDevelopment) => "scenecask-csrf",
        (Kind::GoogleFlow, CookieMode::Secure) => "__Host-scenecask-google",
        (Kind::GoogleFlow, CookieMode::LocalhostDevelopment) => "scenecask-google",
    }
}

/// The first well-formed value of the named cookie. Malformed values are ignored.
pub fn read(headers: &HeaderMap, kind: Kind, mode: CookieMode) -> Option<Secret> {
    let wanted = name(kind, mode);
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(name, _)| *name == wanted)
        .find_map(|(_, value)| Secret::decode(value))
}

/// Set-Cookie for `secret`; `max_age` of `None` makes a browser-session cookie.
pub fn set(
    kind: Kind,
    mode: CookieMode,
    secret: &Secret,
    max_age: Option<Duration>,
) -> HeaderValue {
    let max_age = max_age.map_or(String::new(), |age| format!("; Max-Age={}", age.as_secs()));
    value(kind, mode, &secret.encode(), &max_age)
}

/// Set-Cookie that expires the named cookie immediately.
pub fn clear(kind: Kind, mode: CookieMode) -> HeaderValue {
    value(kind, mode, "", "; Max-Age=0")
}

/// True when `headers` already set or cleared the named cookie.
pub fn is_set(headers: &HeaderMap, kind: Kind, mode: CookieMode) -> bool {
    let prefix = format!("{}=", name(kind, mode));
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .any(|value| value.as_bytes().starts_with(prefix.as_bytes()))
}

fn value(kind: Kind, mode: CookieMode, value: &str, max_age: &str) -> HeaderValue {
    let secure = match mode {
        CookieMode::Secure => "; Secure",
        CookieMode::LocalhostDevelopment => "",
    };
    HeaderValue::from_str(&format!(
        "{}={value}; Path=/; HttpOnly; SameSite=Lax{secure}{max_age}",
        name(kind, mode)
    ))
    .expect("cookie values are base64url")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_cookie_attributes() {
        let secret = Secret::generate().unwrap();
        let value = set(
            Kind::Session,
            CookieMode::Secure,
            &secret,
            Some(Duration::from_secs(60)),
        );
        assert_eq!(
            value.to_str().unwrap(),
            format!(
                "__Host-scenecask={}; Path=/; HttpOnly; SameSite=Lax; Secure; Max-Age=60",
                secret.encode()
            )
        );
        assert_eq!(
            clear(Kind::AnonymousCsrf, CookieMode::LocalhostDevelopment),
            "scenecask-csrf=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"
        );
    }

    #[test]
    fn reads_only_the_named_well_formed_cookie() {
        let secret = Secret::generate().unwrap();
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_str(&format!(
                "scenecask=short; __Host-scenecask-csrf={0}; __Host-scenecask=bad!; __Host-scenecask={0}",
                secret.encode()
            ))
            .unwrap(),
        );
        let read_session = read(&headers, Kind::Session, CookieMode::Secure).unwrap();
        assert_eq!(read_session.encode(), secret.encode());
        assert!(read(&headers, Kind::Session, CookieMode::LocalhostDevelopment).is_none());
    }
}
