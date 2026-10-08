//! SMTP boundary for future outbox delivery. No message bodies or transport errors are logged.
use std::{future::Future, time::Duration};

use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

pub trait MailTransport {
    fn send(&self, message: Message) -> impl Future<Output = Result<(), &'static str>> + Send;
}

pub struct SmtpMailer(AsyncSmtpTransport<Tokio1Executor>);

impl SmtpMailer {
    /// Unencrypted local capture only. Deployment must use the STARTTLS constructor.
    pub fn local_capture(host: &str, port: u16) -> Self {
        Self(
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
                .port(port)
                .timeout(Some(Duration::from_secs(5)))
                .build(),
        )
    }

    pub fn starttls(
        host: &str,
        port: u16,
        username: String,
        password: String,
    ) -> Result<Self, &'static str> {
        let builder = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
            .map_err(|_| "invalid SMTP relay configuration")?;
        Ok(Self(
            builder
                .port(port)
                .credentials(lettre::transport::smtp::authentication::Credentials::new(
                    username, password,
                ))
                .timeout(Some(Duration::from_secs(5)))
                .build(),
        ))
    }
}

impl MailTransport for SmtpMailer {
    async fn send(&self, message: Message) -> Result<(), &'static str> {
        self.0
            .send(message)
            .await
            .map(|_| ())
            .map_err(|_| "email delivery unavailable")
    }
}
