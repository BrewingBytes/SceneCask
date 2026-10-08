//! R13 per-episode progress rules (decisions.md, C05 LibraryItem.progress). Each episode is
//! tracked independently: a mark never implies earlier episodes, and Completed is derived here,
//! never stored.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::release::{ReleaseState, Schedule};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowStatus {
    Returning,
    Ended,
    Canceled,
    Unknown,
}

/// One catalog episode joined with the user's mark. Season 0 holds specials.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Episode<'a> {
    pub id: Uuid,
    pub season: i32,
    pub number: i32,
    pub schedule: Schedule<'a>,
    pub archived: bool,
    pub watched: bool,
}

impl Episode<'_> {
    /// Regular episodes still in the provider's catalog; only these count toward progress.
    /// Archived episodes stay in history but leave active ordering.
    fn is_active_regular(&self) -> bool {
        self.season > 0 && !self.archived
    }
}

/// A show's catalog and the user's marks for its episodes, in any order.
#[derive(Clone, Copy, Debug)]
pub struct Show<'a> {
    pub status: ShowStatus,
    /// False until a full provider import succeeds; an incomplete catalog never yields Completed.
    pub complete_import: bool,
    pub episodes: &'a [Episode<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressState {
    InProgress,
    CaughtUp,
    Completed,
    NotYetAvailable,
    ReleaseUnknown,
}

impl ProgressState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::CaughtUp => "caught_up",
            Self::Completed => "completed",
            Self::NotYetAvailable => "not_yet_available",
            Self::ReleaseUnknown => "release_unknown",
        }
    }
}

/// Episode position without protected details (API EpisodeCode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EpisodeCode {
    pub id: Uuid,
    pub season: i32,
    pub number: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Marked active regular episodes, including individually marked future and undated ones.
    pub watched: u32,
    /// Active regular episodes.
    pub total: u32,
    /// Rounded down, so 100 means every active regular episode is marked.
    pub percent: u8,
    pub state: ProgressState,
    /// First released, unmarked regular episode in season/episode order.
    pub next_episode: Option<EpisodeCode>,
    /// An unmarked active regular episode precedes a marked one, whatever its release state, so
    /// the gap matches the discussion gate even when `next_episode` is `None`.
    pub out_of_order: bool,
    /// An active regular episode has no release date.
    pub release_info_incomplete: bool,
}

/// Derives progress at `now`, the server clock, so new releases count on every read.
pub fn progress(show: &Show<'_>, now: DateTime<Utc>) -> Progress {
    let mut regular: Vec<(&Episode, ReleaseState)> = show
        .episodes
        .iter()
        .filter(|episode| episode.is_active_regular())
        .map(|episode| (episode, episode.schedule.release(now).state))
        .collect();
    regular.sort_by_key(|(episode, _)| (episode.season, episode.number, episode.id));

    let total = regular.len();
    let watched = regular
        .iter()
        .filter(|(episode, _)| episode.watched)
        .count();
    let has = |state: ReleaseState| regular.iter().any(|(_, s)| *s == state);
    let undated = has(ReleaseState::Unknown);
    let next = regular
        .iter()
        .position(|(episode, state)| !episode.watched && *state == ReleaseState::Released);

    let completed = show.complete_import
        && matches!(show.status, ShowStatus::Ended | ShowStatus::Canceled)
        && total > 0
        && watched == total
        && !undated;
    let state = if completed {
        ProgressState::Completed
    } else if !has(ReleaseState::Released) {
        if has(ReleaseState::Future) {
            ProgressState::NotYetAvailable
        } else {
            ProgressState::ReleaseUnknown
        }
    } else if next.is_some() {
        ProgressState::InProgress
    } else {
        ProgressState::CaughtUp
    };

    Progress {
        watched: saturating_u32(watched),
        total: saturating_u32(total),
        percent: percent(watched, total),
        state,
        next_episode: next.map(|index| {
            let episode = regular[index].0;
            EpisodeCode {
                id: episode.id,
                season: episode.season,
                number: episode.number,
            }
        }),
        out_of_order: regular
            .iter()
            .position(|(episode, _)| !episode.watched)
            .is_some_and(|gap| {
                regular[gap + 1..]
                    .iter()
                    .any(|(episode, _)| episode.watched)
            }),
        release_info_incomplete: undated,
    }
}

