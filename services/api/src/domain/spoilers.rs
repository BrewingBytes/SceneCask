//! R14 spoiler gate and redacted projections (C05 Episode DTO, C07, C09 cases 1, 3, 7, 9).
//! Episode details unlock when the viewer individually watched the episode; a regular discussion
//! unlocks when every active regular episode through its target is watched; a session-scoped
//! reveal grant relaxes only its own scope and resource. Specials discussions always need a
//! grant. Grants never mark progress, and the discussion gate requires an
//! [`access::DiscussionMember`](super::access::DiscussionMember), so privacy always runs first.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{access::DiscussionMember, progress::Episode, release::Release};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealScope {
    EpisodeDetails,
    Discussion,
}

impl RevealScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EpisodeDetails => "episode_details",
            Self::Discussion => "discussion",
        }
    }
}

/// The sign-in session a request runs under. Grants from any other session never apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Session<'a> {
    /// `sessions.id_hash`.
    pub id_hash: &'a [u8],
    pub user_id: Uuid,
}

/// A `reveal_grants` row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant<'a> {
    pub session_id: &'a [u8],
    pub user_id: Uuid,
    pub scope: RevealScope,
    pub resource_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

/// Whether a live grant for exactly this session, user, scope and resource exists at `now`.
fn granted(
    grants: &[Grant<'_>],
    session: Session<'_>,
    scope: RevealScope,
    resource_id: Uuid,
    now: DateTime<Utc>,
) -> bool {
    grants.iter().any(|grant| {
        grant.session_id == session.id_hash
            && grant.user_id == session.user_id
            && grant.scope == scope
            && grant.resource_id == resource_id
            && now < grant.expires_at
    })
}

/// API `Access`, shared by episode details and discussions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Locked,
    Watched,
    Revealed,
}

impl Access {
    pub fn is_unlocked(self) -> bool {
        self != Self::Locked
    }
}

/// Episode details gate. The viewer's own individual mark unlocks the episode regardless of
/// earlier gaps; otherwise only an `episode_details` grant for this episode does.
pub fn details_access(
    session: Session<'_>,
    episode_id: Uuid,
    watched: bool,
    grants: &[Grant<'_>],
    now: DateTime<Utc>,
) -> Access {
    if watched {
        Access::Watched
    } else if granted(
        grants,
        session,
        RevealScope::EpisodeDetails,
        episode_id,
        now,
    ) {
        Access::Revealed
    } else {
        Access::Locked
    }
}

/// Spoiler decision for one discussion the viewer may already access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscussionGate {
    id: Uuid,
    is_host: bool,
    access: Access,
    missing: Vec<(i32, i32)>,
    special: bool,
}

/// 403 SPOILER_LOCKED: the viewer may access the thread but not read or post yet. Carries no
/// thread content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpoilerLocked;

/// Proof that a discussion's comments may be read and posted to on this request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnlockedDiscussion {
    pub id: Uuid,
    pub access: Access,
}

impl DiscussionGate {
    pub fn access(&self) -> Access {
        self.access
    }

    /// Unmarked active regular episodes through the target, formatted `S1 E2`, in order. Empty
    /// for specials, whose gate is a grant alone. Still listed when a grant unlocks the thread,
    /// since a reveal marks nothing.
    pub fn missing_episode_codes(&self) -> Vec<String> {
        self.missing
            .iter()
            .map(|(season, number)| format!("S{season} E{number}"))
            .collect()
    }

    pub fn unlocked(&self) -> Result<UnlockedDiscussion, SpoilerLocked> {
        if self.access.is_unlocked() {
            Ok(UnlockedDiscussion {
                id: self.id,
                access: self.access,
            })
        } else {
            Err(SpoilerLocked)
        }
    }

