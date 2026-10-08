//! SMTP settings from the environment. Errors name the variable, never its value.

use std::{env, net::IpAddr};

use lettre::message::Mailbox;

use crate::mail::SmtpMailer;

pub struct MailConfig {
    pub transport: SmtpMailer,
    pub from: Mailbox,
}

impl MailConfig {
    /// `SMTP_HOST`, `SMTP_PORT`, `MAIL_FROM` and, for a relay, `SMTP_USERNAME`/`SMTP_PASSWORD`
    /// (STARTTLS). Without credentials only a loopback capture server is accepted, and `MAIL_FROM`
    /// defaults to a local placeholder.
    pub fn from_env() -> Result<Self, &'static str> {
        let var = |name| env::var(name).ok().filter(|value| !value.trim().is_empty());
        Self::parse(
            var("SMTP_HOST"),
            var("SMTP_PORT"),
            var("SMTP_USERNAME"),
            var("SMTP_PASSWORD"),
            var("MAIL_FROM"),
        )
    }

    fn parse(
        host: Option<String>,
        port: Option<String>,
        username: Option<String>,
        password: Option<String>,
        from: Option<String>,
    ) -> Result<Self, &'static str> {
        let host = host.ok_or("SMTP_HOST is required")?;
        let port = port
            .ok_or("SMTP_PORT is required")?
            .parse()
            .map_err(|_| "SMTP_PORT must be a port number")?;
        let (transport, from) = match (username, password) {
            (Some(username), Some(password)) => (
                SmtpMailer::starttls(&host, port, username, password)?,
                from.ok_or("MAIL_FROM is required with SMTP credentials")?,
            ),
            (None, None) if is_loopback(&host) => (
                SmtpMailer::local_capture(&host, port),
                from.unwrap_or_else(|| "SceneCask <no-reply@localhost>".to_owned()),
            ),
            (None, None) => {
                return Err("SMTP_USERNAME and SMTP_PASSWORD are required off localhost");
            }
            _ => return Err("SMTP_USERNAME and SMTP_PASSWORD must be set together"),
        };
        Ok(Self {
            transport,
            from: from.parse().map_err(|_| "MAIL_FROM must be a mailbox")?,
        })
    }
}

fn is_loopback(host: &str) -> bool {
    host == "localhost" || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(
        host: &str,
        port: &str,
        credentials: Option<(&str, &str)>,
        from: Option<&str>,
    ) -> Result<MailConfig, &'static str> {
        let owned = |value: &str| Some(value.to_owned());
        MailConfig::parse(
            owned(host),
            owned(port),
            credentials.and_then(|(user, _)| owned(user)),
            credentials.and_then(|(_, password)| owned(password)),
            from.and_then(owned),
        )
    }

    #[test]
    fn local_capture_defaults_sender() {
        let config = parse("127.0.0.1", "1025", None, None).unwrap();
        assert_eq!(config.from.email.domain(), "localhost");
        assert!(parse("localhost", "1025", None, Some("Ana <a@b.test>")).is_ok());
    }

    #[test]
    fn rejects_unsafe_configuration_without_echoing_values() {
        for (result, message) in [
            (
                MailConfig::parse(None, Some("1".into()), None, None, None),
                "SMTP_HOST is required",
            ),
            (
                parse("127.0.0.1", "secret-value", None, None),
                "SMTP_PORT must be a port number",
            ),
            (
                parse("smtp.example.test", "587", None, None),
                "SMTP_USERNAME and SMTP_PASSWORD are required off localhost",
            ),
            (
                parse(
                    "smtp.example.test",
                    "587",
                    Some(("user", "secret-value")),
                    None,
                ),
                "MAIL_FROM is required with SMTP credentials",
            ),
            (
                parse("127.0.0.1", "1025", None, Some("secret-value")),
                "MAIL_FROM must be a mailbox",
            ),
        ] {
            assert_eq!(result.err(), Some(message));
        }
        let partial = MailConfig::parse(
            Some("127.0.0.1".into()),
            Some("1025".into()),
            Some("user".into()),
            None,
            None,
        );
        assert_eq!(
            partial.err(),
            Some("SMTP_USERNAME and SMTP_PASSWORD must be set together")
        );
    }
}
