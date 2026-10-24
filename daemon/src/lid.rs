//! Laptop lid monitoring via /proc/acpi/button/lid/*/state.
//!
//! Polls the ACPI lid state once a second. Closing the lid usually
//! suspends before a poll notices, so in practice lid-close mostly
//! fires when docked, and lid-open after a suspend is already
//! covered by the resume sound.

use soundthemed_shared::sound_ids::SoundEvent;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const LID_DIR: &str = "/proc/acpi/button/lid";

fn find_lid() -> Option<PathBuf> {
    std::fs::read_dir(LID_DIR)
        .ok()?
        .flatten()
        .map(|e| e.path().join("state"))
        .find(|p| p.is_file())
}

fn is_closed(state_path: &PathBuf) -> Option<bool> {
    let state = std::fs::read_to_string(state_path).ok()?;
    Some(state.contains("closed"))
}

/// Start polling the lid switch.
pub async fn watch(tx: mpsc::Sender<SoundEvent>) {
    let Some(state_path) = find_lid() else {
        log::info!("lid: no lid switch found, skipping");
        return;
    };

    log::info!("lid: monitoring {}", state_path.display());

    let mut closed = is_closed(&state_path).unwrap_or(false);
    let mut interval = time::interval(POLL_INTERVAL);

    loop {
        interval.tick().await;

        let Some(now) = is_closed(&state_path) else {
            continue;
        };
        if now == closed {
            continue;
        }
        closed = now;

        let event = if closed {
            log::info!("lid: closed");
            SoundEvent::LidClosed
        } else {
            log::info!("lid: opened");
            SoundEvent::LidOpened
        };
        if tx.send(event).await.is_err() {
            break;
        }
    }
}
