//! Battery level monitoring via /sys/class/power_supply.
//!
//! Polls battery percentage periodically and fires events
//! for low and critical levels, and for reaching the charged
//! level or full while charging. Charger plug/unplug is handled
//! instantly by the udev monitor instead.

use soundthemed_shared::sound_ids::SoundEvent;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

const POLL_INTERVAL: Duration = Duration::from_secs(60);
const POWER_SUPPLY_DIR: &str = "/sys/class/power_supply";

/// Find the first battery in /sys/class/power_supply.
fn find_battery() -> Option<PathBuf> {
    let entries = std::fs::read_dir(POWER_SUPPLY_DIR).ok()?;

    for entry in entries.flatten() {
        let type_path = entry.path().join("type");
        if let Ok(psu_type) = std::fs::read_to_string(&type_path) {
            if psu_type.trim() == "Battery" {
                return Some(entry.path());
            }
        }
    }

    None
}

/// Read a sysfs file and return its trimmed contents.
fn read_sysfs(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Parse the battery charge percentage.
fn read_capacity(battery_path: &Path) -> Option<u8> {
    read_sysfs(&battery_path.join("capacity"))?.parse().ok()
}

/// Check if the battery is currently discharging.
fn is_discharging(battery_path: &Path) -> bool {
    matches!(
        read_sysfs(&battery_path.join("status")).as_deref(),
        Some("Discharging") | Some("Not charging")
    )
}

/// Read the battery status ("Charging", "Discharging", "Full", ...).
fn read_status(battery_path: &Path) -> Option<String> {
    read_sysfs(&battery_path.join("status"))
}

/// Start polling battery level.
///
/// Fires events when battery drops below low or critical
/// thresholds while discharging. Resets when battery
/// recovers above the threshold. While charging, also fires
/// once when the battery reaches `charged_pct` (0 disables)
/// and once when it reports full.
pub async fn watch(tx: mpsc::Sender<SoundEvent>, low_pct: u8, crit_pct: u8, charged_pct: u8) {
    let battery_path = match find_battery() {
        Some(p) => {
            log::info!("battery: monitoring {}", p.display());
            p
        }
        None => {
            log::info!("battery: no battery found, skipping monitor");
            return;
        }
    };

    let mut fired_low = false;
    let mut fired_critical = false;
    let mut last_status = read_status(&battery_path);
    let mut last_pct = read_capacity(&battery_path);
    let mut interval = time::interval(POLL_INTERVAL);

    loop {
        interval.tick().await;

        let status = read_status(&battery_path);
        let pct = read_capacity(&battery_path);
        let prev_status = std::mem::replace(&mut last_status, status.clone());
        let prev_pct = std::mem::replace(&mut last_pct, pct);

        if status.as_deref() == Some("Full") && prev_status.as_deref() == Some("Charging") {
            log::info!("battery: full");
            if tx.send(SoundEvent::BatteryFull).await.is_err() {
                break;
            }
        }

        if !is_discharging(&battery_path) {
            // Reset when charging
            fired_low = false;
            fired_critical = false;

            let reached_charged = charged_pct > 0
                && charged_pct < 100
                && status.as_deref() == Some("Charging")
                && matches!((prev_pct, pct), (Some(before), Some(now)) if before < charged_pct && now >= charged_pct);
            if reached_charged {
                log::info!("battery: charged to {charged_pct}%");
                if tx.send(SoundEvent::BatteryCharged).await.is_err() {
                    break;
                }
            }
            continue;
        }

        let pct = match pct {
            Some(p) => p,
            None => continue,
        };

        if pct <= crit_pct && !fired_critical {
            log::warn!("battery: critical ({pct}%)");
            fired_critical = true;
            if tx.send(SoundEvent::BatteryCritical).await.is_err() {
                break;
            }
        } else if pct <= low_pct && !fired_low {
            log::warn!("battery: low ({pct}%)");
            fired_low = true;
            if tx.send(SoundEvent::BatteryLow).await.is_err() {
                break;
            }
        }

        // Reset if recovered
        if pct > low_pct {
            fired_low = false;
            fired_critical = false;
        }
    }
}
