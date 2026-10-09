//! C05 library DTOs. LibraryItem carries show-level data only: progress names the next episode by
//! position and never includes episode titles, overviews or stills.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::progress::{EpisodeCode, Progress},
    error::{ApiError, ErrorCode},
};

/// Manual library statuses. Caught up and Completed are computed, so they are never accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    PlanToWatch,
    Watching,
    OnHold,
    Dropped,
}

impl Status {
    pub const ALL: [Self; 4] = [
        Self::PlanToWatch,
        Self::Watching,
        Self::OnHold,
        Self::Dropped,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlanToWatch => "plan_to_watch",
            Self::Watching => "watching",
            Self::OnHold => "on_hold",
            Self::Dropped => "dropped",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }

    /// A submitted status, as a 422 field error when it is not a manual status (for example
    /// `completed`). The submitted value is never echoed.
    pub(super) fn from_request(value: &str) -> Result<Self, ApiError> {
        Self::parse(value).ok_or_else(|| {
            ApiError::new(ErrorCode::ValidationError).with_field(
                "status",
                "Choose Plan to watch, Watching, On hold or Dropped.",
            )
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeCodeDto {
    pub id: Uuid,
    pub season: i32,
    pub number: i32,
}

impl From<EpisodeCode> for EpisodeCodeDto {
    fn from(code: EpisodeCode) -> Self {
        Self {
            id: code.id,
            season: code.season,
            number: code.number,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub watched: u32,
    pub total: u32,
    pub percent: u8,
    pub state: String,
    pub next_episode: Option<EpisodeCodeDto>,
    pub out_of_order: bool,
    pub release_info_incomplete: bool,
}

impl From<Progress> for ProgressDto {
    fn from(progress: Progress) -> Self {
        Self {
            watched: progress.watched,
            total: progress.total,
            percent: progress.percent,
            state: progress.state.as_str().to_owned(),
            next_episode: progress.next_episode.map(Into::into),
            out_of_order: progress.out_of_order,
            release_info_incomplete: progress.release_info_incomplete,
        }
    }
}

/// One show from the viewer's own library. A show without an entry reads as unsaved
/// `plan_to_watch` at revision 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub show_id: Uuid,
    pub title: String,
    pub year: Option<i16>,
    pub poster_url: Option<String>,
    pub saved: bool,
    pub status: Status,
    pub revision: i64,
    pub progress: ProgressDto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub all: i64,
    pub watching: i64,
    pub plan_to_watch: i64,
    pub on_hold: i64,
    pub dropped: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPage {
    pub items: Vec<LibraryItem>,
    pub next_cursor: Option<String>,
    pub counts: Counts,
}

/// `PUT /library/{showId}`. Status stays a string so `completed` gets a field error.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveRequest {
    pub saved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub expected_revision: i64,
}

/// `PATCH /library/{showId}`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusRequest {
    pub status: String,
    pub expected_revision: i64,
}

pub(crate) fn expected_revision(value: i64) -> Result<i64, ApiError> {
    if value < 0 {
        return Err(ApiError::new(ErrorCode::ValidationError)
            .with_field("expectedRevision", "Use the revision you last loaded."));
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeChange {
    pub id: Uuid,
    pub watched: bool,
    pub revision: i64,
}

/// C05 MutationResult. A no-op has `changed: 0` and null `actionId`/`undoUntil`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub action_id: Option<Uuid>,
    /// RFC 3339 UTC.
    pub undo_until: Option<String>,
    pub changed: u32,
    pub library: LibraryItem,
    pub tracking_revision: i64,
    pub episodes: Vec<EpisodeChange>,
}

/// TMDB secure image base and poster size (catalog provider README: w500). Stored paths carry no
/// credential, and paths outside the provider's shape map to `None`.
const POSTER_BASE: &str = "https://image.tmdb.org/t/p/w500";

pub(super) fn poster_url(path: Option<&str>) -> Option<String> {
    let rest = path?.strip_prefix('/')?;
    let safe = !rest.is_empty()
        && rest.len() < 255
        && !rest.contains("..")
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    safe.then(|| format!("{POSTER_BASE}/{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_manual_statuses_parse() {
        for status in Status::ALL {
            assert_eq!(Status::parse(status.as_str()), Some(status));
            assert_eq!(
                serde_json::to_value(status).unwrap(),
                serde_json::json!(status.as_str())
            );
        }
        for computed in ["completed", "caught_up", "Watching", ""] {
            assert!(Status::from_request(computed).is_err(), "{computed}");
        }
    }

    #[test]
    fn poster_urls_use_the_secure_base_and_reject_unsafe_paths() {
        assert_eq!(
            poster_url(Some("/abc_1-2.jpg")).as_deref(),
            Some("https://image.tmdb.org/t/p/w500/abc_1-2.jpg")
        );
        for unsafe_path in [
            None,
            Some(""),
            Some("/"),
            Some("abc.jpg"),
            Some("/../x.jpg"),
            Some("//evil.example/x.jpg"),
            Some("/a?api_key=1"),
        ] {
            assert_eq!(poster_url(unsafe_path), None, "{unsafe_path:?}");
        }
    }
}
