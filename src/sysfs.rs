//! Reading and writing /sys/class/power_supply.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{PSDIR, START_OFFSET};

pub fn read_u32(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn power_supplies() -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(PSDIR)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.sort();
    paths
}

pub fn find_battery() -> Option<PathBuf> {
    power_supplies()
        .into_iter()
        .find(|p| p.join("charge_control_end_threshold").exists())
}

/// Some(true) if any Mains-type supply is online; a phantom offline adapter
/// must not mask a live one.
pub fn on_ac() -> Option<bool> {
    let mut seen_mains = false;
    for p in power_supplies() {
        if let Ok(t) = fs::read_to_string(p.join("type")) {
            if t.trim() == "Mains" {
                seen_mains = true;
                if read_u32(&p.join("online")) == Some(1) {
                    return Some(true);
                }
            }
        }
    }
    seen_mains.then_some(false)
}

pub fn apply_ceiling(bat: &Path, ceiling: u32) -> std::io::Result<bool> {
    let end_p = bat.join("charge_control_end_threshold");
    let start_p = bat.join("charge_control_start_threshold");
    let cur_end = read_u32(&end_p);
    let start = ceiling.saturating_sub(START_OFFSET);
    let has_start = start_p.exists();
    // Repair a stale start threshold even when end already matches
    // (the EC can reset one across a power cycle but not the other).
    if cur_end == Some(ceiling) {
        if has_start && read_u32(&start_p) != Some(start) {
            fs::write(&start_p, format!("{start}\n"))?;
        }
        return Ok(false);
    }
    // Ordered so start never exceeds end mid-update.
    if ceiling > cur_end.unwrap_or(0) {
        fs::write(&end_p, format!("{ceiling}\n"))?;
        if has_start {
            fs::write(&start_p, format!("{start}\n"))?;
        }
    } else {
        if has_start {
            fs::write(&start_p, format!("{start}\n"))?;
        }
        fs::write(&end_p, format!("{ceiling}\n"))?;
    }
    Ok(true)
}
