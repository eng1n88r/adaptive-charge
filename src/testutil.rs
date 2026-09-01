//! Shared fixtures for module tests.

use chrono::{Local, TimeZone};

use crate::history::Event;

pub fn ev(ts: i64, kind: &str) -> Event {
    Event {
        ts,
        event: kind.into(),
        capacity: Some(50),
        seeded: false,
    }
}

pub fn local_ts(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
    Local
        .with_ymd_and_hms(y, mo, d, h, mi, 0)
        .single()
        .unwrap()
        .timestamp()
}

/// Three Thursdays of "plug in Wednesday 18:00, unplug Thursday 00:45" —
/// a departure whose lead window reaches back across midnight.
pub fn midnight_synthetic() -> Vec<Event> {
    let mut out = Vec::new();
    for d in [10u32, 17, 24] {
        out.push(ev(local_ts(2026, 9, d - 1, 18, 0), "plug"));
        out.push(ev(local_ts(2026, 9, d, 0, 45), "unplug"));
    }
    out
}

/// Three weeks of "plug in 23:00, unplug 08:30 weekdays / 11:00 weekends",
/// starting Monday 2026-09-07.
pub fn synthetic() -> Vec<Event> {
    let mut out = Vec::new();
    for week in 0..3u32 {
        for day in 0..7u32 {
            let d = 7 + week * 7 + day;
            let (h, m) = if day >= 5 { (11, 0) } else { (8, 30) };
            out.push(ev(local_ts(2026, 9, d - 1, 23, 0), "plug"));
            out.push(ev(local_ts(2026, 9, d, h, m), "unplug"));
        }
    }
    out
}
