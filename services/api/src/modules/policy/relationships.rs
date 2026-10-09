//! Current relationship facts for the privacy policy, read on every request (C06, C07).

use sqlx::PgConnection;
use uuid::Uuid;

use crate::domain::{
    access::{self, DiscussionMember, FollowState, Relationship, Viewer},
    spoilers::Discussion,
};

fn follow_state(value: Option<String>) -> FollowState {
    match value.as_deref() {
        Some("approved") => FollowState::Approved,
        Some("pending") => FollowState::Pending,
        _ => FollowState::None,
    }
}

/// Follow and block edges between `viewer` and `other`, in both directions.
pub async fn relationship(
    conn: &mut PgConnection,
    viewer: Uuid,
    other: Uuid,
) -> Result<Relationship, sqlx::Error> {
    let (outgoing, incoming, viewer_blocks, blocked_by): (
        Option<String>,
        Option<String>,
        bool,
        bool,
    ) = sqlx::query_as(
        "SELECT
             (SELECT state FROM follows WHERE follower_id = $1 AND followee_id = $2),
             (SELECT state FROM follows WHERE follower_id = $2 AND followee_id = $1),
             EXISTS (SELECT 1 FROM blocks WHERE blocker_id = $1 AND blocked_id = $2),
             EXISTS (SELECT 1 FROM blocks WHERE blocker_id = $2 AND blocked_id = $1)",
    )
    .bind(viewer)
    .bind(other)
    .fetch_one(conn)
    .await?;
    Ok(Relationship {
        outgoing: follow_state(outgoing),
        incoming: follow_state(incoming),
        viewer_blocks,
        blocked_by,
    })
}

/// The discussion and the viewer's current membership, or `None` when the thread is unknown,
/// its host is gone or disabled, or the viewer is neither the host nor a mutual approved follow
/// without a block. Callers answer `None` with 404 so inaccessible threads stay concealed.
pub async fn discussion_member(
    conn: &mut PgConnection,
    viewer: Uuid,
    discussion_id: Uuid,
) -> Result<Option<(Discussion, DiscussionMember)>, sqlx::Error> {
    let host_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT d.host_id FROM discussions d
         JOIN users h ON h.id = d.host_id AND h.disabled_at IS NULL
         WHERE d.id = $1",
    )
    .bind(discussion_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(host_id) = host_id else {
        return Ok(None);
    };
    let relationship = relationship(conn, viewer, host_id).await?;
    Ok(
        access::discussion_member(Viewer::Member(viewer), host_id, &relationship).map(|member| {
            (
                Discussion {
                    id: discussion_id,
                    host_id,
                },
                member,
            )
        }),
    )
}
