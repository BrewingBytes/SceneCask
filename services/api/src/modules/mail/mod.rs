//! R07 durable email delivery: a PostgreSQL outbox drained by a leased, retrying worker over the
//! SMTP boundary in [`crate::mail`]. Payloads hold identifiers only; composers load current state
//! and mint any secrets at send time, so no token or message body is stored. Email bodies are
//! templates in `templates/` sharing one layout ([`template`]). Integration (R24)
//! builds a [`Worker`] from [`config::MailConfig`] and runs it alongside the API.

pub mod config;
pub mod outbox;
pub mod template;
mod worker;

pub use worker::{Compose, Composed, Pass, Worker};
