//! Process configuration from the environment. Errors name the variable, never its value.

use std::{env, net::SocketAddr};

use crate::{
    middleware::SecurityConfig,
    modules::auth::google::{GOOGLE_ISSUER, GoogleConfig},
};

pub struct Config {
    pub database_url: String,
    pub bind_address: SocketAddr,
    /// The browser-facing origin that serves the web app and proxies `/api/v1` (`PUBLIC_ORIGIN`).
    pub security: SecurityConfig,
    /// `SCENECASK_BETA`: social beta routes and UI. Off unless explicitly `true`.
    pub beta: bool,
    /// `API_MIGRATE_ON_START`: apply pending migrations before serving (development and CI).
    pub migrate_on_start: bool,
    pub tmdb: Option<TmdbConfig>,
    pub google: Option<GoogleConfig>,
}

/// TMDB read-access token, with an optional loopback fixture base for browser journeys.
pub struct TmdbConfig {
    pub token: String,
    pub fixture_base: Option<String>,
}

/// The variables [`Config`] reads; tests supply them directly.
#[derive(Default)]
struct Vars {
    database_url: Option<String>,
    bind: Option<String>,
    public_origin: Option<String>,
    beta: Option<String>,
    migrate: Option<String>,
    tmdb_token: Option<String>,
    tmdb_base: Option<String>,
    google_id: Option<String>,
    google_secret: Option<String>,
    google_issuer: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, &'static str> {
        let var = |name| env::var(name).ok().filter(|value| !value.trim().is_empty());
        Self::parse(Vars {
            database_url: var("DATABASE_URL"),
            bind: var("API_BIND"),
            public_origin: var("PUBLIC_ORIGIN"),
            beta: var("SCENECASK_BETA"),
            migrate: var("API_MIGRATE_ON_START"),
            tmdb_token: var("TMDB_API_TOKEN"),
            tmdb_base: var("TMDB_FIXTURE_BASE"),
            google_id: var("GOOGLE_CLIENT_ID"),
            google_secret: var("GOOGLE_CLIENT_SECRET"),
            google_issuer: var("GOOGLE_FIXTURE_ISSUER"),
        })
    }

    fn parse(vars: Vars) -> Result<Self, &'static str> {
        let database_url = vars
            .database_url
            .filter(|value| !value.trim().is_empty())
            .ok_or("DATABASE_URL is required")?;
        let bind_address = vars
            .bind
            .unwrap_or_else(|| "127.0.0.1:8080".to_owned())
            .parse()
            .map_err(|_| "API_BIND must be an IP address and port")?;
        let security = SecurityConfig::new(
            vars.public_origin
                .as_deref()
                .unwrap_or("http://127.0.0.1:3000"),
        )
        .map_err(|_| "PUBLIC_ORIGIN must be an https origin, or http on localhost")?;
        let tmdb = match (vars.tmdb_token, vars.tmdb_base) {
            (Some(token), fixture_base) => Some(TmdbConfig {
                token,
                fixture_base,
            }),
            (None, Some(_)) => return Err("TMDB_FIXTURE_BASE requires TMDB_API_TOKEN"),
            (None, None) => None,
        };
        let google = match (vars.google_id, vars.google_secret) {
            (Some(client_id), Some(client_secret)) => Some(GoogleConfig {
                client_id,
                client_secret,
                issuer: match vars.google_issuer {
                    Some(issuer) if loopback_http(&issuer) => issuer,
                    Some(_) => {
                        return Err("GOOGLE_FIXTURE_ISSUER must be an http loopback origin");
                    }
                    None => GOOGLE_ISSUER.to_owned(),
                },
            }),
            (None, None) if vars.google_issuer.is_none() => None,
            (None, None) => return Err("GOOGLE_FIXTURE_ISSUER requires Google credentials"),
            _ => return Err("GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET must be set together"),
        };
        Ok(Self {
            database_url,
            bind_address,
            security,
            beta: flag(vars.beta.as_deref(), "SCENECASK_BETA")?,
            migrate_on_start: flag(vars.migrate.as_deref(), "API_MIGRATE_ON_START")?,
            tmdb,
            google,
        })
    }
}

fn flag(value: Option<&str>, name: &'static str) -> Result<bool, &'static str> {
    match value.map(str::trim) {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        Some(_) => Err(match name {
            "SCENECASK_BETA" => "SCENECASK_BETA must be true or false",
            _ => "API_MIGRATE_ON_START must be true or false",
        }),
    }
}

/// `http://localhost[:port]` or an `http` loopback IP, with no path.
fn loopback_http(origin: &str) -> bool {
    let Some(authority) = origin.strip_prefix("http://") else {
        return false;
    };
    let host = match authority.rfind(':') {
        Some(colon) if !authority.ends_with(']') => &authority[..colon],
        _ => authority,
    };
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        Vars {
            database_url: Some("postgres://localhost/scenecask".into()),
            ..Vars::default()
        }
    }

    #[test]
    fn rejects_missing_configuration_without_values() {
        assert!(matches!(
            Config::parse(Vars::default()),
            Err("DATABASE_URL is required")
        ));
        let invalid = [
            Vars {
                bind: Some("secret".into()),
                ..vars()
            },
            Vars {
                public_origin: Some("http://secret.example".into()),
                ..vars()
            },
            Vars {
                beta: Some("secret".into()),
                ..vars()
            },
            Vars {
                google_id: Some("secret".into()),
                ..vars()
            },
            Vars {
                tmdb_base: Some("secret".into()),
                ..vars()
            },
            Vars {
                google_id: Some("id".into()),
                google_secret: Some("secret".into()),
                google_issuer: Some("https://secret.example".into()),
                ..vars()
            },
        ];
        for vars in invalid {
            let error = Config::parse(vars).err().unwrap();
            assert!(!error.contains("secret"), "{error}");
        }
    }

    #[test]
    fn defaults_keep_beta_and_providers_off() {
        let config = Config::parse(vars()).unwrap();
        assert!(!config.beta);
        assert!(!config.migrate_on_start);
        assert!(config.tmdb.is_none() && config.google.is_none());
        assert_eq!(config.security.public_origin, "http://127.0.0.1:3000");
        assert_eq!(config.bind_address.to_string(), "127.0.0.1:8080");
    }

    #[test]
    fn google_uses_the_real_issuer_unless_a_loopback_fixture_is_named() {
        let google = |issuer: Option<&str>| {
            Config::parse(Vars {
                google_id: Some("id".into()),
                google_secret: Some("secret".into()),
                google_issuer: issuer.map(Into::into),
                ..vars()
            })
            .map(|config| config.google.unwrap().issuer)
        };
        assert_eq!(google(None).unwrap(), GOOGLE_ISSUER);
        assert_eq!(
            google(Some("http://127.0.0.1:4011")).unwrap(),
            "http://127.0.0.1:4011"
        );
        assert!(google(Some("http://localhost:4011")).is_ok());
        assert!(google(Some("http://accounts.example")).is_err());
    }
}
