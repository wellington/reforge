use chrono::{DateTime, Datelike, Timelike, Utc};
use std::collections::HashSet;

use crate::automerge::UpdateType;
use crate::config::ScheduleWindow;
use crate::orchestrator::UpdateCandidate;

/// Enforces the maximum number of concurrently open reforge MRs.
pub struct RateLimiter {
    max_open_mrs: Option<usize>,
    current_open: usize,
}

impl RateLimiter {
    pub fn new(max_open_mrs: Option<usize>, current_open: usize) -> Self {
        Self { max_open_mrs, current_open }
    }

    /// Returns `true` when a new MR may be created.
    pub fn can_create_mr(&self) -> bool {
        match self.max_open_mrs {
            None => true,
            Some(max) => self.current_open < max,
        }
    }

    /// Returns the number of additional MRs that may still be created.
    /// Returns `usize::MAX` when there is no limit.
    pub fn remaining_slots(&self) -> usize {
        match self.max_open_mrs {
            None => usize::MAX,
            Some(max) => max.saturating_sub(self.current_open),
        }
    }
}

/// Priority ordering for update candidates when the rate limit is in effect.
/// Lower discriminant = higher priority (created first).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PriorityOrder {
    Security = 0,
    Major = 1,
    Minor = 2,
    Patch = 3,
    Unknown = 4,
}

impl PriorityOrder {
    fn from_update_type(ut: Option<&UpdateType>) -> Self {
        match ut {
            Some(UpdateType::Major) => PriorityOrder::Major,
            Some(UpdateType::Minor) => PriorityOrder::Minor,
            Some(UpdateType::Patch) => PriorityOrder::Patch,
            None => PriorityOrder::Unknown,
        }
    }

    /// Public alias used from outside this module.
    pub fn from_update_type_pub(ut: Option<&UpdateType>) -> Self {
        Self::from_update_type(ut)
    }

    /// Derive priority, promoting to `Security` when the dependency name is
    /// present in the provided set of known-vulnerable package names.
    pub fn from_candidate(c: &UpdateCandidate, security_deps: &HashSet<String>) -> Self {
        if security_deps.contains(&c.dependency.name) {
            return PriorityOrder::Security;
        }
        let ut = UpdateType::classify(
            &c.dependency.current_version,
            &c.new_version.original_tag,
        );
        Self::from_update_type(ut.as_ref())
    }
}

/// Sorts `candidates` in-place by priority (highest priority first).
pub fn sort_candidates_by_priority(candidates: &mut Vec<UpdateCandidate>) {
    sort_candidates_by_priority_with_security(candidates, &HashSet::new());
}

/// Sorts `candidates` in-place, treating entries in `security_deps` as
/// `PriorityOrder::Security` so they are scheduled before all other updates.
pub fn sort_candidates_by_priority_with_security(
    candidates: &mut Vec<UpdateCandidate>,
    security_deps: &HashSet<String>,
) {
    candidates.sort_by_key(|c| PriorityOrder::from_candidate(c, security_deps));
}

