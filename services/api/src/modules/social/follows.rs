//! R25 follows (C06): `POST|DELETE /people/{id}/follow`, `GET /me/follow-requests`,
//! `POST /me/follow-requests/{requesterId}`, `GET /me/followers` and `DELETE /me/followers/{id}`.
//! A follow of a private account is a pending request until approved; a public account is
//! followed immediately. A new pending request creates one in-app notification and one outbox
//! event in the same transaction; repeats find the existing edge and create nothing. Removing an
//! edge cascades to its notification, and decline, removal and unfollow notify nobody.
//!
//! Every write locks both users' rows in ID order with [`lock_pair`] before reading the edges, so
//! concurrent follows, decisions, privacy changes and blocks of one pair serialize. Social writes
//! need the caller's handle.

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::QueryRejection},
    http::StatusCode,
    routing::{delete, get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

use super::people::{self, PeoplePage, Person, PersonRow, listed, path_id, person_columns};
use crate::{
    domain::access::{FollowState, Visibility},
    error::{ApiError, ErrorCode},
    middleware::{
        Security,
        json::{ApiJson, EmptyRequest},
    },
    modules::{
        auth::session::{CurrentSession, VerifiedUser},
        library::{decode_opaque, encode_opaque, page_limit},
        mail::outbox,
    },
};

pub fn routes(security: Security) -> Router {
    Router::new()
        .route(
            "/people/{id}/follow",
            post(follow).delete(remove_edge::<false>),
        )
        .route("/me/follow-requests", get(incoming::<true>))
        .route("/me/follow-requests/{requesterId}", post(decide))
        .route("/me/followers", get(incoming::<false>))
        .route("/me/followers/{id}", delete(remove_edge::<true>))
        .with_state(security)
}

/// The outbox kind of a new pending follow request; its payload names only the notification.
pub const FOLLOW_REQUEST_EVENT: &str = "notification.follow_request";

const DEFAULT_LIMIT: i64 = 20;

/// Locks both users' rows in ID order for the rest of the transaction. Social writes on a pair
/// (follows, decisions, removal, and R26 privacy changes and blocks) take this lock first, so their
/// edge reads and writes cannot interleave.
pub async fn lock_pair(conn: &mut PgConnection, a: Uuid, b: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT 1 FROM users WHERE id = ANY($1) ORDER BY id FOR NO KEY UPDATE")
        .bind([a, b])
        .execute(conn)
        .await
        .map(drop)
}

/// Deletes each user's discussion reveal grants for threads the other hosts. Removing an edge
/// ends their mutuality, so a later follow that restores it must not reopen a thread revealed
/// before (C09 #7). R26 blocks take the same step under [`lock_pair`].
pub async fn revoke_discussion_grants(
    conn: &mut PgConnection,
    a: Uuid,
    b: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM reveal_grants g USING discussions d
         WHERE g.scope = 'discussion' AND g.resource_id = d.id
           AND ((g.user_id = $1 AND d.host_id = $2) OR (g.user_id = $2 AND d.host_id = $1))",
    )
    .bind(a)
    .bind(b)
    .execute(conn)
    .await
    .map(drop)
}

/// 422 until the caller has chosen a handle (C06 social writes).
fn require_handle(current: &CurrentSession) -> Result<Uuid, ApiError> {
    current
        .user
        .handle
        .as_ref()
        .map(|_| current.user.id)
        .ok_or_else(|| {
            ApiError::new(ErrorCode::ValidationError)
                .with_field("handle", "Choose a handle before using social features.")
        })
}

#[derive(Debug, Serialize)]
struct FollowResult {
    state: FollowState,
}

