use std::{env, net::SocketAddr};

pub struct Config {
    pub database_url: String,
    pub bind_address: SocketAddr,
}

impl Config {
    pub fn from_env() -> Result<Self, &'static str> {
        Self::parse(env::var("DATABASE_URL").ok(), env::var("API_BIND").ok())
    }

    fn parse(database_url: Option<String>, bind: Option<String>) -> Result<Self, &'static str> {
        let database_url = database_url
            .filter(|value| !value.trim().is_empty())
            .ok_or("DATABASE_URL is required")?;
        let bind_address = bind
            .unwrap_or_else(|| "127.0.0.1:8080".to_owned())
            .parse()
            .map_err(|_| "API_BIND must be an IP address and port")?;
        Ok(Self {
            database_url,
            bind_address,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_missing_configuration_without_values() {
        assert!(matches!(
            Config::parse(None, None),
            Err("DATABASE_URL is required")
        ));
        assert!(Config::parse(Some(" ".into()), None).is_err());
        assert!(matches!(
            Config::parse(Some("secret".into()), Some("secret".into())),
            Err("API_BIND must be an IP address and port")
        ));
    }
}
