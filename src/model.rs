//! Learning departure times from the event log.

use std::collections::HashMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Local, TimeZone, Timelike};

use crate::config::{history_path, MIN_SAMPLES, MIN_SESSION_HOURS};
use crate::history::{load_history, Event};

#[derive(Clone, Default)]
pub struct Model {
    /// weekday (0 = Mon) -> median departure, minutes since midnight
    pub schedule: HashMap<u32, i64>,
    pub fallback: Option<i64>,
    pub buckets: HashMap<u32, Vec<i64>>,
}

/// Unplugs that ended an AC session of MIN_SESSION_HOURS+, as minutes since
/// midnight per weekday. Shorter unplugs are noise, not departures.
fn departures(events: &[Event]) -> HashMap<u32, Vec<i64>> {
    let mut out: HashMap<u32, Vec<i64>> = HashMap::new();
    let mut last_plug: Option<i64> = None;
    for e in events {
        match e.event.as_str() {
            "plug" => last_plug = Some(e.ts),
            "unplug" => {
                if let Some(p) = last_plug.take() {
                    if (e.ts - p) as f64 / 3600.0 >= MIN_SESSION_HOURS {
                        if let Some(dt) = Local.timestamp_opt(e.ts, 0).single() {
                            out.entry(dt.weekday().num_days_from_monday())
                                .or_default()
                                .push((dt.hour() * 60 + dt.minute()) as i64);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

pub fn median(values: &[i64]) -> i64 {
    let mut v = values.to_vec();
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    }
}

pub fn learn(events: &[Event]) -> Model {
    let buckets = departures(events);
    let mut schedule = HashMap::new();
    for (wd, mins) in &buckets {
        if mins.len() >= MIN_SAMPLES {
            schedule.insert(*wd, median(mins));
        }
    }
    let all: Vec<i64> = buckets.values().flatten().copied().collect();
    let fallback = (all.len() >= MIN_SAMPLES).then(|| median(&all));
    Model {
        schedule,
        fallback,
        buckets,
    }
}

/// Recomputes the model only when the history file's (mtime, size) changes.
#[derive(Default)]
pub struct ModelCache {
    stamp: Option<(SystemTime, u64)>,
    model: Option<Model>,
}

impl ModelCache {
    pub fn get(&mut self) -> Model {
        let stamp = fs::metadata(history_path())
            .ok()
            .map(|m| (m.modified().unwrap_or(UNIX_EPOCH), m.len()));
        if stamp != self.stamp || self.model.is_none() {
            self.model = Some(learn(&load_history()));
            self.stamp = stamp;
        }
        self.model.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    #[test]
    fn learns_weekday_medians() {
        let model = learn(&synthetic());
        assert_eq!(model.schedule.get(&0), Some(&(8 * 60 + 30)));
        assert_eq!(model.schedule.get(&5), Some(&(11 * 60)));
        assert_eq!(model.buckets.values().map(Vec::len).sum::<usize>(), 21);
    }

    #[test]
    fn short_unplugs_are_not_departures() {
        let mut events = synthetic();
        events.push(ev(local_ts(2026, 9, 9, 14, 0), "plug"));
        events.push(ev(local_ts(2026, 9, 9, 14, 20), "unplug"));
        let model = learn(&events);
        assert_eq!(model.buckets.values().map(Vec::len).sum::<usize>(), 21);
    }

    #[test]
    fn median_of_even_and_odd() {
        assert_eq!(median(&[10, 20, 30]), 20);
        assert_eq!(median(&[10, 20, 30, 40]), 25);
    }
}
