pub mod config;
pub mod error;
pub mod mail;
pub mod middleware;
pub mod modules;

use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

pub fn database_pool(url: &str) -> Result<PgPool, &'static str> {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(2))
        .connect_lazy(url)
        .map_err(|_| "DATABASE_URL must be a valid PostgreSQL connection URL")
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .with_state(pool)
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

fn health(status: StatusCode, message: &'static str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(Health { status: message }),
    )
        .into_response()
}

async fn live() -> Response {
    health(StatusCode::OK, "live")
}

async fn ready(State(pool): State<PgPool>) -> Response {
    match tokio::time::timeout(
        Duration::from_secs(3),
        sqlx::query("SELECT 1").execute(&pool),
    )
    .await
    {
        Ok(Ok(_)) => health(StatusCode::OK, "ready"),
        Ok(Err(error)) => not_ready(failure_kind(&error)),
        Err(_) => not_ready("timeout"),
    }
}

/// Logs only a fixed failure category; error text may contain connection details.
fn not_ready(reason: &'static str) -> Response {
    tracing::warn!(reason, "readiness check failed");
    health(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable")
}

/// Fixed category for a database error; sqlx error text can contain connection details.
pub(crate) fn failure_kind(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::PoolTimedOut => "pool_timeout",
        sqlx::Error::PoolClosed => "pool_closed",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::Tls(_) => "tls",
        sqlx::Error::Database(_) => "database",
        _ => "other",
    }
}
