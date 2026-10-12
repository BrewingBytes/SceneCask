//! The `AccountExport` document (C08): the owner's profile, library, progress, authored comments,
//! follow/block relationships and notifications, read from one consistent snapshot.
//!
//! Content is an allowlist: each section selects named columns of the owner's own rows. Password
//! hashes, sessions, tokens, OAuth state, identities, idempotency records, other people's comments
//! and blocks placed on the owner are never read, and no image path is copied (library posters are
//! the same derived URLs `GET /library` returns).

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::modules::{
    library::{dto::LibraryItem, repository},
    social::people::{self, Person, PersonRow, listed, person_columns},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountExport {
    pub profile: Profile,
    pub library: Vec<LibraryItem>,
    pub progress: Vec<Progress>,
    pub comments: Vec<Comment>,
    pub follows: Vec<Follow>,
    pub blocks: Vec<Block>,
    pub notifications: Vec<Notification>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub handle: Option<String>,
    pub visibility: String,
    pub verified: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub episode_id: Uuid,
    pub show_id: Uuid,
    pub watched: bool,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: Uuid,
    pub discussion_id: Uuid,
    pub body: Option<String>,
    pub created_at: String,
    pub removed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Follow {
    pub follower_id: Uuid,
    pub followee_id: Uuid,
    pub state: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub blocker_id: Uuid,
    pub blocked_id: Uuid,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: Uuid,
    pub kind: &'static str,
    pub actor: Person,
    pub created_at: String,
    pub read_at: Option<String>,
}

fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Reads `user_id`'s export at `now`; `None` if the account no longer exists. Run it inside one
/// read-only repeatable-read transaction so every section reflects the same moment.
pub async fn build(
    conn: &mut PgConnection,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Option<AccountExport>, sqlx::Error> {
    let Some(profile) = profile(conn, user_id).await? else {
        return Ok(None);
    };
    // Every show with an entry or retained progress, saved or not, so unsaved history and
    // archived episodes stay in the export (C02).
    let show_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT show_id FROM library_entries WHERE user_id = $1
         UNION
         SELECT e.show_id FROM episode_progress p JOIN episodes e ON e.id = p.episode_id
         WHERE p.user_id = $1
         ORDER BY 1",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?;
    let library = repository::items(conn, user_id, &show_ids, now).await?;
    Ok(Some(AccountExport {
        profile,
        library,
        progress: progress(conn, user_id).await?,
        comments: comments(conn, user_id).await?,
        follows: follows(conn, user_id).await?,
        blocks: blocks(conn, user_id).await?,
        notifications: notifications(conn, user_id).await?,
    }))
}

async fn profile(conn: &mut PgConnection, user_id: Uuid) -> Result<Option<Profile>, sqlx::Error> {
    type Row = (String, Option<String>, Option<String>, String, bool);
    let row: Option<Row> = sqlx::query_as(
        "SELECT normalized_email, display_name, handle, visibility, verified_at IS NOT NULL
         FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(conn)
    .await?;
    Ok(
        row.map(|(email, display_name, handle, visibility, verified)| {
            // The schema requires a name; before onboarding fall back as people cards do.
            let display_name = display_name
                .or_else(|| handle.clone())
                .unwrap_or_else(|| email.clone());
            Profile {
                id: user_id,
                email,
                display_name,
                handle,
                visibility,
                verified,
            }
        }),
    )
}

/// Every progress row, including `false` rows kept for revision provenance and archived
/// episodes, in season and episode order per show.
async fn progress(conn: &mut PgConnection, user_id: Uuid) -> Result<Vec<Progress>, sqlx::Error> {
    let rows: Vec<(Uuid, Uuid, bool, DateTime<Utc>)> = sqlx::query_as(
        "SELECT p.episode_id, e.show_id, p.watched, p.updated_at
         FROM episode_progress p
         JOIN episodes e ON e.id = p.episode_id
         JOIN seasons se ON se.id = e.season_id
         WHERE p.user_id = $1
         ORDER BY e.show_id, se.number, e.number, e.id",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(episode_id, show_id, watched, updated_at)| Progress {
            episode_id,
            show_id,
            watched,
            updated_at: timestamp(updated_at),
        })
        .collect())
}

/// Comments the owner wrote; a removed one stays as a bodyless tombstone.
async fn comments(conn: &mut PgConnection, user_id: Uuid) -> Result<Vec<Comment>, sqlx::Error> {
    type Row = (Uuid, Uuid, Option<String>, DateTime<Utc>, bool);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, discussion_id, CASE WHEN removed_at IS NULL THEN body END, created_at,
                removed_at IS NOT NULL
         FROM comments WHERE author_id = $1
         ORDER BY created_at, id",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, discussion_id, body, created_at, removed)| Comment {
            id,
            discussion_id,
            body,
            created_at: timestamp(created_at),
            removed,
        })
        .collect())
}

/// Follow edges in both directions: whom the owner follows or asked to, and who follows them.
async fn follows(conn: &mut PgConnection, user_id: Uuid) -> Result<Vec<Follow>, sqlx::Error> {
    let rows: Vec<(Uuid, Uuid, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT follower_id, followee_id, state, created_at FROM follows
         WHERE follower_id = $1 OR followee_id = $1
         ORDER BY created_at, follower_id, followee_id",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(follower_id, followee_id, state, created_at)| Follow {
            follower_id,
            followee_id,
            state,
            created_at: timestamp(created_at),
        })
        .collect())
}

/// Blocks the owner placed. Blocks placed on the owner belong to the blocker and are omitted.
async fn blocks(conn: &mut PgConnection, user_id: Uuid) -> Result<Vec<Block>, sqlx::Error> {
    let rows: Vec<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "SELECT blocked_id, created_at FROM blocks WHERE blocker_id = $1
         ORDER BY created_at, blocked_id",
    )
    .bind(user_id)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(blocked_id, created_at)| Block {
            blocker_id: user_id,
            blocked_id,
            created_at: timestamp(created_at),
        })
        .collect())
}

/// The owner's notifications as `GET /notifications` shows them: still-pending follow requests
/// from people the owner can currently see, with the requester's Person card.
async fn notifications(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<Vec<Notification>, sqlx::Error> {
    type Row = (
        Uuid,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Uuid,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        bool,
        bool,
    );
    let rows: Vec<Row> = sqlx::query_as(concat!(
        "SELECT n.id, n.created_at, n.read_at, ",
        person_columns!(),
        " FROM notifications n
         JOIN follows f ON f.follower_id = n.follow_requester_id AND f.followee_id = n.user_id
         JOIN users u ON u.id = n.actor_id
         WHERE n.user_id = $1 AND n.kind = 'follow_request' AND f.state = 'pending' AND ",
        listed!(),
        " ORDER BY n.created_at, n.id",
    ))
    .bind(user_id)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let (id, created_at, read_at, actor, handle, name, visibility, out, inc, vb, bb) = row;
            let person_row: PersonRow = (actor, handle, name, visibility, out, inc, vb, bb);
            Some(Notification {
                id,
                kind: "follow_request",
                actor: people::person(user_id, person_row)?,
                created_at: timestamp(created_at),
                read_at: read_at.map(timestamp),
            })
        })
        .collect())
}
