pub mod config;
pub mod mail;

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
        _ => health(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"),
    }
}
