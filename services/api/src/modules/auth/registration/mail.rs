//! The verification email. Each send mints a fresh token, so a retried or resent message
//! invalidates the links sent before it. The link carries the token in the fragment, which
//! browsers never send to a server or in a Referer.

use lettre::message::Mailbox;
use sqlx::PgPool;
use uuid::Uuid;

use super::{VERIFY_KIND, store};
use crate::{
    failure_kind,
    modules::{
        auth::session::secret::Secret,
        mail::{Compose, Composed, outbox::Claimed, template::VERIFY_EMAIL},
    },
};

pub struct VerificationMail {
    pool: PgPool,
    public_origin: String,
    from: Mailbox,
}

impl VerificationMail {
    /// `public_origin` is the validated origin the web app is served from
    /// (`SecurityConfig::public_origin`).
    pub fn new(pool: PgPool, public_origin: String, from: Mailbox) -> Self {
        Self {
            pool,
            public_origin,
            from,
        }
    }
}

impl Compose for VerificationMail {
    async fn compose(&self, message: &Claimed) -> Result<Composed, &'static str> {
        if message.kind != VERIFY_KIND {
            return Err("unknown_kind");
        }
        let user_id = message
            .payload
            .get("userId")
            .and_then(|value| value.as_str())
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or("invalid_payload")?;
        let token = Secret::generate().map_err(|_| "entropy")?;
        let Some(email) = store::mint_token(&self.pool, user_id, &token.hash())
            .await
            .map_err(|error| failure_kind(&error))?
        else {
            return Ok(Composed::Skip);
        };
        let Ok(to) = email.parse::<Mailbox>() else {
            // Registration accepts only parseable addresses; nothing a retry could fix.
            tracing::warn!(
                kind = VERIFY_KIND,
                "verification recipient is not a mailbox"
            );
            return Ok(Composed::Skip);
        };
        let link = format!(
            "{}/auth/verify#token={}",
            self.public_origin,
            token.encode()
        );
        VERIFY_EMAIL
            .render(&self.public_origin, &[("link", &link)])?
            .into_message(self.from.clone(), to)
            .map(Composed::Send)
    }
}
