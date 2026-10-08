//! R13 episode release rules (decisions.md, C05 Episode DTO). A user's watched mark never feeds
//! into these rules, so marking a future or undated episode keeps its release label.

use chrono::{DateTime, NaiveDate, Utc};

/// When an episode becomes available, as stored in the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Schedule {
    /// No air date from the provider; release is unresolved.
    Undated,
    /// Date-only release without a reliable timezone: starts at UTC midnight and is disclosed as
    /// estimated.
    Date(NaiveDate),
    /// Date in the provider's zone. `starts_at` is local midnight of `date` in that zone; the
    /// caller resolves it (PostgreSQL `air_date::timestamp AT TIME ZONE release_timezone`) because
    /// this module carries no timezone database.
    Zoned {
        date: NaiveDate,
        starts_at: DateTime<Utc>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseState {
    Released,
    Future,
    /// Undated: release is unresolved.
    Unknown,
}

impl ReleaseState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Released => "released",
            Self::Future => "future",
            Self::Unknown => "unknown",
        }
    }
}

/// Release classification at a given instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Release {
    pub state: ReleaseState,
    pub date: Option<NaiveDate>,
    /// True when the start instant uses the UTC-midnight fallback.
    pub estimated: bool,
}

impl Schedule {
    /// The instant the episode becomes released, or `None` when undated.
    pub fn starts_at(self) -> Option<DateTime<Utc>> {
        match self {
            Self::Undated => None,
            Self::Date(date) => Some(date.and_time(chrono::NaiveTime::MIN).and_utc()),
            Self::Zoned { starts_at, .. } => Some(starts_at),
        }
    }

    /// Classifies the release at `now`; an episode is released from its start instant onward.
    pub fn release(self, now: DateTime<Utc>) -> Release {
        let (date, estimated) = match self {
            Self::Undated => (None, false),
            Self::Date(date) => (Some(date), true),
            Self::Zoned { date, .. } => (Some(date), false),
        };
        let state = match self.starts_at() {
            None => ReleaseState::Unknown,
            Some(start) if now >= start => ReleaseState::Released,
            Some(_) => ReleaseState::Future,
        };
        Release {
            state,
            date,
            estimated,
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, s).unwrap()
    }

    #[test]
    fn date_only_release_starts_at_utc_midnight_and_is_estimated() {
        let schedule = Schedule::Date(date(2026, 11, 12));
        let cases = [
            (at(2026, 11, 11, 23, 59, 59), ReleaseState::Future),
            (at(2026, 11, 12, 0, 0, 0), ReleaseState::Released),
            (at(2026, 11, 12, 0, 0, 1), ReleaseState::Released),
        ];
        for (now, state) in cases {
            let release = schedule.release(now);
            assert_eq!(release.state, state, "at {now}");
            assert_eq!(release.date, Some(date(2026, 11, 12)));
            assert!(release.estimated);
        }
    }

    #[test]
    fn zoned_release_uses_resolved_local_midnight() {
        // Local midnight in America/New_York (UTC-5 in November) is 05:00 UTC; the UTC date has
        // already turned but the episode is still future.
        let east = Schedule::Zoned {
            date: date(2026, 11, 12),
            starts_at: at(2026, 11, 12, 5, 0, 0),
        };
        // Local midnight in Asia/Tokyo (UTC+9) is 15:00 UTC the previous day.
        let tokyo = Schedule::Zoned {
            date: date(2026, 11, 12),
            starts_at: at(2026, 11, 11, 15, 0, 0),
        };
        let cases = [
            (east, at(2026, 11, 12, 0, 0, 0), ReleaseState::Future),
            (east, at(2026, 11, 12, 4, 59, 59), ReleaseState::Future),
            (east, at(2026, 11, 12, 5, 0, 0), ReleaseState::Released),
            (tokyo, at(2026, 11, 11, 14, 59, 59), ReleaseState::Future),
            (tokyo, at(2026, 11, 11, 15, 0, 0), ReleaseState::Released),
        ];
        for (schedule, now, state) in cases {
            let release = schedule.release(now);
            assert_eq!(release.state, state, "{schedule:?} at {now}");
            assert!(!release.estimated);
            assert_eq!(release.date, Some(date(2026, 11, 12)));
        }
    }

    #[test]
    fn undated_release_is_unknown_at_any_instant() {
        for now in [
            DateTime::<Utc>::MIN_UTC,
            at(2026, 10, 8, 12, 0, 0),
            DateTime::<Utc>::MAX_UTC,
        ] {
            let release = Schedule::Undated.release(now);
            assert_eq!(release.state, ReleaseState::Unknown);
            assert_eq!(release.date, None);
            assert!(!release.estimated);
        }
        assert_eq!(Schedule::Undated.starts_at(), None);
    }

    #[test]
    fn state_names_match_the_api_contract() {
        assert_eq!(ReleaseState::Released.as_str(), "released");
        assert_eq!(ReleaseState::Future.as_str(), "future");
        assert_eq!(ReleaseState::Unknown.as_str(), "unknown");
    }
}
