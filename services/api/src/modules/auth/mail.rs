//! Composes the account emails (verification and password reset) for the outbox worker. Each
//! send mints a fresh token, so a retried or resent message invalidates the links sent before it.

use lettre::message::Mailbox;
use sqlx::PgPool;
use uuid::Uuid;

use super::{email_token, session::secret::Secret};
use crate::{
    failure_kind,
    modules::mail::{Compose, Composed, outbox::Claimed},
};

pub struct AuthMail {
    pool: PgPool,
    public_origin: String,
    from: Mailbox,
}

impl AuthMail {
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

impl Compose for AuthMail {
    async fn compose(&self, message: &Claimed) -> Result<Composed, &'static str> {
        let email_token = email_token::ALL
            .into_iter()
            .find(|candidate| candidate.kind == message.kind)
            .ok_or("unknown_kind")?;
        let user_id = message
            .payload
            .get("userId")
            .and_then(|value| value.as_str())
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or("invalid_payload")?;
        let token = Secret::generate().map_err(|_| "entropy")?;
        let Some(email) = email_token
            .mint(&self.pool, user_id, &token.hash())
            .await
            .map_err(|error| failure_kind(&error))?
        else {
            return Ok(Composed::Skip);
        };
        let Ok(to) = email.parse::<Mailbox>() else {
            // Registration accepts only parseable addresses; nothing a retry could fix.
            tracing::warn!(
                kind = email_token.kind,
                "account email recipient is not a mailbox"
            );
            return Ok(Composed::Skip);
        };
        let link = format!(
            "{}{}#token={}",
            self.public_origin,
            email_token.path,
            token.encode()
        );
        email_token
            .template
            .render(&self.public_origin, &[("link", &link)])?
            .into_message(self.from.clone(), to)
            .map(Composed::Send)
    }
}
