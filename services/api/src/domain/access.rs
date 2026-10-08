//! R14 privacy gate (C06, C07 access, architecture.md request boundary). Decides who may see a
//! person, their library and activity, or a hosted episode discussion, from relationship facts
//! the caller reads on every request. It runs before the spoiler gate in [`super::spoilers`] and
//! never looks at progress or reveal grants, so no grant can widen what it allows.

use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FollowState {
    #[default]
    None,
    Pending,
    Approved,
}

/// Who is asking. Social content is never visible to anonymous requests, even for public
/// accounts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    Anonymous,
    Member(Uuid),
}

/// The person whose profile, library, activity or hosted discussion is requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subject {
    pub id: Uuid,
    pub visibility: Visibility,
}

/// Current edges between the viewer and another person, read at request time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Relationship {
    /// Viewer follows the other person.
    pub outgoing: FollowState,
    /// The other person follows the viewer.
    pub incoming: FollowState,
    pub viewer_blocks: bool,
    pub blocked_by: bool,
}

impl Relationship {
    /// A block in either direction conceals both people from each other.
    pub fn blocked(&self) -> bool {
        self.viewer_blocks || self.blocked_by
    }

    /// Both follows approved and no block.
    pub fn mutual(&self) -> bool {
        !self.blocked()
            && self.outgoing == FollowState::Approved
            && self.incoming == FollowState::Approved
    }