/// Returns `true` when `now` falls within the given `ScheduleWindow`.
pub fn is_within_schedule_window(now: DateTime<Utc>, window: &ScheduleWindow) -> bool {
    if !window.days.is_empty() {
        let iso_weekday = now.weekday().number_from_monday();
        let day_allowed = window.days.iter().any(|d| d.iso_number() == iso_weekday);
        if !day_allowed {
            return false;
        }
    }

    let hour = now.hour();

    if let Some(start) = window.hours_start {
        if hour < start {
            return false;
        }
    }

    if let Some(end) = window.hours_end {
        if hour >= end {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ── RateLimiter ─────────────────────────────────────────────────────────

    #[test]
    fn rate_limiter_no_limit() {
        let rl = RateLimiter::new(None, 100);
        assert!(rl.can_create_mr());
        assert_eq!(rl.remaining_slots(), usize::MAX);
    }

    #[test]
    fn rate_limiter_under_limit() {
        let rl = RateLimiter::new(Some(5), 3);
        assert!(rl.can_create_mr());
        assert_eq!(rl.remaining_slots(), 2);
    }

    #[test]
    fn rate_limiter_at_limit() {
        let rl = RateLimiter::new(Some(5), 5);
        assert!(!rl.can_create_mr());
        assert_eq!(rl.remaining_slots(), 0);
    }

    #[test]
    fn rate_limiter_over_limit() {
        let rl = RateLimiter::new(Some(5), 7);
        assert!(!rl.can_create_mr());
        assert_eq!(rl.remaining_slots(), 0);
    }

    #[test]
    fn rate_limiter_zero_limit() {
        let rl = RateLimiter::new(Some(0), 0);
        assert!(!rl.can_create_mr());
        assert_eq!(rl.remaining_slots(), 0);
    }

    // ── PriorityOrder ────────────────────────────────────────────────────────

    #[test]
    fn priority_ordering() {
        assert!(PriorityOrder::Security < PriorityOrder::Major);
        assert!(PriorityOrder::Major < PriorityOrder::Minor);
        assert!(PriorityOrder::Minor < PriorityOrder::Patch);
        assert!(PriorityOrder::Patch < PriorityOrder::Unknown);
    }

    #[test]
    fn priority_from_update_type_major() {
        assert_eq!(
            PriorityOrder::from_update_type(Some(&UpdateType::Major)),
            PriorityOrder::Major
        );
    }

    #[test]
    fn priority_from_update_type_minor() {
        assert_eq!(
            PriorityOrder::from_update_type(Some(&UpdateType::Minor)),
            PriorityOrder::Minor
        );
    }

    #[test]
    fn priority_from_update_type_patch() {
        assert_eq!(
            PriorityOrder::from_update_type(Some(&UpdateType::Patch)),
            PriorityOrder::Patch
        );
    }

    #[test]
    fn priority_from_update_type_none() {
        assert_eq!(
            PriorityOrder::from_update_type(None),
            PriorityOrder::Unknown
        );
    }

    // ── is_within_schedule_window ────────────────────────────────────────────

    fn utc(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap()
    }

    #[test]
    fn schedule_no_restrictions() {
        let window = ScheduleWindow { days: vec![], hours_start: None, hours_end: None };
        // Any time should pass
        assert!(is_within_schedule_window(utc(2026, 4, 6, 3), &window));
    }

    #[test]
    fn schedule_allowed_day() {
        use crate::config::Weekday;
        // 2026-04-06 is a Monday
        let window = ScheduleWindow {
            days: vec![Weekday::Monday, Weekday::Wednesday],
            hours_start: None,
            hours_end: None,
        };
        assert!(is_within_schedule_window(utc(2026, 4, 6, 10), &window));
    }

    #[test]
    fn schedule_disallowed_day() {
        use crate::config::Weekday;
        // 2026-04-07 is a Tuesday
        let window = ScheduleWindow {
            days: vec![Weekday::Monday, Weekday::Wednesday],
            hours_start: None,
            hours_end: None,
        };
        assert!(!is_within_schedule_window(utc(2026, 4, 7, 10), &window));
    }

    #[test]
    fn schedule_within_hours() {
        let window = ScheduleWindow {
            days: vec![],
            hours_start: Some(9),
            hours_end: Some(17),
        };
        assert!(is_within_schedule_window(utc(2026, 4, 6, 9), &window));
        assert!(is_within_schedule_window(utc(2026, 4, 6, 16), &window));
    }

    #[test]
    fn schedule_before_hours_start() {
        let window = ScheduleWindow {
            days: vec![],
            hours_start: Some(9),
            hours_end: Some(17),
        };
        assert!(!is_within_schedule_window(utc(2026, 4, 6, 8), &window));
    }

    #[test]
    fn schedule_at_or_after_hours_end() {
        let window = ScheduleWindow {
            days: vec![],
            hours_start: Some(9),
            hours_end: Some(17),
        };
        assert!(!is_within_schedule_window(utc(2026, 4, 6, 17), &window));
        assert!(!is_within_schedule_window(utc(2026, 4, 6, 20), &window));
    }

    #[test]
    fn schedule_day_and_hours_combined() {
        use crate::config::Weekday;
        // 2026-04-06 is Monday
        let window = ScheduleWindow {
            days: vec![Weekday::Monday],
            hours_start: Some(9),
            hours_end: Some(17),
        };
        assert!(is_within_schedule_window(utc(2026, 4, 6, 12), &window));
        // Correct day but wrong hour
        assert!(!is_within_schedule_window(utc(2026, 4, 6, 8), &window));
        // Correct hour but wrong day (Tuesday)
        assert!(!is_within_schedule_window(utc(2026, 4, 7, 12), &window));
    }
}
