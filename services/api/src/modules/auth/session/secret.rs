//! Opaque 256-bit secrets for session IDs and anonymous CSRF cookies. Only the SHA-256 hash of a
//! session secret is stored; CSRF tokens are HMACs keyed by the cookie secret, so neither a
//! database row nor a CSRF token reveals the cookie.

use crate::error::ApiError;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

const LEN: usize = 32;

#[derive(Clone)]
pub struct Secret([u8; LEN]);

/// Separates tokens derived from session and anonymous secrets.
#[derive(Clone, Copy)]
pub enum CsrfPurpose {
    Session,
    Anonymous,
}

impl Secret {
    pub fn generate() -> Result<Self, ApiError> {
        let mut bytes = [0; LEN];
        getrandom::fill(&mut bytes).map_err(|_| ApiError::unavailable("entropy"))?;
        Ok(Self(bytes))
    }

    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Accepts exactly the 43-character base64url form produced by [`Secret::encode`].
    pub fn decode(value: &str) -> Option<Self> {
        if value.len() != 43 {
            return None;
        }
        URL_SAFE_NO_PAD
            .decode(value)
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .map(Self)
    }

    /// The `sessions.id_hash` value.
    pub fn hash(&self) -> Vec<u8> {
        Sha256::digest(self.0).to_vec()
    }

    pub fn csrf_token(&self, purpose: CsrfPurpose) -> String {
        let label: &[u8] = match purpose {
            CsrfPurpose::Session => b"scenecask/csrf/session/v1",
            CsrfPurpose::Anonymous => b"scenecask/csrf/anonymous/v1",
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.0).expect("HMAC accepts any key length");
        mac.update(label);
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Secret(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_malformed_values() {
        let secret = Secret::generate().unwrap();
        let encoded = secret.encode();
        assert_eq!(encoded.len(), 43);
        assert_eq!(Secret::decode(&encoded).unwrap().0, secret.0);
        for bad in [
            "",
            "abc",
            &format!("{encoded}A"),
            &encoded.replace(&encoded[..1], "+"),
        ] {
            assert!(Secret::decode(bad).is_none());
        }
        assert_eq!(format!("{secret:?}"), "Secret(redacted)");
    }

    #[test]
    fn derived_values_differ_from_the_secret_and_each_other() {
        let secret = Secret::generate().unwrap();
        let encoded = secret.encode();
        let session = secret.csrf_token(CsrfPurpose::Session);
        let anonymous = secret.csrf_token(CsrfPurpose::Anonymous);
        assert_ne!(session, anonymous);
        assert_ne!(session, encoded);
        assert_eq!(secret.hash().len(), 32);
        assert_ne!(URL_SAFE_NO_PAD.encode(secret.hash()), encoded);
        assert_ne!(
            Secret::generate().unwrap().csrf_token(CsrfPurpose::Session),
            session
        );
    }
}