    /// The Person DTO `relationship` object. Callers must check [`can_see_person`] first; a
    /// blocked person has no card to put it on.
    pub fn view(&self) -> RelationshipView {
        RelationshipView {
            outgoing: self.outgoing,
            incoming: self.incoming,
            mutual: self.mutual(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RelationshipView {
    pub outgoing: FollowState,
    pub incoming: FollowState,
    pub mutual: bool,
}

/// The profile card (id, handle, display name, visibility, relationship) is discoverable by any
/// signed-in member without a block either way.
pub fn can_see_person(viewer: Viewer, subject_id: Uuid, relationship: &Relationship) -> bool {
    match viewer {
        Viewer::Anonymous => false,
        Viewer::Member(id) if id == subject_id => true,
        Viewer::Member(_) => !relationship.blocked(),
    }
}

/// Library, progress and activity: the owner; any signed-in member for public accounts; an
/// approved follower for private ones. Never through a block.
pub fn can_view_library(viewer: Viewer, subject: Subject, relationship: &Relationship) -> bool {
    if !can_see_person(viewer, subject.id, relationship) {
        return false;
    }
    viewer == Viewer::Member(subject.id)
        || subject.visibility == Visibility::Public
        || relationship.outgoing == FollowState::Approved
}

/// Proof that the viewer passed the discussion privacy gate for one host. The spoiler gate
/// requires it, so a reveal grant is never consulted without current privacy access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscussionMember {
    viewer_id: Uuid,
    host_id: Uuid,
}

impl DiscussionMember {
    pub fn viewer_id(&self) -> Uuid {
        self.viewer_id
    }

    pub fn host_id(&self) -> Uuid {
        self.host_id
    }

    pub fn is_host(&self) -> bool {
        self.viewer_id == self.host_id
    }
}

/// Discussion access: the host, or a current mutual approved follow of the host with no block
/// either way. Account visibility plays no part; a public host's threads are not public.
pub fn discussion_member(
    viewer: Viewer,
    host_id: Uuid,
    relationship: &Relationship,
) -> Option<DiscussionMember> {
    let Viewer::Member(viewer_id) = viewer else {
        return None;
    };
    (viewer_id == host_id || relationship.mutual())
        .then_some(DiscussionMember { viewer_id, host_id })
}

/// Whether two eligible participants see each other's comments and identities. A participant
/// block hides both sides without affecting either one's access to the host's thread.
pub fn participants_visible(relationship: &Relationship) -> bool {
    !relationship.blocked()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWER: Uuid = Uuid::from_u128(1);
    const OTHER: Uuid = Uuid::from_u128(2);

    const STATES: [FollowState; 3] = [
        FollowState::None,
        FollowState::Pending,
        FollowState::Approved,
    ];

    fn every_relationship() -> Vec<Relationship> {
        let mut all = Vec::new();
        for outgoing in STATES {
            for incoming in STATES {
                for viewer_blocks in [false, true] {
                    for blocked_by in [false, true] {
                        all.push(Relationship {
                            outgoing,
                            incoming,
                            viewer_blocks,
                            blocked_by,
                        });
                    }
                }
            }
        }
        all
    }

    fn approved(outgoing: bool, incoming: bool) -> Relationship {
        let state = |on| {
            if on {
                FollowState::Approved
            } else {
                FollowState::None
            }
        };
        Relationship {
            outgoing: state(outgoing),
            incoming: state(incoming),
            ..Relationship::default()
        }
    }

    #[test]
    fn one_way_follow_is_not_a_discussion_member_but_mutual_follow_is() {
        let viewer = Viewer::Member(VIEWER);
        assert_eq!(
            discussion_member(viewer, OTHER, &approved(true, false)),
            None
        );
        assert_eq!(
            discussion_member(viewer, OTHER, &approved(false, true)),
            None
        );
        let member = discussion_member(viewer, OTHER, &approved(true, true)).unwrap();
        assert_eq!(member.viewer_id(), VIEWER);
        assert_eq!(member.host_id(), OTHER);
        assert!(!member.is_host());
    }

    #[test]
    fn host_is_always_a_member_of_own_threads() {
        let member =
            discussion_member(Viewer::Member(VIEWER), VIEWER, &Relationship::default()).unwrap();
        assert!(member.is_host());
    }

    #[test]
    fn block_or_unfollow_revokes_discussion_membership() {
        let viewer = Viewer::Member(VIEWER);
        let mutual = approved(true, true);
        assert!(discussion_member(viewer, OTHER, &mutual).is_some());
        for revoked in [
            Relationship {
                viewer_blocks: true,
                ..mutual
            },
            Relationship {
                blocked_by: true,
                ..mutual
            },
            approved(false, true),
            approved(true, false),
            Relationship {
                incoming: FollowState::Pending,
                ..mutual
            },
        ] {
            assert_eq!(
                discussion_member(viewer, OTHER, &revoked),
                None,
                "{revoked:?}"
            );
        }
    }

    #[test]
    fn public_library_requires_a_signed_in_viewer() {
        let public = Subject {
            id: OTHER,
            visibility: Visibility::Public,
        };
        let none = Relationship::default();
        assert!(!can_view_library(Viewer::Anonymous, public, &none));
        assert!(!can_see_person(Viewer::Anonymous, OTHER, &none));
        assert!(can_view_library(Viewer::Member(VIEWER), public, &none));
    }

    #[test]
    fn private_library_requires_an_approved_follow_and_pending_is_not_enough() {
        let private = Subject {
            id: OTHER,
            visibility: Visibility::Private,
        };
        let viewer = Viewer::Member(VIEWER);
        let pending = Relationship {
            outgoing: FollowState::Pending,
            ..Relationship::default()
        };
        assert!(can_see_person(viewer, OTHER, &pending));
        assert!(!can_view_library(viewer, private, &pending));
        // A reverse edge alone grants nothing.
        assert!(!can_view_library(viewer, private, &approved(false, true)));
        assert!(can_view_library(viewer, private, &approved(true, false)));
    }

    #[test]
    fn switching_visibility_changes_library_access_immediately() {
        let viewer = Viewer::Member(VIEWER);
        let stranger = Relationship::default();
        let follower = approved(true, false);
        let as_ = |visibility| Subject {
            id: OTHER,
            visibility,
        };
        assert!(can_view_library(viewer, as_(Visibility::Public), &stranger));
        assert!(!can_view_library(
            viewer,
            as_(Visibility::Private),
            &stranger
        ));
        // Going private keeps existing approved follows.
        assert!(can_view_library(
            viewer,
            as_(Visibility::Private),
            &follower
        ));
    }

    #[test]
    fn block_conceals_both_people_in_both_directions() {
        let public = Subject {
            id: OTHER,
            visibility: Visibility::Public,
        };
        for relationship in [
            Relationship {
                viewer_blocks: true,
                ..approved(true, true)
            },
            Relationship {
                blocked_by: true,
                ..approved(true, true)
            },
        ] {
            let viewer = Viewer::Member(VIEWER);
            assert!(!can_see_person(viewer, OTHER, &relationship));
            assert!(!can_view_library(viewer, public, &relationship));
            assert!(!participants_visible(&relationship));
            assert!(!relationship.mutual());
        }
    }

    #[test]
    fn relationship_view_serializes_contract_names() {
        let json = serde_json::to_value(approved(true, true).view()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"outgoing": "approved", "incoming": "approved", "mutual": true})
        );
        let pending = Relationship {
            outgoing: FollowState::Pending,
            ..Relationship::default()
        };
        assert_eq!(
            serde_json::to_value(pending.view()).unwrap(),
            serde_json::json!({"outgoing": "pending", "incoming": "none", "mutual": false})
        );
    }

    /// Every viewer kind × visibility × follow edge pair × block pair, checked against the C06/C07
    /// rules stated independently.
    #[test]
    fn privacy_truth_table() {
        let viewers = [
            ("anonymous", Viewer::Anonymous),
            ("self", Viewer::Member(OTHER)),
            ("member", Viewer::Member(VIEWER)),
        ];
        let mut checked = 0;
        for (kind, viewer) in viewers {
            for visibility in [Visibility::Private, Visibility::Public] {
                for relationship in every_relationship() {
                    let subject = Subject {
                        id: OTHER,
                        visibility,
                    };
                    let context = format!("{kind} {visibility:?} {relationship:?}");
                    let signed_in = kind != "anonymous";
                    let is_self = kind == "self";
                    let blocked = relationship.viewer_blocks || relationship.blocked_by;
                    let follows = relationship.outgoing == FollowState::Approved;
                    let followed = relationship.incoming == FollowState::Approved;

                    let see = signed_in && (is_self || !blocked);
                    assert_eq!(
                        can_see_person(viewer, OTHER, &relationship),
                        see,
                        "{context}"
                    );
                    let library = see && (is_self || visibility == Visibility::Public || follows);
                    assert_eq!(
                        can_view_library(viewer, subject, &relationship),
                        library,
                        "{context}"
                    );
                    let member = signed_in && (is_self || (!blocked && follows && followed));
                    assert_eq!(
                        discussion_member(viewer, OTHER, &relationship).is_some(),
                        member,
                        "{context}"
                    );
                    assert_eq!(participants_visible(&relationship), !blocked, "{context}");
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 3 * 2 * 3 * 3 * 2 * 2);
    }
}
