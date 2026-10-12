//! R25 people (C06): `GET /people`, `GET /people/{id}` and `GET /people/{id}/library`. A listed
//! person is verified, enabled and has a handle; a block in either direction conceals them. The
//! profile card is discoverable by any signed-in member, while the library needs
//! [`access::can_view_library`]. Search matches handles and display names only, never email.

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    routing::get,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::{
    domain::access::{
        self, FollowState, Relationship, RelationshipView, Subject, Viewer, Visibility,
    },
    error::{ApiError, ErrorCode},
    middleware::{Security, rate_limit::SEARCH_RULES},
    modules::{
        auth::session::VerifiedUser,
        library::{
            decode_cursor, decode_opaque,
            dto::{ProgressDto, Status},
            encode_opaque, page_limit,
            repository::{self, PageQuery, escape_like},
            split_page,
        },
        policy::relationships,
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route("/people", get(search))
        .route("/people/{id}", get(profile))
        .route("/people/{id}/library", get(person_library))
        .with_state(security)
}

const DEFAULT_LIMIT: i64 = 20;

/// The C06 Person card. `relationship` describes the viewer's edges with this person only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub initials: String,
    pub visibility: &'static str,
    pub relationship: RelationshipView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeoplePage {
    pub items: Vec<Person>,
    pub next_cursor: Option<String>,
}

/// Person columns for `u`, as seen by the viewer bound to `$1`. Use with [`PersonRow`].
macro_rules! person_columns {
    () => {
        "u.id, u.handle, u.display_name, u.visibility,
         (SELECT state FROM follows WHERE follower_id = $1 AND followee_id = u.id),
         (SELECT state FROM follows WHERE follower_id = u.id AND followee_id = $1),
         EXISTS (SELECT 1 FROM blocks WHERE blocker_id = $1 AND blocked_id = u.id),
         EXISTS (SELECT 1 FROM blocks WHERE blocker_id = u.id AND blocked_id = $1)"
    };
}

/// Whether `u` can appear in the viewer's (`$1`) people lists at all.
macro_rules! listed {
    () => {
        "u.handle IS NOT NULL AND u.verified_at IS NOT NULL AND u.disabled_at IS NULL
         AND NOT EXISTS (SELECT 1 FROM blocks b
                         WHERE (b.blocker_id = $1 AND b.blocked_id = u.id)
                            OR (b.blocker_id = u.id AND b.blocked_id = $1))"
    };
}

pub(crate) use {listed, person_columns};

/// A row selected with [`person_columns`].
pub(crate) type PersonRow = (
    Uuid,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    bool,
    bool,
);

fn follow_state(value: Option<&str>) -> FollowState {
    match value {
        Some("approved") => FollowState::Approved,
        Some("pending") => FollowState::Pending,
        _ => FollowState::None,
    }
}

/// The card `viewer` may see for `row`, or `None` when the policy conceals that person.
pub(crate) fn person(viewer: Uuid, row: PersonRow) -> Option<Person> {
    let (id, handle, display_name, visibility, outgoing, incoming, viewer_blocks, blocked_by) = row;
    let relationship = Relationship {
        outgoing: follow_state(outgoing.as_deref()),
        incoming: follow_state(incoming.as_deref()),
        viewer_blocks,
        blocked_by,
    };
    card(
        viewer,
        (id, handle, display_name, visibility),
        &relationship,
    )
}

/// A person's own columns: id, handle, display name and visibility.
type CardRow = (Uuid, Option<String>, Option<String>, String);

fn card(viewer: Uuid, row: CardRow, relationship: &Relationship) -> Option<Person> {
    let (id, handle, display_name, visibility) = row;
    if !access::can_see_person(Viewer::Member(viewer), id, relationship) {
        return None;
    }
    let handle = handle?;
    let display_name = display_name.unwrap_or_else(|| handle.clone());
    Some(Person {
        id,
        initials: initials(&display_name),
        handle,
        display_name,
        visibility: if visibility == "public" {
            "public"
        } else {
            "private"
        },
        relationship: relationship.view(),
    })
}

/// Up to two uppercase initials: the first letters of the first and last words.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let first = words.first().and_then(|word| word.chars().next());
    let last = (words.len() > 1)
        .then(|| words.last().and_then(|word| word.chars().next()))
        .flatten();
    first
        .into_iter()
        .chain(last)
        .filter_map(|c| c.to_uppercase().next())
        .collect()
}

/// A path ID that is not a UUID cannot name a person: 404, like an unknown one.
pub(super) fn path_id(value: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(value).map_err(|_| ApiError::not_found())
}

