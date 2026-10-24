//! Audio volume change sounds.
//!
//! The PipeWire monitor (pipewire.rs) notices the default sink's
//! volume or mute changing and pokes this module, which fires an
//! event for each change, rate-limited by COOLDOWN.
//!
//! Suppresses the volume sound while any MPRIS-compatible media
//! player is in the Playing state (Spotify, VLC, etc.) — checked
//! via `playerctl -a status`.

use soundthemed_shared::sound_ids::SoundEvent;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

const COOLDOWN: Duration = Duration::from_millis(20);

/// Start the volume sound task. Returns the sender the PipeWire
/// monitor pokes on every default sink volume change.
pub fn spawn(tx: mpsc::Sender<SoundEvent>) -> mpsc::Sender<()> {
    let (poke_tx, poke_rx) = mpsc::channel::<()>(8);
    tokio::spawn(watch(poke_rx, tx));
    poke_tx
}

async fn watch(mut poke_rx: mpsc::Receiver<()>, tx: mpsc::Sender<SoundEvent>) {
    log::info!("volume: watching default sink via PipeWire");

    let mut last_sound = Instant::now() - COOLDOWN;

    while poke_rx.recv().await.is_some() {
        let now = Instant::now();
        if now.duration_since(last_sound) >= COOLDOWN && !is_media_playing().await {
            last_sound = now;
            log::debug!("volume: change detected, playing sound");
            if tx.send(SoundEvent::AudioVolumeChange).await.is_err() {
                break;
            }
        }
    }
}

/// Check whether any MPRIS media player is currently Playing.
async fn is_media_playing() -> bool {
    let output = tokio::process::Command::new("playerctl")
        .args(["-a", "status"])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let statuses = String::from_utf8_lossy(&o.stdout);
            statuses.lines().any(|line| line.trim() == "Playing")
        }
        _ => false,
    }
}