fn percent(watched: usize, total: usize) -> u8 {
    if total == 0 {
        return 0;
    }
    // watched <= total, so the quotient is at most 100.
    u8::try_from(watched as u128 * 100 / total as u128).unwrap_or(100)
}

fn saturating_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone};

    use super::*;

    const NOW: (i32, u32, u32) = (2026, 10, 8);

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(NOW.0, NOW.1, NOW.2, 12, 0, 0).unwrap()
    }

    fn released() -> Schedule<'static> {
        Schedule::Date(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap())
    }

    fn future() -> Schedule<'static> {
        Schedule::Date(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap())
    }

    fn episode(
        season: i32,
        number: i32,
        schedule: Schedule<'static>,
        watched: bool,
    ) -> Episode<'static> {
        Episode {
            id: Uuid::from_u128(((season as u128) << 32) | number as u128),
            season,
            number,
            schedule,
            archived: false,
            watched,
        }
    }

    fn show<'a>(status: ShowStatus, episodes: &'a [Episode<'a>]) -> Show<'a> {
        Show {
            status,
            complete_import: true,
            episodes,
        }
    }

    fn code(episode: &Episode) -> Option<EpisodeCode> {
        Some(EpisodeCode {
            id: episode.id,
            season: episode.season,
            number: episode.number,
        })
    }

    #[test]
    fn marking_e1_and_e3_leaves_e2_next_and_out_of_order() {
        let episodes = [
            episode(1, 1, released(), true),
            episode(1, 2, released(), false),
            episode(1, 3, released(), true),
        ];
        let result = progress(&show(ShowStatus::Returning, &episodes), now());
        assert_eq!(result.next_episode, code(&episodes[1]));
        assert!(result.out_of_order);
        assert_eq!(result.state, ProgressState::InProgress);
        assert_eq!((result.watched, result.total, result.percent), (2, 3, 66));
    }

    #[test]
    fn episodes_are_ordered_by_season_then_number_regardless_of_input_order() {
        let episodes = [
            episode(2, 1, released(), false),
            episode(1, 10, released(), false),
            episode(1, 2, released(), true),
        ];
        let result = progress(&show(ShowStatus::Returning, &episodes), now());
        assert_eq!(result.next_episode, code(&episodes[1]));
        assert!(!result.out_of_order);
    }

    #[test]
    fn undated_episode_is_never_next_and_blocks_completed_even_when_marked() {
        for watched in [false, true] {
            let episodes = [
                episode(1, 1, released(), true),
                episode(1, 2, Schedule::Undated, watched),
                episode(1, 3, released(), true),
            ];
            let result = progress(&show(ShowStatus::Ended, &episodes), now());
            assert_eq!(result.next_episode, None, "watched={watched}");
            // An unmarked undated E2 is still a gap behind E3.
            assert_eq!(result.out_of_order, !watched, "watched={watched}");
            assert!(result.release_info_incomplete);
            assert_eq!(result.state, ProgressState::CaughtUp, "watched={watched}");
        }
    }

    #[test]
    fn marked_future_episode_counts_without_becoming_released() {
        let episodes = [
            episode(1, 1, released(), true),
            episode(1, 2, released(), true),
            episode(1, 3, released(), true),
            episode(1, 4, future(), true),
            episode(1, 5, future(), false),
        ];
        let result = progress(&show(ShowStatus::Returning, &episodes), now());
        assert_eq!((result.watched, result.total, result.percent), (4, 5, 80));
        assert_eq!(result.next_episode, None);
        assert_eq!(result.state, ProgressState::CaughtUp);
        assert_eq!(
            episodes[3].schedule.release(now()).state,
            ReleaseState::Future
        );
    }

    #[test]
    fn future_gap_behind_a_marked_future_episode_is_out_of_order() {
        let episodes = [
            episode(1, 1, released(), true),
            episode(1, 2, future(), false),
            episode(1, 3, future(), true),
        ];
        let result = progress(&show(ShowStatus::Returning, &episodes), now());
        assert_eq!(result.next_episode, None);
        assert!(result.out_of_order);
        assert_eq!(result.state, ProgressState::CaughtUp);
    }

    #[test]
    fn next_skips_unreleased_episodes() {
        let episodes = [
            episode(1, 1, released(), true),
            episode(1, 2, future(), false),
            episode(1, 3, Schedule::Undated, false),
            episode(1, 4, released(), false),
        ];
        let result = progress(&show(ShowStatus::Returning, &episodes), now());
        assert_eq!(result.next_episode, code(&episodes[3]));
        assert!(!result.out_of_order);
    }

    #[test]
    fn specials_are_excluded_from_every_aggregate() {
        let regular = [
            episode(1, 1, released(), true),
            episode(1, 2, released(), false),
        ];
        let baseline = progress(&show(ShowStatus::Returning, &regular), now());
        for special in [
            episode(0, 1, released(), false),
            episode(0, 1, released(), true),
            episode(0, 1, Schedule::Undated, false),
            episode(0, 1, future(), true),
        ] {
            let mut episodes = regular.to_vec();
            episodes.insert(0, special);
            let result = progress(&show(ShowStatus::Returning, &episodes), now());
            assert_eq!(result, baseline, "{special:?}");
        }
    }

    #[test]
    fn archived_episodes_leave_active_progress() {
        let mut gone = episode(1, 2, Schedule::Undated, false);
        gone.archived = true;
        let episodes = [episode(1, 1, released(), true), gone];
        let result = progress(&show(ShowStatus::Ended, &episodes), now());
        assert_eq!((result.watched, result.total), (1, 1));
        assert!(!result.release_info_incomplete);
        assert_eq!(result.state, ProgressState::Completed);
    }

    /// C05 derived states and C09 case 6, at the fixed clock.
    #[test]
    fn derived_state_table() {
        use ProgressState::*;
        use ShowStatus::*;

        let r = |watched| episode(1, 1, released(), watched);
        let r2 = |watched| episode(1, 2, released(), watched);
        let f = |watched| episode(1, 3, future(), watched);
        let u = |watched| episode(1, 4, Schedule::Undated, watched);
        let s = |watched| episode(0, 1, released(), watched);

        let cases: &[(&str, ShowStatus, bool, Vec<Episode>, ProgressState)] = &[
            ("empty returning", Returning, true, vec![], ReleaseUnknown),
            (
                "empty ended is never completed",
                Ended,
                true,
                vec![],
                ReleaseUnknown,
            ),
            ("specials only", Ended, true, vec![s(true)], ReleaseUnknown),
            (
                "future only",
                Returning,
                true,
                vec![f(false)],
                NotYetAvailable,
            ),
            (
                "future and undated",
                Returning,
                true,
                vec![f(false), u(false)],
                NotYetAvailable,
            ),
            (
                "undated only",
                Returning,
                true,
                vec![u(false)],
                ReleaseUnknown,
            ),
            (
                "undated only marked",
                Ended,
                true,
                vec![u(true)],
                ReleaseUnknown,
            ),
            (
                "future only marked returning",
                Returning,
                true,
                vec![f(true)],
                NotYetAvailable,
            ),
            (
                "ended future only marked",
                Ended,
                true,
                vec![f(true)],
                Completed,
            ),
            (
                "ongoing unwatched",
                Returning,
                true,
                vec![r(false), r2(false)],
                InProgress,
            ),
            (
                "ongoing gap",
                Returning,
                true,
                vec![r(false), r2(true)],
                InProgress,
            ),
            (
                "ongoing all released marked",
                Returning,
                true,
                vec![r(true), r2(true)],
                CaughtUp,
            ),
            (
                "ongoing caught up with future",
                Returning,
                true,
                vec![r(true), f(false)],
                CaughtUp,
            ),
            (
                "ended all marked",
                Ended,
                true,
                vec![r(true), r2(true)],
                Completed,
            ),
            (
                "canceled all marked",
                Canceled,
                true,
                vec![r(true), r2(true)],
                Completed,
            ),
            (
                "ended with marked future",
                Ended,
                true,
                vec![r(true), f(true)],
                Completed,
            ),
            (
                "ended with unmarked future",
                Ended,
                true,
                vec![r(true), f(false)],
                CaughtUp,
            ),
            (
                "ended with marked undated",
                Ended,
                true,
                vec![r(true), u(true)],
                CaughtUp,
            ),
            (
                "ended with unmarked special",
                Ended,
                true,
                vec![r(true), s(false)],
                Completed,
            ),
            (
                "ended incomplete catalog",
                Ended,
                false,
                vec![r(true), r2(true)],
                CaughtUp,
            ),
            (
                "unknown status all marked",
                Unknown,
                true,
                vec![r(true), r2(true)],
                CaughtUp,
            ),
            (
                "ended gap",
                Ended,
                true,
                vec![r(true), r2(false)],
                InProgress,
            ),
        ];
        for (name, status, complete_import, episodes, expected) in cases {
            let show = Show {
                status: *status,
                complete_import: *complete_import,
                episodes,
            };
            assert_eq!(progress(&show, now()).state, *expected, "{name}");
        }
    }

    #[test]
    fn empty_total_gives_zero_percent() {
        let result = progress(&show(ShowStatus::Returning, &[]), now());
        assert_eq!((result.watched, result.total, result.percent), (0, 0, 0));
        assert_eq!(result.next_episode, None);
        assert!(!result.out_of_order);
        assert!(!result.release_info_incomplete);
    }

    #[test]
    fn percent_rounds_down_so_only_all_marked_reaches_100() {
        assert_eq!(percent(2, 6), 33);
        assert_eq!(percent(2, 3), 66);
        assert_eq!(percent(199, 200), 99);
        assert_eq!(percent(200, 200), 100);
        assert_eq!(percent(usize::MAX, usize::MAX), 100);
    }

    #[test]
    fn release_crossing_the_utc_midnight_boundary_updates_progress_on_read() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let episodes = [
            episode(1, 1, released(), true),
            episode(1, 2, Schedule::Date(day), false),
        ];
        let show = show(ShowStatus::Returning, &episodes);
        let before = Utc.with_ymd_and_hms(2026, 11, 11, 23, 59, 59).unwrap();
        let after = Utc.with_ymd_and_hms(2026, 11, 12, 0, 0, 0).unwrap();
        assert_eq!(progress(&show, before).state, ProgressState::CaughtUp);
        assert_eq!(progress(&show, before).next_episode, None);
        assert_eq!(progress(&show, after).state, ProgressState::InProgress);
        assert_eq!(progress(&show, after).next_episode, code(&episodes[1]));
    }

    #[test]
    fn zoned_release_waits_for_local_midnight() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap();
        let starts_at = Utc.with_ymd_and_hms(2026, 11, 12, 5, 0, 0).unwrap();
        let episodes = [episode(
            1,
            1,
            Schedule::Zoned {
                date: day,
                zone: "America/New_York",
                starts_at,
            },
            false,
        )];
        let show = show(ShowStatus::Returning, &episodes);
        let utc_midnight = Utc.with_ymd_and_hms(2026, 11, 12, 0, 0, 0).unwrap();
        assert_eq!(
            progress(&show, utc_midnight).state,
            ProgressState::NotYetAvailable
        );
        assert_eq!(progress(&show, starts_at).state, ProgressState::InProgress);
    }

    #[test]
    fn state_names_match_the_api_contract() {
        assert_eq!(ProgressState::InProgress.as_str(), "in_progress");
        assert_eq!(ProgressState::CaughtUp.as_str(), "caught_up");
        assert_eq!(ProgressState::Completed.as_str(), "completed");
        assert_eq!(ProgressState::NotYetAvailable.as_str(), "not_yet_available");
        assert_eq!(ProgressState::ReleaseUnknown.as_str(), "release_unknown");
    }

    /// Exhaustive property check over every three-episode catalog built from released, future
    /// and undated episodes, marked or not, plus an optional special, for every show status and
    /// import completeness.
    #[test]
    fn invariants_hold_for_every_small_catalog() {
        let schedules = [released(), future(), Schedule::Undated];
        let statuses = [
            ShowStatus::Returning,
            ShowStatus::Ended,
            ShowStatus::Canceled,
            ShowStatus::Unknown,
        ];
        let variants: Vec<(Schedule, bool)> = schedules
            .iter()
            .flat_map(|&schedule| [(schedule, false), (schedule, true)])
            .collect();
        let mut checked = 0;
        for &a in &variants {
            for &b in &variants {
                for &c in &variants {
                    let regular: Vec<Episode> = [a, b, c]
                        .iter()
                        .zip(1..)
                        .map(|(&(schedule, watched), number)| episode(1, number, schedule, watched))
                        .collect();
                    for &status in &statuses {
                        for complete_import in [false, true] {
                            check_invariants(status, complete_import, &regular);
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 6 * 6 * 6 * 4 * 2);
    }

    fn check_invariants(status: ShowStatus, complete_import: bool, regular: &[Episode]) {
        let at = now();
        let base = Show {
            status,
            complete_import,
            episodes: regular,
        };
        let result = progress(&base, at);
        let context = format!("{status:?} complete={complete_import} {regular:?}");
        let state_of = |episode: &Episode| episode.schedule.release(at).state;

        assert_eq!(result.total as usize, regular.len(), "{context}");
        assert_eq!(
            result.watched as usize,
            regular.iter().filter(|e| e.watched).count(),
            "{context}"
        );
        assert_eq!(
            result.percent == 100,
            result.total > 0 && result.watched == result.total,
            "{context}"
        );

        // Next is the earliest released unmarked episode; nothing unreleased can be next.
        let expected_next = regular
            .iter()
            .find(|e| !e.watched && state_of(e) == ReleaseState::Released);
        assert_eq!(
            result.next_episode,
            expected_next.and_then(code),
            "{context}"
        );
        // Out of order: some unmarked episode, released or not, precedes a marked one.
        let first_gap = regular.iter().find(|e| !e.watched).map(|e| e.number);
        assert_eq!(
            result.out_of_order,
            first_gap.is_some_and(|n| regular.iter().any(|e| e.number > n && e.watched)),
            "{context}"
        );

        let undated = regular.iter().any(|e| state_of(e) == ReleaseState::Unknown);
        assert_eq!(result.release_info_incomplete, undated, "{context}");
        let completed = complete_import
            && matches!(status, ShowStatus::Ended | ShowStatus::Canceled)
            && regular.iter().all(|e| e.watched)
            && !undated;
        assert_eq!(
            result.state == ProgressState::Completed,
            completed,
            "{context}"
        );
        if !completed {
            let any_released = regular
                .iter()
                .any(|e| state_of(e) == ReleaseState::Released);
            let any_future = regular.iter().any(|e| state_of(e) == ReleaseState::Future);
            let expected = match (any_released, expected_next.is_some(), any_future) {
                (false, _, true) => ProgressState::NotYetAvailable,
                (false, _, false) => ProgressState::ReleaseUnknown,
                (true, true, _) => ProgressState::InProgress,
                (true, false, _) => ProgressState::CaughtUp,
            };
            assert_eq!(result.state, expected, "{context}");
        }

        // Input order and specials never change the result.
        let mut reversed = regular.to_vec();
        reversed.reverse();
        for special_watched in [false, true] {
            reversed.push(episode(0, 1, released(), special_watched));
            let shuffled = Show {
                episodes: &reversed,
                ..base
            };
            assert_eq!(progress(&shuffled, at), result, "{context}");
            reversed.pop();
        }
    }
}