    /// The Discussion list item. It never carries comments, previews or counts, so it is safe in
    /// every access state.
    pub fn summary<P: Serialize>(&self, host: P) -> DiscussionSummary<P> {
        DiscussionSummary {
            id: self.id,
            host,
            is_host: self.is_host,
            access: self.access,
            missing_episode_codes: self.missing_episode_codes(),
            is_special: self.special,
        }
    }
}

/// API `Discussion`; `P` is the caller's Person DTO for the host.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscussionSummary<P> {
    id: Uuid,
    host: P,
    is_host: bool,
    access: Access,
    missing_episode_codes: Vec<String>,
    is_special: bool,
}

/// A `discussions` row: the thread and the user hosting it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Discussion {
    pub id: Uuid,
    pub host_id: Uuid,
}

/// Discussion spoiler gate for `discussion` on `target`. `episodes` is the show's catalog
/// joined with the viewer's own marks (never the host's), in any order. Every active regular
/// episode ordered before the target, released or not, and the target itself must be marked.
/// The member must belong to the session's user and to the discussion's host, otherwise the
/// gate stays locked.
pub fn discussion_gate(
    member: &DiscussionMember,
    session: Session<'_>,
    discussion: Discussion,
    target: &Episode<'_>,
    episodes: &[Episode<'_>],
    grants: &[Grant<'_>],
    now: DateTime<Utc>,
) -> DiscussionGate {
    let special = target.season == 0;
    let mut missing: Vec<&Episode> = if special {
        Vec::new()
    } else {
        let key = |episode: &Episode| (episode.season, episode.number, episode.id);
        episodes
            .iter()
            .filter(|episode| {
                episode.season > 0
                    && !episode.archived
                    && !episode.watched
                    && key(episode) < key(target)
            })
            .chain((!target.watched).then_some(target))
            .collect()
    };
    missing.sort_by_key(|episode| (episode.season, episode.number, episode.id));
    missing.dedup_by_key(|episode| episode.id);

    let access = if member.viewer_id() != session.user_id || member.host_id() != discussion.host_id
    {
        Access::Locked
    } else if !special && missing.is_empty() {
        Access::Watched
    } else if granted(grants, session, RevealScope::Discussion, discussion.id, now) {
        Access::Revealed
    } else {
        Access::Locked
    };
    DiscussionGate {
        id: discussion.id,
        is_host: member.is_host() && member.host_id() == discussion.host_id,
        access,
        missing: missing
            .into_iter()
            .map(|episode| (episode.season, episode.number))
            .collect(),
        special,
    }
}

/// Spoiler-protected content. It has no `Serialize` impl and its `Debug` output is redacted, so
/// the only way into a response is through a gated projection such as [`episode_view`].
pub struct Protected<T>(T);

impl<T> fmt::Debug for Protected<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Protected(..)")
    }
}

/// Protected episode title, overview and still URL.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeDetails {
    title: Option<String>,
    overview: Option<String>,
    still_url: Option<String>,
}

impl EpisodeDetails {
    pub fn protect(
        title: Option<String>,
        overview: Option<String>,
        still_url: Option<String>,
    ) -> Protected<Self> {
        Protected(Self {
            title,
            overview,
            still_url,
        })
    }
}

impl fmt::Debug for EpisodeDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EpisodeDetails(..)")
    }
}

/// Unprotected episode facts: position, release and the viewer's own progress.
#[derive(Clone, Copy, Debug)]
pub struct EpisodeFacts<'a> {
    pub id: Uuid,
    pub show_id: Uuid,
    pub season: i32,
    pub number: i32,
    pub release: Release<'a>,
    pub watched: bool,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReleaseView {
    state: &'static str,
    date: Option<String>,
    timezone: Option<String>,
    estimated: bool,
}

impl From<Release<'_>> for ReleaseView {
    fn from(release: Release<'_>) -> Self {
        Self {
            state: release.state.as_str(),
            date: release.date.map(|date| date.format("%Y-%m-%d").to_string()),
            timezone: release.timezone.map(str::to_owned),
            estimated: release.estimated,
        }
    }
}

