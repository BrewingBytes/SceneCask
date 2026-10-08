//! Drains the outbox: claim due messages, compose each at send time, deliver over SMTP, then
//! settle or schedule a retry. Logs carry only the message kind and fixed failure categories.

use std::{future::Future, time::Duration};

use lettre::Message;
use sqlx::PgPool;

use super::outbox::{self, Claimed};
use crate::{failure_kind, mail::MailTransport};

/// What to do with a claimed message.
pub enum Composed {
    Send(Message),
    /// Nothing to send any more (for example, the account was verified meanwhile); settle it.
    Skip,
}

/// Builds the email for a claimed message from current state. Errors are fixed categories and
/// are retried like delivery failures.
pub trait Compose {
    fn compose(
        &self,
        message: &Claimed,
    ) -> impl Future<Output = Result<Composed, &'static str>> + Send;
}

/// Outcome counts of one [`Worker::run_once`] pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pass {
    pub sent: usize,
    pub skipped: usize,
    pub failed: usize,
}

pub struct Worker<T, C> {
    pool: PgPool,
    transport: T,
    composer: C,
    batch: i64,
    poll: Duration,
}

impl<T: MailTransport + Sync, C: Compose + Sync> Worker<T, C> {
    pub fn new(pool: PgPool, transport: T, composer: C) -> Self {
        Self {
            pool,
            transport,
            composer,
            batch: 10,
            poll: Duration::from_secs(5),
        }
    }

    /// Processes up to one batch of due messages. Each is claimed just before it is composed and
    /// sent, so its lease covers one send rather than a whole batch of them.
    pub async fn run_once(&self) -> Result<Pass, sqlx::Error> {
        let mut pass = Pass::default();
        for _ in 0..self.batch {
            let Some(message) = outbox::claim(&self.pool, 1, outbox::LEASE).await?.pop() else {
                break;
            };
            let result = match self.composer.compose(&message).await {
                Ok(Composed::Send(email)) => self.transport.send(email).await.map(|()| true),
                Ok(Composed::Skip) => Ok(false),
                Err(category) => Err(category),
            };
            match result {
                Ok(sent) => {
                    outbox::settle(&self.pool, &message).await?;
                    if sent {
                        pass.sent += 1;
                    } else {
                        pass.skipped += 1;
                    }
                }
                Err(category) => {
                    pass.failed += 1;
                    self.fail(&message, category).await?;
                }
            }
        }
        Ok(pass)
    }

    async fn fail(&self, message: &Claimed, category: &'static str) -> Result<(), sqlx::Error> {
        let kind = message.kind.as_str();
        if message.attempts >= outbox::MAX_ATTEMPTS {
            tracing::error!(kind, category, "outbox message exhausted its retries");
            return Ok(());
        }
        tracing::warn!(
            kind,
            category,
            attempts = message.attempts,
            "outbox delivery failed"
        );
        outbox::retry_later(&self.pool, message, outbox::backoff(message.attempts)).await
    }

    /// Polls until `shutdown` resolves. Database failures are logged by category and retried on
    /// the next poll.
    pub async fn run(self, shutdown: impl Future<Output = ()>) {
        tokio::pin!(shutdown);
        loop {
            if let Err(error) = self.run_once().await {
                tracing::warn!(category = failure_kind(&error), "outbox poll failed");
            }
            tokio::select! {
                () = &mut shutdown => return,
                () = tokio::time::sleep(self.poll) => {}
            }
        }
    }
}
