//! Choosing the ceiling and when to look again.

use std::fs;
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, Timelike};

use crate::config::{
    hold_ceiling, override_path, unix_now, FULL_CEILING, LEAD_MINUTES, MAX_SLEEP, MIN_SLEEP,
};
use crate::model::Model;

const DAY: i64 = 24 * 60;
/// Re-evaluate shortly after midnight so the weekday rolls over.
const ROLLOVER: i64 = DAY + 5;

pub fn read_override() -> Option<(u32, f64)> {
    let s = fs::read_to_string(override_path()).ok()?;
    let mut it = s.split_whitespace();
    let ceiling: u32 = it.next()?.parse().ok()?;
    let expiry: f64 = it.next()?.parse().ok()?;
    (unix_now() < expiry).then_some((ceiling, expiry))
}

fn target_for(model: &Model, wd: u32) -> (Option<i64>, &'static str) {
    match model.schedule.get(&wd) {
        Some(t) => (Some(*t), "this weekday"),
        None => (model.fallback, "all days (fallback)"),
    }
}

fn hhmm(t: i64) -> String {
    format!("{:02}:{:02}", (t % DAY) / 60, t % 60)
}

/// `ac == None` (unreadable adapter state) is treated as on-AC in both
/// decide() and next_sleep(), so the two never disagree about scheduling.
pub fn decide(now: &DateTime<Local>, model: &Model, ac: Option<bool>) -> (u32, String) {
    if let Some((ceiling, expiry)) = read_override() {
        let left = (expiry - unix_now()) / 3600.0;
        return (ceiling, format!("pinned to {ceiling}% for another {left:.1}h"));
    }
    if ac == Some(false) {
        return (
            hold_ceiling(),
            "on battery - ceiling is inert until you plug in".into(),
        );
    }
    let wd = now.weekday().num_days_from_monday();
    let now_min = (now.hour() * 60 + now.minute()) as i64;
    let (today, source) = target_for(model, wd);
    let tomorrow = target_for(model, (wd + 1) % 7).0;

    if let Some(t) = today {
        let delta = t - now_min;
        if (0..=LEAD_MINUTES).contains(&delta) {
            return (
                FULL_CEILING,
                format!(
                    "departure predicted at {} ({source}), {delta} min out - topping up",
                    hhmm(t)
                ),
            );
        }
    }
    // A departure shortly after midnight needs its lead window tonight.
    if let Some(t) = tomorrow {
        let delta = t + DAY - now_min;
        if (0..=LEAD_MINUTES).contains(&delta) {
            return (
                FULL_CEILING,
                format!(
                    "departure predicted at {} (tomorrow), {delta} min out - topping up",
                    hhmm(t)
                ),
            );
        }
    }
    match today {
        None => (
            hold_ceiling(),
            "not enough history yet - holding at the safe default".into(),
        ),
        Some(t) if t < now_min => (
            hold_ceiling(),
            format!(
                "predicted departure {} ({source}) already passed - back to hold",
                hhmm(t)
            ),
        ),
        Some(t) => {
            let delta = t - now_min;
            (
                hold_ceiling(),
                format!(
                    "departure predicted at {} ({source}), {}h{:02}m out - holding",
                    hhmm(t),
                    delta / 60,
                    delta % 60
                ),
            )
        }
    }
}

/// Time until the decision could next change on its own: top-up start or end
/// (today's or tomorrow's, if its lead window reaches back past midnight),
/// pin expiry, or the day rolling over.
pub fn next_sleep(now: &DateTime<Local>, model: &Model, ac: Option<bool>) -> Duration {
    let clamp = |d: Duration| d.clamp(MIN_SLEEP, MAX_SLEEP);
    if let Some((_, expiry)) = read_override() {
        return clamp(Duration::from_secs_f64((expiry - unix_now()).max(1.0)));
    }
    if ac == Some(false) {
        return MAX_SLEEP;
    }
    let wd = now.weekday().num_days_from_monday();
    let today = target_for(model, wd).0;
    let tomorrow = target_for(model, (wd + 1) % 7).0;
    if today.is_none() && tomorrow.is_none() {
        return MAX_SLEEP;
    }
    let now_min = (now.hour() * 60 + now.minute()) as i64;
    let mut candidates = vec![ROLLOVER];
    if let Some(t) = today {
        candidates.push(t - LEAD_MINUTES);
        candidates.push(t);
    }
    if let Some(t) = tomorrow {
        candidates.push(t + DAY - LEAD_MINUTES);
        candidates.push(t + DAY);
    }
    let boundary = candidates
        .into_iter()
        .filter(|c| *c > now_min)
        .min()
        .unwrap_or(ROLLOVER);
    let secs = (boundary - now_min) * 60 - now.second() as i64;
    clamp(Duration::from_secs(secs.max(1) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HOLD_CEILING;
    use crate::model::learn;
    use crate::testutil::*;
    use chrono::TimeZone;

    fn at(h: u32, mi: u32) -> DateTime<Local> {
        // 2026-09-09 is a Wednesday; learned target 08:30.
        Local.with_ymd_and_hms(2026, 9, 9, h, mi, 0).single().unwrap()
    }

    #[test]
    fn decision_boundaries() {
        let model = learn(&synthetic());
        assert_eq!(decide(&at(6, 59), &model, Some(true)).0, HOLD_CEILING);
        assert_eq!(decide(&at(7, 0), &model, Some(true)).0, FULL_CEILING);
        assert_eq!(decide(&at(8, 29), &model, Some(true)).0, FULL_CEILING);
        assert_eq!(decide(&at(8, 31), &model, Some(true)).0, HOLD_CEILING);
        assert_eq!(decide(&at(7, 30), &model, Some(false)).0, HOLD_CEILING);
    }

    #[test]
    fn unknown_ac_schedules_like_on_ac() {
        let model = learn(&synthetic());
        assert_eq!(decide(&at(7, 30), &model, None).0, FULL_CEILING);
        assert_eq!(next_sleep(&at(7, 30), &model, None).as_secs(), 3600);
    }

    #[test]
    fn sleep_targets_the_next_boundary() {
        let model = learn(&synthetic());
        assert_eq!(next_sleep(&at(2, 0), &model, Some(true)).as_secs(), 5 * 3600);
        assert_eq!(next_sleep(&at(7, 30), &model, Some(true)).as_secs(), 3600);
        assert_eq!(next_sleep(&at(7, 30), &model, Some(false)), MAX_SLEEP);
    }

    #[test]
    fn lead_window_crosses_midnight() {
        // Departures at 00:45 on Thursdays; Wednesday 23:30 is inside the
        // 90-minute window that starts at 23:15 the night before.
        let model = learn(&midnight_synthetic());
        let (ceiling, reason) = decide(&at(23, 30), &model, Some(true));
        assert_eq!(ceiling, FULL_CEILING);
        assert!(reason.contains("tomorrow"), "{reason}");
        // Next boundary is the 24:05 rollover (75 min before the 00:45 end).
        assert_eq!(next_sleep(&at(23, 30), &model, Some(true)).as_secs(), 35 * 60);
        // 22:00 is before the window: wake exactly at 23:15.
        assert_eq!(decide(&at(22, 0), &model, Some(true)).0, HOLD_CEILING);
        assert_eq!(next_sleep(&at(22, 0), &model, Some(true)).as_secs(), 75 * 60);
    }
}
