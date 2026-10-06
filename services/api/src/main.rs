use scenecask_api::{config::Config, database_pool, router};

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
    let config = Config::from_env()?;
    let pool = database_pool(&config.database_url)?;
    let listener = tokio::net::TcpListener::bind(config.bind_address)
        .await
        .map_err(|_| "API listener could not bind; check API_BIND and port availability")?;
    axum::serve(listener, router(pool))
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|_| "API server stopped unexpectedly")
}

async fn shutdown() {
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
