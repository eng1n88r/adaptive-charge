# adaptive-charge

Smart charging for laptops that expose `charge_control_end_threshold`
(ThinkPads, ASUS, Framework, …) — the Linux counterpart to macOS Optimized
Battery Charging.

The idea: a battery kept plugged in at 100% wears out fast. So while the
laptop is plugged in, charging stops at **80%**. The daemon learns at what
time you usually unplug — say, most weekdays around 08:30 — and finishes
charging to **100%** ninety minutes before that time. You leave with a full
battery; it just never spent the night at full.

## How it decides

- It records every time AC is plugged or unplugged.
- An unplug matters only if the laptop had been on AC for 4+ hours first —
  that's "grabbing the laptop and leaving". Pulling the plug for ten minutes
  to sit on the couch is ignored.
- For each weekday it takes the median of those unplug times. Once a weekday
  has 3 of them, top-ups are scheduled from it; until then a median across
  all days fills in, and with no history at all it simply holds 80% (the
  safe default — you'd just have 80% when you leave, never an aged battery).
- Nothing polls. The daemon sleeps until the next moment its decision could
  change (top-up start, your usual unplug time, a pin expiring, midnight),
  and D-Bus wakes it early when AC changes (UPower) or the machine resumes
  from sleep (logind). ~4–10 wakeups/day total.

## Layout

- `src/main.rs` — CLI dispatch and commands (`run`, `status`, `seed`, `pin`, `unpin`)
- `src/config.rs` — tunables and shared paths
- `src/sysfs.rs` — battery discovery, AC state, threshold writes
- `src/history.rs` — the plug/unplug event log; UPower sample collapsing
- `src/model.rs` — learning the unplug schedule; medians, mtime-keyed cache
- `src/policy.rs` — ceiling decision and next-wake scheduling
- `src/watch.rs` — D-Bus wake sources and history fetch
- `src/testutil.rs` — shared test fixtures (`cfg(test)` only)
- `contrib/adaptive-charge.service` — hardened systemd unit
- `contrib/check-new-battery.sh` — verify a replacement battery charges
- `contrib/thinkpad-charge-thresholds.service` — superseded static 75/85 unit

## Install

**As a pacman package (recommended — no Rust toolchain needed):**

```sh
git clone https://github.com/eng1n88r/adaptive-charge
cd adaptive-charge/packaging/bin
makepkg -si
sudo systemctl enable --now adaptive-charge.service
sudo adaptive-charge seed --write   # optional: learn from UPower's history
```

The package pulls the prebuilt release binary, and pacman tracks every file
(`pacman -R adaptive-charge-bin` uninstalls cleanly). An AUR listing will
follow once AUR account registration reopens.

**From source (for development):**

```sh
make build          # as your user; needs cargo
sudo make install   # binary + unit + seed + enable, prints status when done
```

`sudo make uninstall` reverses it. `install` also retires the old static
thinkpad-charge-thresholds unit if present, and seeds the model from UPower's
charge history so it doesn't start blind (`adaptive-charge seed` previews
that; `--write` merges, deduplicated).

`adaptive-charge ceiling 90` changes the hold level (50–100, default 80).
`adaptive-charge pin 100 4` forces 100% for four hours (leaving earlier than
usual); `adaptive-charge unpin` cancels — both take effect immediately in the
running daemon. State lives in `/var/lib/adaptive-charge/`
(`ADAPTIVE_CHARGE_STATE` overrides, used by the tests).

## Known limitation

The per-weekday median is computed on minutes-since-midnight, so an unplug
habit that *straddles* midnight (23:50 some days, 00:10 others) averages to
a mid-day nonsense value. A habit consistently just *after* midnight is fine
— the top-up window correctly reaches back into the previous evening.
Fixable with circular statistics if it ever matters.

## Omarchy widget

A power-panel widget for Omarchy (on/off switch, ceiling presets, live
status) lives at
[omarchy-adaptive-power](https://github.com/eng1n88r/omarchy-adaptive-power).
