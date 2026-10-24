//! Power profile monitoring via power-profiles-daemon D-Bus.
//!
//! Watches the ActiveProfile property. Entering "power-saver" or
//! "performance" plays that mode's "on" sound; returning to
//! "balanced" plays the "off" sound of the mode just left.

use futures_util::StreamExt;
use soundthemed_shared::sound_ids::SoundEvent;
use tokio::sync::mpsc;
use zbus::Connection;

#[zbus::proxy(
    interface = "org.freedesktop.UPower.PowerProfiles",
    default_service = "org.freedesktop.UPower.PowerProfiles",
    default_path = "/org/freedesktop/UPower/PowerProfiles"
)]
trait PowerProfiles {
    #[zbus(property)]
    fn active_profile(&self) -> zbus::Result<String>;
}

/// Start watching the active power profile.
pub async fn watch(tx: mpsc::Sender<SoundEvent>) {
    if let Err(e) = watch_inner(tx).await {
        log::info!("power-profile: not available ({e}), skipping");
    }
}

/// The sound for moving from one profile to another, if any.
fn transition(from: &str, to: &str) -> Option<SoundEvent> {
    match (from, to) {
        (_, "power-saver") => Some(SoundEvent::PowerSaverOn),
        (_, "performance") => Some(SoundEvent::PerformanceOn),
        ("power-saver", _) => Some(SoundEvent::PowerSaverOff),
        ("performance", _) => Some(SoundEvent::PerformanceOff),
        _ => None,
    }
}

async fn watch_inner(tx: mpsc::Sender<SoundEvent>) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let proxy = PowerProfilesProxy::new(&connection).await?;

    let mut current = proxy.active_profile().await?;
    let mut changes = proxy.receive_active_profile_changed().await;

    log::info!("power-profile: watching active profile (now {current})");

    while let Some(change) = changes.next().await {
        let Ok(profile) = change.get().await else {
            continue;
        };
        if profile == current {
            continue;
        }
        let previous = std::mem::replace(&mut current, profile);
        log::info!("power-profile: {previous} -> {current}");

        let Some(event) = transition(&previous, &current) else {
            continue;
        };
        if tx.send(event).await.is_err() {
            break;
        }
    }

    Ok(())
}
