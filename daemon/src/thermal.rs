//! Processor throttling monitoring.
//!
//! On AMD APUs whose amdgpu driver exposes `gpu_metrics` format 3.0
//! (Strix and later), reads the SMU firmware's throttle residency
//! counters — milliseconds spent throttled, per cause — so the sound
//! reflects real throttling. Thermal causes (core, graphics, SoC
//! temperature and PROCHOT) always count; power-limit causes count
//! only if `throttle_power_limits` is set, since they engage on any
//! heavy load.
//!
//! Elsewhere, falls back to the CPU temperature sensor (k10temp,
//! zenpower, coretemp): "throttling" at the hot threshold, "cooled"
//! below the (lower) cool threshold.

use soundthemed_shared::sound_ids::SoundEvent;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::time::{self, Duration, Instant};

const DRM_DIR: &str = "/sys/class/drm";
const HWMON_DIR: &str = "/sys/class/hwmon";
const CPU_SENSORS: &[&str] = &["k10temp", "zenpower", "coretemp"];

const COUNTER_POLL: Duration = Duration::from_secs(2);
/// Throttled for at least this long in one poll counts as throttling
const ACTIVE_MS: u64 = 200;
/// No throttling for this long counts as recovered, so a processor
/// hovering at its limit doesn't repeat the sounds
const RECOVER_AFTER: Duration = Duration::from_secs(10);

const TEMP_POLL: Duration = Duration::from_secs(5);

/// gpu_metrics_v3_0 throttle residency counters (u32 each).
const PROCHOT: usize = 228;
const SPL: usize = 232;
const FPPT: usize = 236;
const SPPT: usize = 240;
const THM_CORE: usize = 244;
const THM_GFX: usize = 248;
const THM_SOC: usize = 252;
const V3_0_SIZE: usize = 264;

/// Find a gpu_metrics file in format 3.0.
fn find_gpu_metrics() -> Option<PathBuf> {
    std::fs::read_dir(DRM_DIR)
        .ok()?
        .flatten()
        .find_map(|entry| {
            let path = entry.path().join("device/gpu_metrics");
            let data = std::fs::read(&path).ok()?;
            let size = u16::from_le_bytes([*data.first()?, *data.get(1)?]) as usize;
            let (format, content) = (*data.get(2)?, *data.get(3)?);
            (format == 3 && content == 0 && size >= V3_0_SIZE && data.len() >= V3_0_SIZE)
                .then_some(path)
        })
}

/// Total milliseconds throttled for the causes we count.
fn read_throttle_ms(path: &PathBuf, power_limits: bool) -> Option<u64> {
    let data = std::fs::read(path).ok()?;
    let counter = |offset: usize| -> Option<u64> {
        let bytes = data.get(offset..offset + 4)?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?) as u64)
    };

    let mut causes = vec![PROCHOT, THM_CORE, THM_GFX, THM_SOC];
    if power_limits {
        causes.extend([SPL, FPPT, SPPT]);
    }
    causes.into_iter().map(counter).sum()
}

/// Start watching for processor throttling.
pub async fn watch(tx: mpsc::Sender<SoundEvent>, power_limits: bool, hot: u32, cool: u32) {
    match find_gpu_metrics() {
        Some(path) => watch_counters(tx, path, power_limits).await,
        None => watch_temperature(tx, hot, cool).await,
    }
}

async fn watch_counters(tx: mpsc::Sender<SoundEvent>, path: PathBuf, power_limits: bool) {
    let causes = if power_limits {
        "thermal and power-limit"
    } else {
        "thermal"
    };
    log::info!(
        "thermal: watching {causes} throttling via {}",
        path.display()
    );

    let mut last_total = read_throttle_ms(&path, power_limits);
    let mut throttling = false;
    let mut last_throttled = Instant::now();
    let mut interval = time::interval(COUNTER_POLL);

    loop {
        interval.tick().await;

        let total = read_throttle_ms(&path, power_limits);
        // Counters can reset (resume, driver reload); treat that as no throttling
        let delta = match (last_total, total) {
            (Some(before), Some(now)) => now.saturating_sub(before),
            _ => 0,
        };
        last_total = total;

        let event = if delta >= ACTIVE_MS {
            last_throttled = Instant::now();
            if throttling {
                continue;
            }
            log::warn!("thermal: processor throttling ({delta} ms in {COUNTER_POLL:?})");
            throttling = true;
            SoundEvent::ThermalThrottling
        } else if throttling && last_throttled.elapsed() >= RECOVER_AFTER {
            log::info!("thermal: processor no longer throttling");
            throttling = false;
            SoundEvent::ThermalCooled
        } else {
            continue;
        };

        if tx.send(event).await.is_err() {
            break;
        }
    }
}

/// Find temp1_input (Tctl / package temperature) of the CPU sensor.
fn find_cpu_sensor() -> Option<PathBuf> {
    std::fs::read_dir(HWMON_DIR)
        .ok()?
        .flatten()
        .find_map(|entry| {
            let name = std::fs::read_to_string(entry.path().join("name")).ok()?;
            CPU_SENSORS
                .contains(&name.trim())
                .then(|| entry.path().join("temp1_input"))
        })
}

/// Temperature in whole degrees Celsius.
fn read_celsius(path: &PathBuf) -> Option<u32> {
    let millideg: u32 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    Some(millideg / 1000)
}

async fn watch_temperature(tx: mpsc::Sender<SoundEvent>, hot: u32, cool: u32) {
    let Some(sensor) = find_cpu_sensor() else {
        log::info!("thermal: no throttle counters or CPU temperature sensor, skipping");
        return;
    };

    log::info!(
        "thermal: monitoring {} (hot {hot}°C, cool {cool}°C)",
        sensor.display()
    );

    let mut is_hot = read_celsius(&sensor).is_some_and(|t| t >= hot);
    let mut interval = time::interval(TEMP_POLL);

    loop {
        interval.tick().await;

        let Some(temp) = read_celsius(&sensor) else {
            continue;
        };

        let event = if !is_hot && temp >= hot {
            log::warn!("thermal: processor hot ({temp}°C)");
            is_hot = true;
            SoundEvent::ThermalThrottling
        } else if is_hot && temp < cool {
            log::info!("thermal: processor cooled ({temp}°C)");
            is_hot = false;
            SoundEvent::ThermalCooled
        } else {
            continue;
        };

        if tx.send(event).await.is_err() {
            break;
        }
    }
}
