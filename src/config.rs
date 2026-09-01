//! Tunables and shared paths.

use std::env;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const PSDIR: &str = "/sys/class/power_supply";
pub const HOLD_CEILING: u32 = 80;
pub const FULL_CEILING: u32 = 100;
pub const LEAD_MINUTES: i64 = 90;
pub const MIN_SESSION_HOURS: f64 = 4.0;
pub const MIN_SAMPLES: usize = 3;
pub const START_OFFSET: u32 = 5;
pub const MAX_SLEEP: Duration = Duration::from_secs(6 * 3600);
pub const MIN_SLEEP: Duration = Duration::from_secs(30);

pub fn state_dir() -> PathBuf {
    env::var("ADAPTIVE_CHARGE_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/adaptive-charge"))
}

pub fn history_path() -> PathBuf {
    state_dir().join("history.jsonl")
}

pub fn override_path() -> PathBuf {
    state_dir().join("override")
}

pub fn pid_path() -> PathBuf {
    state_dir().join("daemon.pid")
}

pub fn ceiling_path() -> PathBuf {
    state_dir().join("ceiling")
}

/// User-configured hold ceiling (`adaptive-charge ceiling <n>`), falling back
/// to HOLD_CEILING. Read at each decision, so changes apply without restart.
pub fn hold_ceiling() -> u32 {
    std::fs::read_to_string(ceiling_path())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|v| (50..=100).contains(v))
        .unwrap_or(HOLD_CEILING)
}

pub fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
