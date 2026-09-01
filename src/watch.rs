//! D-Bus: wake sources (UPower AC changes, logind resume) and history fetch.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub fn spawn_upower_watch(tx: mpsc::Sender<&'static str>) {
    thread::spawn(move || loop {
        let attempt = (|| -> zbus::Result<()> {
            let conn = zbus::blocking::Connection::system()?;
            let proxy = zbus::blocking::Proxy::new(
                &conn,
                "org.freedesktop.UPower",
                "/org/freedesktop/UPower",
                "org.freedesktop.UPower",
            )?;
            for _change in proxy.receive_property_changed::<bool>("OnBattery") {
                if tx.send("AC state changed").is_err() {
                    return Ok(());
                }
            }
            Ok(())
        })();
        if let Err(e) = attempt {
            eprintln!("upower watch: {e}; retrying in 30s");
        }
        thread::sleep(Duration::from_secs(30));
    });
}

pub fn spawn_resume_watch(tx: mpsc::Sender<&'static str>) {
    thread::spawn(move || loop {
        let attempt = (|| -> zbus::Result<()> {
            let conn = zbus::blocking::Connection::system()?;
            let proxy = zbus::blocking::Proxy::new(
                &conn,
                "org.freedesktop.login1",
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
            )?;
            for msg in proxy.receive_signal("PrepareForSleep")? {
                if let Ok(entering_sleep) = msg.body().deserialize::<bool>() {
                    if !entering_sleep && tx.send("resumed from sleep").is_err() {
                        return Ok(());
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = attempt {
            eprintln!("logind watch: {e}; retrying in 30s");
        }
        thread::sleep(Duration::from_secs(30));
    });
}

/// SIGHUP is the "pin changed" doorbell rung by `pin`/`unpin`.
pub fn spawn_signal_watch(tx: mpsc::Sender<&'static str>) {
    thread::spawn(move || {
        let Ok(mut signals) = signal_hook::iterator::Signals::new([signal_hook::consts::SIGHUP])
        else {
            eprintln!("signal watch: could not register SIGHUP");
            return;
        };
        for _ in signals.forever() {
            if tx.send("pin updated (SIGHUP)").is_err() {
                return;
            }
        }
    });
}

/// `bat` is the sysfs battery name (e.g. "BAT0"), so seed reads the same
/// battery the daemon manages.
pub fn upower_charge_history(bat: &str) -> zbus::Result<Vec<(u32, f64, u32)>> {
    let conn = zbus::blocking::Connection::system()?;
    let path = format!("/org/freedesktop/UPower/devices/battery_{bat}");
    let proxy = zbus::blocking::Proxy::new(
        &conn,
        "org.freedesktop.UPower",
        path.as_str(),
        "org.freedesktop.UPower.Device",
    )?;
    proxy.call("GetHistory", &("charge", 0u32, 1_000_000u32))
}