async fn follow(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    ApiJson(EmptyRequest {}): ApiJson<EmptyRequest>,
) -> Result<Json<FollowResult>, ApiError> {
    let target = path_id(&id)?;
    let viewer = require_handle(&current)?;
    if target == viewer {
        return Err(ApiError::new(ErrorCode::ValidationError)
            .with_field("id", "You can't follow yourself."));
    }
    let mut tx = security.pool.begin().await?;
    lock_pair(&mut tx, viewer, target).await?;
    let (_, subject, relationship) = people::subject(&mut tx, viewer, target)
        .await?
        .ok_or_else(ApiError::not_found)?;
    // A repeat returns the existing edge; a pending request stays pending even if the account
    // has since become public (C06: no silent conversion).
    if relationship.outgoing != FollowState::None {
        return Ok(Json(FollowResult {
            state: relationship.outgoing,
        }));
    }
    let state = match subject.visibility {
        Visibility::Public => FollowState::Approved,
        Visibility::Private => FollowState::Pending,
    };
    sqlx::query(
        "INSERT INTO follows (follower_id, followee_id, state) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(viewer)
    .bind(target)
    .bind(if state == FollowState::Approved {
        "approved"
    } else {
        "pending"
    })
    .execute(&mut *tx)
    .await?;
    if state == FollowState::Pending {
        let notification: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO notifications (user_id, kind, actor_id) VALUES ($1, 'follow_request', $2)
             ON CONFLICT ON CONSTRAINT notifications_follow_request_key DO NOTHING
             RETURNING id",
        )
        .bind(target)
        .bind(viewer)
        .fetch_optional(&mut *tx)
        .await?;
        // Each request edge has one notification, so keying the event on it lets a request made
        // after a decline or withdrawal notify again while retries of one request cannot.
        if let Some(notification) = notification {
            outbox::enqueue(
                &mut tx,
                FOLLOW_REQUEST_EVENT,
                &format!("follow_request:{notification}"),
                &json!({ "notificationId": notification }),
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(Json(FollowResult { state }))
}

/// `DELETE /people/{id}/follow` (`FOLLOWER = false`) withdraws a pending request or unfollows;
/// `DELETE /me/followers/{id}` (`FOLLOWER = true`) removes an approved follower. Both are
/// idempotent and silent, leave the reverse edge alone, and delete the edge's notification with it.
/// Removing an edge also revokes the pair's discussion reveal grants.
async fn remove_edge<const FOLLOWER: bool>(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    ApiJson(EmptyRequest {}): ApiJson<EmptyRequest>,
) -> Result<StatusCode, ApiError> {
    let other = path_id(&id)?;
    let viewer = require_handle(&current)?;
    let (follower, followee, state) = if FOLLOWER {
        (other, viewer, Some("approved"))
    } else {
        (viewer, other, None)
    };
    let mut tx = security.pool.begin().await?;
    lock_pair(&mut tx, follower, followee).await?;
    let removed = sqlx::query(
        "DELETE FROM follows
         WHERE follower_id = $1 AND followee_id = $2 AND ($3::text IS NULL OR state = $3)",
    )
    .bind(follower)
    .bind(followee)
    .bind(state)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if removed > 0 {
        revoke_discussion_grants(&mut tx, follower, followee).await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /me/follow-requests/{requesterId}`. Decision stays a string so an unknown one gets a
/// field error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionRequest {
    decision: String,
}

async fn decide(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    Path(id): Path<String>,
    ApiJson(request): ApiJson<DecisionRequest>,
) -> Result<StatusCode, ApiError> {
    let requester = path_id(&id)?;
    let viewer = require_handle(&current)?;
    let approve = match request.decision.as_str() {
        "approve" => true,
        "decline" => false,
        _ => {
            return Err(ApiError::new(ErrorCode::ValidationError)
                .with_field("decision", "Choose approve or decline."));
        }
    };
    let mut tx = security.pool.begin().await?;
    lock_pair(&mut tx, viewer, requester).await?;
    // Only a request from someone the viewer can still see counts as pending.
    let (_, _, relationship) = people::subject(&mut tx, viewer, requester)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if relationship.incoming != FollowState::Pending {
        return Err(ApiError::not_found());
    }
    // Decline deletes the edge and, with it, the request's notification; neither decision
    // notifies the requester.
    sqlx::query(if approve {
        "UPDATE follows SET state = 'approved'
         WHERE follower_id = $1 AND followee_id = $2 AND state = 'pending'"
    } else {
        "DELETE FROM follows WHERE follower_id = $1 AND followee_id = $2 AND state = 'pending'"
    })
    .bind(requester)
    .bind(viewer)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PageParams {
    cursor: Option<String>,
    limit: Option<String>,
}

/// `GET /me/follow-requests` (`PENDING = true`) and `GET /me/followers`: listed people with an
/// incoming edge in that state, by edge createdAt,id descending.
async fn incoming<const PENDING: bool>(
    VerifiedUser(current): VerifiedUser,
    State(security): State<Security>,
    params: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<PeoplePage>, ApiError> {
    let viewer = current.user.id;
    let state = if PENDING { "pending" } else { "approved" };
    let Query(params) = params.map_err(|_| ApiError::malformed())?;
    let limit = page_limit(params.limit.as_deref(), DEFAULT_LIMIT)?;
    let after = params
        .cursor
        .as_deref()
        .map(|cursor| {
            decode_opaque(cursor, |text| {
                let (micros, id) = text.split_once('.')?;
                Some((
                    DateTime::<Utc>::from_timestamp_micros(micros.parse().ok()?)?,
                    Uuid::try_parse(id).ok()?,
                ))
            })
        })
        .transpose()?;
    let mut conn = security.pool.acquire().await?;
    let rows: Vec<(DateTime<Utc>, Uuid)> = sqlx::query_as(concat!(
        "SELECT f.created_at, f.follower_id
             FROM follows f
             JOIN users u ON u.id = f.follower_id
             WHERE f.followee_id = $1 AND f.state = $2 AND ",
        listed!(),
        " AND ($3::timestamptz IS NULL OR (f.created_at, f.follower_id) < ($3, $4))
             ORDER BY f.created_at DESC, f.follower_id DESC
             LIMIT $5"
    ))
    .bind(viewer)
    .bind(state)
    .bind(after.map(|(created_at, _)| created_at))
    .bind(after.map(|(_, id)| id))
    .bind(limit + 1)
    .fetch_all(&mut *conn)
    .await?;
    let more = rows.len() as i64 > limit;
    let page: Vec<_> = rows.into_iter().take(limit as usize).collect();
    let next_cursor = more.then(|| page.last()).flatten().map(|(created_at, id)| {
        encode_opaque(&format!(
            "{}.{}",
            created_at.timestamp_micros(),
            id.simple()
        ))
    });
    let ids: Vec<Uuid> = page.iter().map(|row| row.1).collect();
    let items = cards(&mut conn, viewer, &ids).await?;
    Ok(Json(PeoplePage { items, next_cursor }))
}

/// Person cards for `ids`, in that order, dropping anyone the policy now conceals.
async fn cards(
    conn: &mut PgConnection,
    viewer: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Person>, sqlx::Error> {
    let rows: Vec<PersonRow> = sqlx::query_as(concat!(
        "SELECT ",
        person_columns!(),
        " FROM users u WHERE u.id = ANY($2)"
    ))
    .bind(viewer)
    .bind(ids)
    .fetch_all(conn)
    .await?;
    let mut cards: Vec<Person> = rows
        .into_iter()
        .filter_map(|row| people::person(viewer, row))
        .collect();
    cards.sort_by_key(|card| ids.iter().position(|id| *id == card.id));
    Ok(cards)
}
