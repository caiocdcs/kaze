//! When each followed series should get its next chapter, guessed from
//! how often it got one lately. Sites publish no schedules.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use ts_rs::TS;
use utoipa::ToSchema;

/// Release days looked at: the last ten give nine gaps.
const RECENT: usize = 10;
/// Nothing new for this long and a series counts as stalled.
const STALLED_AFTER: Duration = Duration::days(90);
/// Expected within this long (or overdue) is "this week".
const SOON: Duration = Duration::days(7);

/// The usual gap between a series' releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    pub last: DateTime<Utc>,
    pub every: Duration,
}

impl Cadence {
    /// From release dates in any order; `None` without two release days.
    /// Chapters out the same day count as one release, or a batch would
    /// make the gap zero.
    #[must_use]
    pub fn from_dates(dates: &[DateTime<Utc>]) -> Option<Self> {
        let mut days: Vec<DateTime<Utc>> = dates.to_vec();
        days.sort_unstable_by(|a, b| b.cmp(a));
        days.dedup_by_key(|d| d.date_naive());
        days.truncate(RECENT);
        let mut gaps: Vec<Duration> = days.windows(2).map(|w| w[0] - w[1]).collect();
        if gaps.is_empty() {
            return None;
        }
        gaps.sort_unstable();
        // The median: one odd gap (a hiatus, a double release) can't move it.
        let mid = gaps.len() / 2;
        let every = if gaps.len().is_multiple_of(2) {
            (gaps[mid - 1] + gaps[mid]) / 2
        } else {
            gaps[mid]
        };
        Some(Self {
            last: days[0],
            every,
        })
    }

    /// The gap in whole days, rounded, at least 1.
    #[must_use]
    pub fn days(&self) -> i64 {
        let half_day = Duration::hours(12);
        (self.every + half_day).num_days().max(1)
    }

    #[must_use]
    pub fn expected_at(&self) -> DateTime<Utc> {
        self.last + self.every
    }
}

/// Ordered as the page lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, TS, ToSchema)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum UpcomingState {
    /// Due within a week, or overdue.
    Expected,
    Later,
    /// Nothing new in 90 days.
    Stalled,
}

/// A followed series with its guessed next chapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS, ToSchema)]
#[ts(export)]
pub struct Upcoming {
    pub series_id: i64,
    pub title: String,
    pub last_published_at: DateTime<Utc>,
    /// Days between releases (rounded, at least 1), when there were two
    /// to compare.
    pub cadence_days: Option<i64>,
    pub expected_at: Option<DateTime<Utc>>,
    pub state: UpcomingState,
}

impl Upcoming {
    /// `None` for a series without any release date.
    #[must_use]
    pub fn new(
        series_id: i64,
        title: String,
        dates: &[DateTime<Utc>],
        now: DateTime<Utc>,
    ) -> Option<Self> {
        let last = dates.iter().max().copied()?;
        let cadence = Cadence::from_dates(dates);
        let expected_at = cadence.as_ref().map(Cadence::expected_at);
        let state = if now - last > STALLED_AFTER {
            UpcomingState::Stalled
        } else if expected_at.is_some_and(|at| at <= now + SOON) {
            UpcomingState::Expected
        } else {
            UpcomingState::Later
        };
        Some(Self {
            series_id,
            title,
            last_published_at: last,
            cadence_days: cadence.map(|c| c.days()),
            expected_at,
            state,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(n: i64) -> DateTime<Utc> {
        DateTime::UNIX_EPOCH + Duration::days(n)
    }

    #[test]
    fn cadence_needs_two_release_days() {
        assert_eq!(Cadence::from_dates(&[]), None);
        assert_eq!(Cadence::from_dates(&[day(3)]), None);
        // A batch on one day is one release.
        let batch = [day(3), day(3) + Duration::hours(2)];
        assert_eq!(Cadence::from_dates(&batch), None);
    }

    #[test]
    fn cadence_is_the_median_gap() {
        // Weekly, with one month-long break: still weekly.
        let dates = [day(0), day(7), day(14), day(44), day(51)];
        let c = Cadence::from_dates(&dates).unwrap();
        assert_eq!(c.last, day(51));
        assert_eq!(c.every, Duration::days(7));
        assert_eq!(c.expected_at(), day(58));
        // An even count averages the middle two: gaps 2, 4, 6, 8.
        let even = [day(0), day(2), day(6), day(12), day(20)];
        assert_eq!(Cadence::from_dates(&even).unwrap().every, Duration::days(5));
    }

    #[test]
    fn only_recent_releases_count() {
        // Monthly long ago, weekly in the last ten releases.
        let mut dates: Vec<_> = (0..5).map(|n| day(n * 30)).collect();
        dates.extend((0..10).map(|n| day(200 + n * 7)));
        assert_eq!(
            Cadence::from_dates(&dates).unwrap().every,
            Duration::days(7)
        );
    }

    #[test]
    fn states() {
        let weekly = [day(0), day(7), day(14)];
        let at = |now: i64| {
            Upcoming::new(1, "S".into(), &weekly, day(now))
                .unwrap()
                .state
        };
        assert_eq!(at(15), UpcomingState::Expected, "due on day 21");
        assert_eq!(at(25), UpcomingState::Expected, "overdue");
        assert_eq!(at(14 + 91), UpcomingState::Stalled);

        let monthly = [day(0), day(30)];
        let later = Upcoming::new(1, "S".into(), &monthly, day(31)).unwrap();
        assert_eq!(later.state, UpcomingState::Later);
        assert_eq!(later.cadence_days, Some(30));
        let nearly_weekly = [day(0), day(7) - Duration::hours(3)];
        let rounded = Upcoming::new(1, "S".into(), &nearly_weekly, day(8)).unwrap();
        assert_eq!(rounded.cadence_days, Some(7), "6 days 21 hours rounds up");
        let same_day = Cadence {
            last: day(1),
            every: Duration::hours(2),
        };
        assert_eq!(same_day.days(), 1);
        let once = Upcoming::new(1, "S".into(), &[day(0)], day(1)).unwrap();
        assert_eq!((once.state, once.expected_at), (UpcomingState::Later, None));
        assert_eq!(Upcoming::new(1, "S".into(), &[], day(1)), None);
    }
}
