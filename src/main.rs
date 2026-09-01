//! adaptive-charge: event-driven battery charge ceiling.
//!
//! Holds the battery at HOLD_CEILING on AC, learns the per-weekday time the
//! machine is usually unplugged, and lifts the ceiling to FULL_CEILING
//! LEAD_MINUTES beforehand. Wakes on UPower AC changes and logind resume;
//! otherwise sleeps until the next decision boundary.

mod config;
mod history;
mod model;
mod policy;
mod sysfs;
#[cfg(test)]
mod testutil;
mod watch;

use std::collections::HashSet;
use std::env;
use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use chrono::{Local, TimeZone};

use config::{override_path, pid_path, state_dir, unix_now, MIN_SAMPLES, MIN_SESSION_HOURS};
use history::{append_event, append_events, load_history, seed_transitions};
use model::{learn, median, ModelCache};
use policy::{decide, next_sleep};
use sysfs::{apply_ceiling, find_battery, on_ac, read_u32};
use watch::{spawn_resume_watch, spawn_signal_watch, spawn_upower_watch, upower_charge_history};

type AnyError = Box<dyn std::error::Error>;

fn cmd_run() -> Result<i32, AnyError> {
    let bat = find_battery().ok_or("no battery exposes charge_control_end_threshold")?;
    println!("adaptive-charge watching {}", bat.display());

    let _ = fs::create_dir_all(state_dir())
        .and_then(|_| fs::write(pid_path(), format!("{}\n", std::process::id())));

    let (tx, rx) = mpsc::channel();
    spawn_upower_watch(tx.clone());
    spawn_resume_watch(tx.clone());
    spawn_signal_watch(tx);

    let mut cache = ModelCache::default();
    let mut last_ac: Option<bool> = None;
    let mut last_ceiling: Option<u32> = None;

    loop {
        let ac = on_ac();
        let capacity = read_u32(&bat.join("capacity")).map(i64::from);

        if let (Some(prev), Some(cur)) = (last_ac, ac) {
            if prev != cur {
                let kind = if cur { "plug" } else { "unplug" };
                match append_event(kind, capacity) {
                    Ok(()) => println!("event: {kind} at {capacity:?}%"),
                    Err(e) => eprintln!("history write failed: {e}"),
                }
            }
        }
        last_ac = ac;

        let model = cache.get();
        let (ceiling, reason) = decide(&Local::now(), &model, ac);
        match apply_ceiling(&bat, ceiling) {
            Ok(changed) => {
                if changed || last_ceiling != Some(ceiling) {
                    println!("ceiling -> {ceiling}% ({reason})");
                    last_ceiling = Some(ceiling);
                }
            }
            Err(e) => eprintln!("failed to write ceiling: {e}"),
        }

        match rx.recv_timeout(next_sleep(&Local::now(), &model, ac)) {
            Ok(source) => println!("wake: {source}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => thread::sleep(Duration::from_secs(60)),
        }
    }
}

fn cmd_status() -> Result<i32, AnyError> {
    let bat = find_battery().ok_or("no battery exposes charge_control_end_threshold")?;
    let events = load_history();
    let model = learn(&events);
    let ac = on_ac();
    let (ceiling, reason) = decide(&Local::now(), &model, ac);
    let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    println!("battery      : {}", bat.display());
    println!(
        "capacity     : {}%",
        read_u32(&bat.join("capacity")).unwrap_or(0)
    );
    println!("on AC        : {ac:?}");
    println!(
        "ceiling now  : {}%",
        read_u32(&bat.join("charge_control_end_threshold")).unwrap_or(0)
    );
    println!(
        "start now    : {}%",
        read_u32(&bat.join("charge_control_start_threshold")).unwrap_or(0)
    );
    println!("hold ceiling : {}%", config::hold_ceiling());
    println!();
    println!("transitions recorded : {}", events.len());
    println!(
        "qualifying departures: {} (AC sessions over {MIN_SESSION_HOURS}h)",
        model.buckets.values().map(Vec::len).sum::<usize>()
    );
    println!();
    println!("learned departure times:");
    if model.buckets.is_empty() {
        println!("  (none yet - it needs a few days of plug/unplug history)");
    }
    for wd in 0..7u32 {
        let Some(mins) = model.buckets.get(&wd) else {
            continue;
        };
        let med = median(mins);
        let trusted = if mins.len() >= MIN_SAMPLES {
            "used".to_string()
        } else {
            format!("needs {} more", MIN_SAMPLES - mins.len())
        };
        println!(
            "  {}  {:02}:{:02}   n={}  ({trusted})",
            names[wd as usize],
            med / 60,
            med % 60,
            mins.len()
        );
    }
    if let Some(fb) = model.fallback {
        println!("  fallback across all days: {:02}:{:02}", fb / 60, fb % 60);
    }
    println!();
    println!("decision     : {ceiling}%");
    println!("reason       : {reason}");
    Ok(0)
}

fn cmd_seed(write: bool) -> Result<i32, AnyError> {
    let bat_name = find_battery()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "BAT0".into());
    let samples = upower_charge_history(&bat_name)?;
    let events = seed_transitions(&samples);
    if samples.is_empty() {
        println!("UPower returned no history - nothing to seed");
        return Ok(1);
    }

    let fmt = |ts: i64| {
        Local
            .timestamp_opt(ts, 0)
            .single()
            .map(|d| d.format("%a %m-%d %H:%M").to_string())
            .unwrap_or_else(|| ts.to_string())
    };
    println!("upower samples : {}", samples.len());
    println!("transitions    : {}", events.len());
    println!();
    for e in &events {
        println!(
            "  {}  {:<7} at {:>3}%",
            fmt(e.ts),
            e.event,
            e.capacity.unwrap_or(-1)
        );
    }

    if !write {
        println!("\n(preview only - pass --write to merge into the daemon's history)");
        return Ok(0);
    }

    // Append-only: load_history() sorts on read, so order in the file does
    // not matter and a running daemon's own appends are never clobbered.
    let existing = load_history();
    let seen: HashSet<_> = existing.iter().map(|e| (e.ts, e.event.clone())).collect();
    let mut new: Vec<_> = events
        .into_iter()
        .filter(|e| !seen.contains(&(e.ts, e.event.clone())))
        .collect();
    new.sort_by_key(|e| e.ts);
    append_events(&new)?;
    println!(
        "\nappended {} new events ({} total) to {}",
        new.len(),
        existing.len() + new.len(),
        config::history_path().display()
    );
    Ok(0)
}

