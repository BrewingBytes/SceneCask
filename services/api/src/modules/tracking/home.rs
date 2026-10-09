//! R18 Home (C05), relative to `/api/v1`: `GET /home`. Integration (R24/R38) mounts [`routes`]
//! behind the C03 security layers. Only saved Watching shows populate the sections, in saved_at,id
//! descending order, so the first Up next item is the featured one. `libraryEmpty` looks at every
//! saved show, so a library holding only on-hold, dropped or planned shows yields empty sections
//! with `libraryEmpty: false` (the "Choose a show to watch" state), never a false empty library.

use axum::{Json, Router, extract::State, routing::get};
use chrono::Utc;
use serde::Serialize;

use crate::{
    domain::progress::ProgressState,
    error::ApiError,
    middleware::Security,
    modules::{
        auth::session::VerifiedUser,
        library::{dto::LibraryItem, repository::saved_watching},
    },
};

pub fn routes(security: Security) -> Router {
    Router::new().route("/home", get(home)).with_state(security)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Home {
    /// Watching shows with a released unwatched regular episode.
    up_next: Vec<LibraryItem>,
    /// Watching shows with nothing released left to watch: caught up, completed, or waiting for a
    /// first or dated release.
    caught_up: Vec<LibraryItem>,
    library_empty: bool,
}

async fn home(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
) -> Result<Json<Home>, ApiError> {
    let mut conn = security.pool.acquire().await?;
    let (watching, any_saved) = saved_watching(&mut conn, current.user.id, Utc::now()).await?;
    let (up_next, caught_up) = watching
        .into_iter()
        .partition(|item| item.progress.state == ProgressState::InProgress.as_str());
    Ok(Json(Home {
        up_next,
        caught_up,
        library_empty: !any_saved,
    }))
}
