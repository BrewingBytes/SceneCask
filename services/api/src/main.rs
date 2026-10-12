use std::net::SocketAddr;

use scenecask_api::{
    app::{MetadataProvider, Services, application, run_workers},
    config::Config,
    database_pool,
    modules::{auth::google::Google, catalog::provider::tmdb::Tmdb, mail::config::MailConfig},
};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), &'static str> {
    tracing_subscriber::fmt().init();
    let config = Config::from_env()?;
    let mail = MailConfig::from_env()?;
    let pool = database_pool(&config.database_url)?;
    if config.migrate_on_start {
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|_| "database migrations failed; check DATABASE_URL and migration history")?;
    }
    let provider = match &config.tmdb {
        Some(tmdb) => MetadataProvider::Tmdb(
            match &tmdb.fixture_base {
                Some(base) => Tmdb::loopback_fixture(&tmdb.token, base),
                None => Tmdb::new(&tmdb.token),
            }
            .map_err(|_| "TMDB configuration is invalid")?,
        ),
        None => {
            tracing::warn!(
                "TMDB_API_TOKEN not set; search and import report the provider as unavailable"
            );
            MetadataProvider::Unconfigured
        }
    };
    let google = match config.google.clone() {
        Some(google) => Some(Google::new(google, &config.security.public_origin)?),
        None => {
            tracing::warn!("Google credentials not set; Google sign-in routes are not mounted");
            None
        }
    };
    let services = Services::new(pool, config.security, provider, google, config.beta);
    let listener = tokio::net::TcpListener::bind(config.bind_address)
        .await
        .map_err(|_| "API listener could not bind; check API_BIND and port availability")?;

    let (stop, stopped) = watch::channel(false);
    let workers = {
        let services = services.clone();
        tokio::spawn(async move { run_workers(&services, mail, stopped).await })
    };
    let served = axum::serve(
        listener,
        application(&services).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        signal().await;
        let _ = stop.send(true);
    })
    .await
    .map_err(|_| "API server stopped unexpectedly");
    let _ = workers.await;
    served
}

async fn signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