/// API `Episode`. A locked episode has no `details` key at all, not a null or blurred one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeView {
    id: Uuid,
    show_id: Uuid,
    season: i32,
    number: i32,
    release: ReleaseView,
    watched: bool,
    revision: i64,
    details_access: Access,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<EpisodeDetails>,
}

impl EpisodeView {
    pub fn details_access(&self) -> Access {
        self.details_access
    }
}

/// Gates and projects one episode for the session's viewer. Locked details are dropped here,
/// so they cannot reach a response, log or error.
pub fn episode_view(
    session: Session<'_>,
    facts: EpisodeFacts<'_>,
    details: Protected<EpisodeDetails>,
    grants: &[Grant<'_>],
    now: DateTime<Utc>,
) -> EpisodeView {
    let access = details_access(session, facts.id, facts.watched, grants, now);
    EpisodeView {
        id: facts.id,
        show_id: facts.show_id,
        season: facts.season,
        number: facts.number,
        release: facts.release.into(),
        watched: facts.watched,
        revision: facts.revision,
        details_access: access,
        details: access.is_unlocked().then_some(details.0),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::domain::{
        access::{FollowState, Relationship, Viewer, discussion_member},
        progress::tests::{episode as episode_at, future, now, released},
        release::Schedule,
    };

    const VIEWER: Uuid = Uuid::from_u128(0xa);
    const STRANGER: Uuid = Uuid::from_u128(0xb);
    const HOST: Uuid = Uuid::from_u128(0xc);
    const THREAD_A: Uuid = Uuid::from_u128(0xd1);
    const THREAD_B: Uuid = Uuid::from_u128(0xd2);
    const SESSION: &[u8] = b"current-session";
    const OTHER_SESSION: &[u8] = b"other-session";

    const TITLE: &str = "The Orchard Burns";
    const OVERVIEW: &str = "Mara learns who lit the fire.";
    const STILL: &str = "https://image.tmdb.org/t/p/w780/secret-still.jpg";

    fn session() -> Session<'static> {
        Session {
            id_hash: SESSION,
            user_id: VIEWER,
        }
    }

    fn episode(season: i32, number: i32, watched: bool) -> Episode<'static> {
        episode_at(season, number, released(), watched)
    }

    fn grant(scope: RevealScope, resource_id: Uuid) -> Grant<'static> {
        Grant {
            session_id: SESSION,
            user_id: VIEWER,
            scope,
            resource_id,
            expires_at: now() + chrono::Duration::hours(12),
        }
    }

    fn member() -> DiscussionMember {
        let mutual = Relationship {
            outgoing: FollowState::Approved,
            incoming: FollowState::Approved,
            ..Relationship::default()
        };
        discussion_member(Viewer::Member(VIEWER), HOST, &mutual).unwrap()
    }

    fn gate(
        thread: Uuid,
        target: &Episode,
        episodes: &[Episode],
        grants: &[Grant],
    ) -> DiscussionGate {
        discussion_gate(
            &member(),
            session(),
            Discussion {
                id: thread,
                host_id: HOST,
            },
            target,
            episodes,
            grants,
            now(),
        )
    }

    fn details() -> Protected<EpisodeDetails> {
        EpisodeDetails::protect(
            Some(TITLE.to_owned()),
            Some(OVERVIEW.to_owned()),
            Some(STILL.to_owned()),
        )
    }

    fn view(watched: bool, grants: &[Grant]) -> EpisodeView {
        let target = episode_at(1, 3, future(), watched);
        let facts = EpisodeFacts {
            id: target.id,
            show_id: Uuid::from_u128(0x5),
            season: 1,
            number: 3,
            release: target.schedule.release(now()),
            watched,
            revision: i64::from(watched),
        };
        episode_view(session(), facts, details(), grants, now())
    }

    fn keys(value: &Value) -> Vec<&str> {
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        keys
    }

    fn assert_no_protected_text(text: &str) {
        for secret in [TITLE, OVERVIEW, STILL, "secret-still"] {
            assert!(!text.contains(secret), "leaked {secret:?} in {text}");
        }
    }

    /// C09 case 1 and acceptance criterion: E3 watched with an E2 gap.
    #[test]
    fn watched_e3_with_e2_gap_unlocks_details_but_not_discussion() {
        let episodes = [
            episode(1, 1, true),
            episode(1, 2, false),
            episode(1, 3, true),
        ];
        let e3 = episodes[2];
        assert_eq!(
            details_access(session(), e3.id, e3.watched, &[], now()),
            Access::Watched
        );
        let thread = gate(THREAD_A, &e3, &episodes, &[]);
        assert_eq!(thread.access(), Access::Locked);
        assert_eq!(thread.missing_episode_codes(), ["S1 E2"]);
        assert_eq!(thread.unlocked(), Err(SpoilerLocked));

        // Filling the gap or an explicit discussion grant opens it.
        let filled = [episode(1, 1, true), episode(1, 2, true), e3];
        assert_eq!(gate(THREAD_A, &e3, &filled, &[]).access(), Access::Watched);
        let revealed = gate(
            THREAD_A,
            &e3,
            &episodes,
            &[grant(RevealScope::Discussion, THREAD_A)],
        );
        assert_eq!(revealed.access(), Access::Revealed);
        assert_eq!(revealed.missing_episode_codes(), ["S1 E2"]);
        assert_eq!(
            revealed.unlocked(),
            Ok(UnlockedDiscussion {
                id: THREAD_A,
                access: Access::Revealed
            })
        );
    }

    /// Acceptance criterion: a grant for thread A never opens thread B on the same episode.
    #[test]
    fn grant_for_one_thread_leaves_another_thread_at_the_same_episode_locked() {
        let episodes = [episode(1, 1, false), episode(1, 2, false)];
        let grants = [grant(RevealScope::Discussion, THREAD_A)];
        assert_eq!(
            gate(THREAD_A, &episodes[1], &episodes, &grants).access(),
            Access::Revealed
        );
        assert_eq!(
            gate(THREAD_B, &episodes[1], &episodes, &grants).access(),
            Access::Locked
        );
    }

    /// Acceptance criterion: once privacy is revoked, no grant can authorize access. The spoiler
    /// gate cannot even be evaluated without a `DiscussionMember`.
    #[test]
    fn privacy_revocation_supersedes_a_reveal_grant() {
        let grants = [grant(RevealScope::Discussion, THREAD_A)];
        // Unwatched, so the grant alone unlocks the thread while privacy holds.
        let episodes = [episode(1, 1, false)];
        let read = |relationship: Relationship| {
            discussion_member(Viewer::Member(VIEWER), HOST, &relationship).map(|member| {
                discussion_gate(
                    &member,
                    session(),
                    Discussion {
                        id: THREAD_A,
                        host_id: HOST,
                    },
                    &episodes[0],
                    &episodes,
                    &grants,
                    now(),
                )
                .access()
            })
        };
        let mutual = Relationship {
            outgoing: FollowState::Approved,
            incoming: FollowState::Approved,
            ..Relationship::default()
        };
        assert_eq!(read(mutual), Some(Access::Revealed));
        for revoked in [
            Relationship {
                outgoing: FollowState::None,
                ..mutual
            },
            Relationship {
                incoming: FollowState::None,
                ..mutual
            },
            Relationship {
                viewer_blocks: true,
                ..mutual
            },
            Relationship {
                blocked_by: true,
                ..mutual
            },
        ] {
            assert_eq!(read(revoked), None, "{revoked:?}");
        }
    }

    #[test]
    fn locked_projection_omits_protected_keys_entirely() {
        let locked = view(false, &[]);
        assert_eq!(locked.details_access(), Access::Locked);
        let json = serde_json::to_value(&locked).unwrap();
        assert_eq!(
            keys(&json),
            [
                "detailsAccess",
                "id",
                "number",
                "release",
                "revision",
                "season",
                "showId",
                "watched"
            ]
        );
        assert_eq!(json["detailsAccess"], "locked");
        assert_eq!(
            json["release"],
            serde_json::json!({
                "state": "future", "date": "2026-11-12", "timezone": "UTC", "estimated": true
            })
        );
        let text = serde_json::to_string(&locked).unwrap();
        assert_no_protected_text(&text);
        for key in ["details", "title", "overview", "stillUrl", "still"] {
            assert!(!text.contains(&format!("\"{key}\"")), "{key} in {text}");
        }
        assert_no_protected_text(&format!("{locked:?}"));
    }

    #[test]
    fn unlocked_projection_carries_details_and_keeps_the_future_label() {
        for (watched, grants, access) in [
            (true, vec![], "watched"),
            (
                false,
                vec![grant(RevealScope::EpisodeDetails, view(false, &[]).id)],
                "revealed",
            ),
        ] {
            let json = serde_json::to_value(view(watched, &grants)).unwrap();
            assert_eq!(json["detailsAccess"], access);
            assert_eq!(json["watched"], watched);
            assert_eq!(json["release"]["state"], "future");
            assert_eq!(
                json["details"],
                serde_json::json!({"title": TITLE, "overview": OVERVIEW, "stillUrl": STILL})
            );
            assert_eq!(keys(&json["details"]), ["overview", "stillUrl", "title"]);
        }
    }

    #[test]
    fn protected_content_is_redacted_from_debug_output() {
        assert_no_protected_text(&format!("{:?}", details()));
        assert_no_protected_text(&format!("{:?}", view(true, &[])));
    }

    #[test]
    fn specials_discussion_requires_an_explicit_grant_even_when_marked() {
        let special = episode(0, 1, true);
        let episodes = [episode(1, 1, true), special];
        let locked = gate(THREAD_A, &special, &episodes, &[]);
        assert_eq!(locked.access(), Access::Locked);
        assert!(locked.missing_episode_codes().is_empty());
        assert_eq!(
            gate(
                THREAD_A,
                &special,
                &episodes,
                &[grant(RevealScope::Discussion, THREAD_A)]
            )
            .access(),
            Access::Revealed
        );
        // Special details still unlock by individual watch.
        assert_eq!(
            details_access(session(), special.id, true, &[], now()),
            Access::Watched
        );
    }

    /// C09 case 3: undated and future predecessors still gate the discussion.
    #[test]
    fn unmarked_undated_or_future_predecessor_keeps_discussion_locked() {
        let target = episode(1, 3, true);
        for schedule in [Schedule::Undated, future()] {
            let episodes = [
                episode(1, 1, true),
                episode_at(1, 2, schedule, false),
                target,
            ];
            let thread = gate(THREAD_A, &target, &episodes, &[]);
            assert_eq!(thread.access(), Access::Locked, "{schedule:?}");
            assert_eq!(thread.missing_episode_codes(), ["S1 E2"]);
        }
    }

    #[test]
    fn gate_orders_by_season_then_number_ignores_specials_and_archived_and_later_episodes() {
        let target = episode(2, 1, true);
        let mut archived = episode(1, 5, false);
        archived.archived = true;
        let episodes = [
            episode(2, 2, false),
            episode(1, 10, false),
            target,
            episode(0, 1, false),
            archived,
            episode(1, 2, false),
        ];
        let thread = gate(THREAD_A, &target, &episodes, &[]);
        assert_eq!(thread.missing_episode_codes(), ["S1 E2", "S1 E10"]);

        // Missing target itself is listed last, once, even when also present in the catalog.
        let unwatched = episode(1, 2, false);
        let thread = gate(
            THREAD_A,
            &unwatched,
            &[episode(1, 1, false), unwatched],
            &[],
        );
        assert_eq!(thread.missing_episode_codes(), ["S1 E1", "S1 E2"]);
    }

    #[test]
    fn summary_serializes_contract_shape_without_thread_content() {
        let episodes = [episode(1, 1, false), episode(1, 2, true)];
        let summary = gate(THREAD_A, &episodes[1], &episodes, &[]).summary("host-person");
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": THREAD_A, "host": "host-person", "isHost": false, "access": "locked",
                "missingEpisodeCodes": ["S1 E1"], "isSpecial": false
            })
        );
    }

    #[test]
    fn reveal_scope_names_match_the_api_contract() {
        assert_eq!(RevealScope::EpisodeDetails.as_str(), "episode_details");
        assert_eq!(RevealScope::Discussion.as_str(), "discussion");
        for (access, name) in [
            (Access::Locked, "locked"),
            (Access::Watched, "watched"),
            (Access::Revealed, "revealed"),
        ] {
            assert_eq!(serde_json::to_value(access).unwrap(), name);
        }
    }

    /// Grants that differ from a valid grant for `scope`/`resource` in exactly one way, with
    /// whether each should apply.
    fn grant_variants(
        scope: RevealScope,
        resource: Uuid,
    ) -> Vec<(&'static str, Grant<'static>, bool)> {
        let valid = grant(scope, resource);
        let other_scope = match scope {
            RevealScope::EpisodeDetails => RevealScope::Discussion,
            RevealScope::Discussion => RevealScope::EpisodeDetails,
        };
        vec![
            ("valid", valid, true),
            (
                "other scope",
                Grant {
                    scope: other_scope,
                    ..valid
                },
                false,
            ),
            (
                "other resource",
                Grant {
                    resource_id: Uuid::from_u128(0xee),
                    ..valid
                },
                false,
            ),
            (
                "other session",
                Grant {
                    session_id: OTHER_SESSION,
                    ..valid
                },
                false,
            ),
            (
                "other user",
                Grant {
                    user_id: STRANGER,
                    ..valid
                },
                false,
            ),
            (
                "expired",
                Grant {
                    expires_at: now(),
                    ..valid
                },
                false,
            ),
            (
                "almost expired",
                Grant {
                    expires_at: now() + chrono::Duration::seconds(1),
                    ..valid
                },
                true,
            ),
        ]
    }

    #[test]
    fn details_truth_table() {
        let id = Uuid::from_u128(0x13);
        for watched in [false, true] {
            for granted in [None].into_iter().chain(
                grant_variants(RevealScope::EpisodeDetails, id)
                    .into_iter()
                    .map(Some),
            ) {
                let grants: Vec<Grant> = granted.iter().map(|(_, grant, _)| *grant).collect();
                let applies = granted.as_ref().is_some_and(|(_, _, applies)| *applies);
                let expected = if watched {
                    Access::Watched
                } else if applies {
                    Access::Revealed
                } else {
                    Access::Locked
                };
                let name = granted.as_ref().map_or("none", |(name, _, _)| name);
                assert_eq!(
                    details_access(session(), id, watched, &grants, now()),
                    expected,
                    "watched={watched} grant={name}"
                );

                // The projection agrees with the gate and only unlocked views carry details.
                let target = episode_at(1, 3, future(), watched);
                let facts = EpisodeFacts {
                    id,
                    show_id: Uuid::from_u128(0x5),
                    season: 1,
                    number: 3,
                    release: target.schedule.release(now()),
                    watched,
                    revision: 0,
                };
                let json =
                    serde_json::to_value(episode_view(session(), facts, details(), &grants, now()))
                        .unwrap();
                assert_eq!(
                    json["detailsAccess"],
                    serde_json::to_value(expected).unwrap()
                );
                assert_eq!(json.get("details").is_some(), expected.is_unlocked());
                if !expected.is_unlocked() {
                    assert_no_protected_text(&json.to_string());
                }
            }
        }
    }

    /// Every predecessor/target mark combination (with released, future and undated E2) for a
    /// regular and a special target, against every grant variant, plus a member whose user does
    /// not own the session or was issued for another host.
    #[test]
    fn discussion_truth_table() {
        let mut checked = 0;
        for special in [false, true] {
            for e1 in [false, true] {
                for (e2_schedule, e2) in [released(), future(), Schedule::Undated]
                    .into_iter()
                    .flat_map(|schedule| [(schedule, false), (schedule, true)])
                {
                    for target_watched in [false, true] {
                        let target = if special {
                            episode(0, 1, target_watched)
                        } else {
                            episode(1, 3, target_watched)
                        };
                        let episodes =
                            [episode(1, 1, e1), episode_at(1, 2, e2_schedule, e2), target];
                        let all_marked = e1 && e2 && target_watched;
                        let variants = [None].into_iter().chain(
                            grant_variants(RevealScope::Discussion, THREAD_A)
                                .into_iter()
                                .map(Some),
                        );
                        for granted in variants {
                            let grants: Vec<Grant> =
                                granted.iter().map(|(_, grant, _)| *grant).collect();
                            let applies = granted.as_ref().is_some_and(|(_, _, a)| *a);
                            let name = granted.as_ref().map_or("none", |(name, _, _)| name);
                            let context = format!(
                                "special={special} e1={e1} e2={e2}/{e2_schedule:?} \
                                 target={target_watched} grant={name}"
                            );
                            let expected = if !special && all_marked {
                                Access::Watched
                            } else if applies {
                                Access::Revealed
                            } else {
                                Access::Locked
                            };
                            let thread = gate(THREAD_A, &target, &episodes, &grants);
                            assert_eq!(thread.access(), expected, "{context}");
                            assert_eq!(thread.unlocked().is_ok(), expected.is_unlocked());
                            let missing: Vec<String> = if special {
                                vec![]
                            } else {
                                [(e1, "S1 E1"), (e2, "S1 E2"), (target_watched, "S1 E3")]
                                    .into_iter()
                                    .filter(|(marked, _)| !marked)
                                    .map(|(_, code)| code.to_owned())
                                    .collect()
                            };
                            assert_eq!(thread.missing_episode_codes(), missing, "{context}");

                            // Grants never open a second thread at the same episode.
                            let other = gate(THREAD_B, &target, &episodes, &grants);
                            let other_expected = if !special && all_marked {
                                Access::Watched
                            } else {
                                Access::Locked
                            };
                            assert_eq!(other.access(), other_expected, "{context}");

                            // A member from another user never unlocks under this session.
                            let stranger = discussion_member(
                                Viewer::Member(STRANGER),
                                STRANGER,
                                &Relationship::default(),
                            )
                            .unwrap();
                            let mismatched = discussion_gate(
                                &stranger,
                                session(),
                                Discussion {
                                    id: THREAD_A,
                                    host_id: HOST,
                                },
                                &target,
                                &episodes,
                                &grants,
                                now(),
                            );
                            assert_eq!(mismatched.access(), Access::Locked, "{context}");

                            // A member issued for another host never unlocks this thread, even
                            // the viewer's own self-hosted membership.
                            let own = discussion_member(
                                Viewer::Member(VIEWER),
                                VIEWER,
                                &Relationship::default(),
                            )
                            .unwrap();
                            let wrong_host = discussion_gate(
                                &own,
                                session(),
                                Discussion {
                                    id: THREAD_A,
                                    host_id: HOST,
                                },
                                &target,
                                &episodes,
                                &grants,
                                now(),
                            );
                            assert_eq!(wrong_host.access(), Access::Locked, "{context}");
                            assert!(wrong_host.unlocked().is_err(), "{context}");
                            let summary = serde_json::to_value(wrong_host.summary(())).unwrap();
                            assert_eq!(summary["isHost"], false, "{context}");
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 2 * 2 * 6 * 2 * 8);
    }
}