/// Ring the running daemon's SIGHUP doorbell so a pin change applies now,
/// not at its next scheduled wake.
fn notify_daemon() {
    let pid = fs::read_to_string(pid_path())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        // Guard against a recycled PID: only signal a process that is
        // actually adaptive-charge, never whatever inherited the number.
        .filter(|p| {
            fs::read_to_string(format!("/proc/{p}/comm"))
                .is_ok_and(|c| c.trim() == "adaptive-charge")
        });
    let Some(pid) = pid else {
        println!("(daemon not running - applies at its next wake)");
        return;
    };
    match std::process::Command::new("kill")
        .args(["-HUP", &pid.to_string()])
        .status()
    {
        Ok(s) if s.success() => println!("(daemon notified)"),
        _ => println!("(could not notify daemon - applies at its next wake)"),
    }
}

fn cmd_ceiling(args: &[String]) -> Result<i32, AnyError> {
    let Some(value) = args.first().and_then(|a| a.parse::<u32>().ok()) else {
        eprintln!("usage: adaptive-charge ceiling <50-100>");
        return Ok(1);
    };
    if !(50..=100).contains(&value) {
        eprintln!("ceiling must be between 50 and 100");
        return Ok(1);
    }
    fs::create_dir_all(state_dir())?;
    fs::write(config::ceiling_path(), format!("{value}\n"))?;
    println!("hold ceiling set to {value}%");
    notify_daemon();
    Ok(0)
}

fn cmd_pin(args: &[String]) -> Result<i32, AnyError> {
    let Some(ceiling) = args.first().and_then(|a| a.parse::<u32>().ok()) else {
        eprintln!("usage: adaptive-charge pin <ceiling%> [hours]");
        return Ok(1);
    };
    if !(20..=100).contains(&ceiling) {
        eprintln!("ceiling must be between 20 and 100");
        return Ok(1);
    }
    let hours = match args.get(1) {
        None => 4.0,
        Some(a) => match a.parse::<f64>() {
            Ok(h) if h.is_finite() && h > 0.0 && h <= 24.0 * 7.0 => h,
            _ => {
                eprintln!("hours must be a positive number up to 168 (got '{a}')");
                return Ok(1);
            }
        },
    };
    fs::create_dir_all(state_dir())?;
    fs::write(
        override_path(),
        format!("{ceiling} {}\n", unix_now() + hours * 3600.0),
    )?;
    let until = Local::now() + chrono::Duration::seconds((hours * 3600.0) as i64);
    println!("pinned at {ceiling}% until {}", until.format("%H:%M"));
    notify_daemon();
    Ok(0)
}

fn cmd_unpin() -> Result<i32, AnyError> {
    match fs::remove_file(override_path()) {
        Ok(()) => {
            println!("pin removed");
            notify_daemon();
        }
        Err(_) => println!("no pin set"),
    }
    Ok(0)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("status");
    let result = match cmd {
        "run" => cmd_run(),
        "status" => cmd_status(),
        "seed" => cmd_seed(args.iter().any(|a| a == "--write")),
        "ceiling" => cmd_ceiling(&args[1..]),
        "pin" => cmd_pin(&args[1..]),
        "unpin" => cmd_unpin(),
        other => {
            eprintln!(
                "unknown command: {other} (run | status | seed [--write] | ceiling <n> | pin | unpin)"
            );
            Ok(1)
        }
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("adaptive-charge: {e}");
            std::process::exit(1);
        }
    }
}