/// A listed person and their visibility, with the privacy decision made from the current edges.
/// `None` when the person is unknown, unlisted or concealed from `viewer`.
pub(super) async fn subject(
    conn: &mut PgConnection,
    viewer: Uuid,
    id: Uuid,
) -> Result<Option<(Person, Subject, Relationship)>, sqlx::Error> {
    let row: Option<CardRow> = sqlx::query_as(
        "SELECT id, handle, display_name, visibility FROM users
         WHERE id = $1 AND handle IS NOT NULL AND verified_at IS NOT NULL
           AND disabled_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let subject = Subject {
        id,
        visibility: if row.3 == "public" {
            Visibility::Public
        } else {
            Visibility::Private
        },
    };
    let relationship = relationships::relationship(conn, viewer, id).await?;
    Ok(card(viewer, row, &relationship).map(|person| (person, subject, relationship)))
}

#[derive(Deserialize)]
struct SearchParams {
    q: Option<String>,
    cursor: Option<String>,
    limit: Option<String>,
}

async fn search(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    params: Result<Query<SearchParams>, QueryRejection>,
) -> Result<Json<PeoplePage>, ApiError> {
    let viewer = current.user.id;
    security
        .limiter
        .check(&format!("people.search:{viewer}"), &SEARCH_RULES)?;
    let Query(params) = params.map_err(|_| ApiError::malformed())?;
    let q = params.q.as_deref().map(str::trim).unwrap_or_default();
    if !(2..=100).contains(&q.chars().count()) {
        return Err(
            ApiError::new(ErrorCode::ValidationError).with_field("q", "Use 2 to 100 characters.")
        );
    }
    let limit = page_limit(params.limit.as_deref(), DEFAULT_LIMIT)?;
    let after = params
        .cursor
        .as_deref()
        .map(|cursor| {
            decode_opaque(cursor, |text| {
                let (handle, id) = text.split_once('.')?;
                Some((handle.to_owned(), Uuid::try_parse(id).ok()?))
            })
        })
        .transpose()?;
    let pattern = format!("%{}%", escape_like(q));
    let mut conn = security.pool.acquire().await?;
    let rows: Vec<PersonRow> = sqlx::query_as(concat!(
        "SELECT ",
        person_columns!(),
        " FROM users u
         WHERE ",
        listed!(),
        " AND u.id <> $1
           AND (u.handle ILIKE $2 ESCAPE '\\' OR u.display_name ILIKE $2 ESCAPE '\\')
           AND ($3::text IS NULL OR (u.handle, u.id) > ($3, $4))
         ORDER BY u.handle, u.id
         LIMIT $5"
    ))
    .bind(viewer)
    .bind(pattern)
    .bind(after.as_ref().map(|(handle, _)| handle.as_str()))
    .bind(after.as_ref().map(|(_, id)| *id))
    .bind(limit + 1)
    .fetch_all(&mut *conn)
    .await?;
    let mut items: Vec<Person> = rows
        .into_iter()
        .filter_map(|row| person(viewer, row))
        .collect();
    let next_cursor = if items.len() as i64 > limit {
        items.truncate(limit as usize);
        items
            .last()
            .map(|last| encode_opaque(&format!("{}.{}", last.handle, last.id.simple())))
    } else {
        None
    };
    Ok(Json(PeoplePage { items, next_cursor }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersonResult {
    person: Person,
    can_view_library: bool,
}

async fn profile(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
) -> Result<Json<PersonResult>, ApiError> {
    let viewer = current.user.id;
    let id = path_id(&id)?;
    let mut conn = security.pool.acquire().await?;
    let (person, subject, relationship) = subject(&mut conn, viewer, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(PersonResult {
        person,
        can_view_library: access::can_view_library(Viewer::Member(viewer), subject, &relationship),
    }))
}

/// A saved show in someone else's library: their progress, without internal revisions. Like
/// LibraryItem it never carries episode titles, overviews or stills.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VisibleLibraryItem {
    show_id: Uuid,
    title: String,
    year: Option<i16>,
    poster_url: Option<String>,
    saved: bool,
    status: Status,
    progress: ProgressDto,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VisibleLibraryPage {
    items: Vec<VisibleLibraryItem>,
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct PageParams {
    cursor: Option<String>,
    limit: Option<String>,
}

async fn person_library(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    params: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<VisibleLibraryPage>, ApiError> {
    let viewer = current.user.id;
    let id = path_id(&id)?;
    let Query(params) = params.map_err(|_| ApiError::malformed())?;
    let mut conn = security.pool.acquire().await?;
    // Privacy first: a forbidden library is concealed before the parameters are judged.
    let (_, subject, relationship) = subject(&mut conn, viewer, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if !access::can_view_library(Viewer::Member(viewer), subject, &relationship) {
        return Err(ApiError::not_found());
    }
    let limit = page_limit(params.limit.as_deref(), DEFAULT_LIMIT)?;
    let after = params.cursor.as_deref().map(decode_cursor).transpose()?;
    let positions = repository::page(
        &mut conn,
        id,
        &PageQuery {
            status: None,
            q: None,
            after,
            limit,
        },
    )
    .await?;
    let (ids, next_cursor) = split_page(positions, limit);
    let items = repository::items(&mut conn, id, &ids, Utc::now())
        .await?
        .into_iter()
        .map(|item| VisibleLibraryItem {
            show_id: item.show_id,
            title: item.title,
            year: item.year,
            poster_url: item.poster_url,
            saved: item.saved,
            status: item.status,
            progress: item.progress,
        })
        .collect();
    Ok(Json(VisibleLibraryPage { items, next_cursor }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_use_first_and_last_words() {
        assert_eq!(initials("Ana"), "A");
        assert_eq!(initials("ana maria  rusu"), "AR");
        assert_eq!(initials("  "), "");
        assert_eq!(initials("éva ßeta"), "ÉS");
    }

    #[test]
    fn concealed_or_handleless_rows_have_no_card() {
        let viewer = Uuid::from_u128(1);
        let row = |handle: Option<&str>, viewer_blocks, blocked_by| -> PersonRow {
            (
                Uuid::from_u128(2),
                handle.map(Into::into),
                None,
                "private".into(),
                Some("pending".into()),
                None,
                viewer_blocks,
                blocked_by,
            )
        };
        let card = person(viewer, row(Some("ana_r"), false, false)).unwrap();
        assert_eq!(card.display_name, "ana_r");
        assert_eq!(card.relationship.outgoing, FollowState::Pending);
        assert!(person(viewer, row(None, false, false)).is_none());
        assert!(person(viewer, row(Some("ana_r"), true, false)).is_none());
        assert!(person(viewer, row(Some("ana_r"), false, true)).is_none());
    }
}
