//! The plug/unplug event log, and seeding it from UPower samples.

use std::fs;
use std::io::Write as _;

use serde::{Deserialize, Serialize};

use crate::config::{history_path, state_dir, unix_now};

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Event {
    pub ts: i64,
    pub event: String,
    #[serde(default)]
    pub capacity: Option<i64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub seeded: bool,
}

/// Sorted by timestamp, so learners can rely on order regardless of how the
/// file was written (daemon appends and seed merges may interleave).
pub fn load_history() -> Vec<Event> {
    let Ok(text) = fs::read_to_string(history_path()) else {
        return Vec::new();
    };
    let mut events: Vec<Event> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l.trim()).ok())
        .collect();
    events.sort_by_key(|e| e.ts);
    events
}

/// Append-only writer: a single O_APPEND write per call, so a concurrently
/// appending daemon and a `seed --write` cannot clobber each other and
/// readers never see a torn rewrite.
pub fn append_events(events: &[Event]) -> std::io::Result<()> {
    fs::create_dir_all(state_dir())?;
    let mut out = String::new();
    for e in events {
        out.push_str(&serde_json::to_string(e)?);
        out.push('\n');
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(history_path())?;
    f.write_all(out.as_bytes())
}

pub fn append_event(kind: &str, capacity: Option<i64>) -> std::io::Result<()> {
    append_events(&[Event {
        ts: unix_now() as i64,
        event: kind.to_string(),
        capacity,
        seeded: false,
    }])
}

/// Collapse UPower's sample stream into plug/unplug events.
/// States: 1 charging, 4 fully-charged, 5 pending-charge (AC — pending-charge
/// is what a threshold-held battery reports while plugged in); 2 discharging
/// (battery).
pub fn seed_transitions(samples: &[(u32, f64, u32)]) -> Vec<Event> {
    let mut sorted: Vec<_> = samples
        .iter()
        .filter(|(ts, _, _)| *ts > 1_000_000_000)
        .copied()
        .collect();
    sorted.sort_by_key(|s| s.0);
    let mut out = Vec::new();
    let mut prev: Option<bool> = None;
    for (ts, val, state) in sorted {
        let ac = match state {
            1 | 4 | 5 => true,
            2 => false,
            _ => continue,
        };
        if prev.is_some() && prev != Some(ac) {
            out.push(Event {
                ts: ts as i64,
                event: if ac { "plug" } else { "unplug" }.into(),
                capacity: Some(val as i64),
                seeded: true,
            });
        }
        prev = Some(ac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_collapses_samples_into_transitions() {
        // discharging -> charging -> fully-charged -> discharging
        let samples = vec![
            (1_700_000_000u32, 40.0, 2u32),
            (1_700_000_100, 41.0, 1), // plug
            (1_700_000_200, 99.0, 4), // still AC: no event
            (1_700_000_300, 98.0, 2), // unplug
            (999, 50.0, 1),           // bogus timestamp: dropped
        ];
        let events = seed_transitions(&samples);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "plug");
        assert_eq!(events[1].event, "unplug");
        assert!(events.iter().all(|e| e.seeded));
    }

    #[test]
    fn pending_charge_counts_as_ac() {
        // A threshold-held battery reports pending-charge (5) while plugged in.
        let samples = vec![
            (1_700_000_000u32, 60.0, 2u32),
            (1_700_000_100, 80.0, 5), // plug (held at the cap)
            (1_700_020_000, 80.0, 5),
            (1_700_040_000, 79.0, 2), // unplug
        ];
        let events = seed_transitions(&samples);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "plug");
        assert_eq!(events[1].event, "unplug");
    }
}
