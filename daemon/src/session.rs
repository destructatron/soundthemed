//! Session monitoring via systemd-logind D-Bus.
//!
//! Subscribes to logind signals for:
//!   - PrepareForSleep(true)  → mute event sounds for the duration of sleep
//!   - PrepareForSleep(false) → re-enable sounds and play the wake sound
//!   - Unlock signal on the session

use futures_util::StreamExt;
use soundthemed_shared::sound_ids::SoundEvent;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use zbus::Connection;

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Login1Manager {
    #[zbus(signal)]
    fn prepare_for_sleep(&self, going_to_sleep: bool);

    fn get_session(&self, session_id: &str) -> zbus::Result<zbus::zvariant::OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Login1Session {
    #[zbus(signal)]
    fn unlock(&self);

    #[zbus(signal)]
    fn lock(&self);
}

/// Start watching logind session events.
pub async fn watch(tx: mpsc::Sender<SoundEvent>, suspending: Arc<AtomicBool>) {
    if let Err(e) = watch_inner(tx, suspending).await {
        log::error!("session monitor error: {e}");
    }
}

async fn watch_inner(tx: mpsc::Sender<SoundEvent>, suspending: Arc<AtomicBool>) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let manager = Login1ManagerProxy::new(&connection).await?;

    log::info!("session: watching logind signals");

    // Watch PrepareForSleep for suspend/resume
    let mut sleep_stream = manager.receive_prepare_for_sleep().await?;
    let sleep_tx = tx.clone();

    tokio::spawn(async move {
        while let Some(signal) = sleep_stream.next().await {
            if let Ok(args) = signal.args() {
                if args.going_to_sleep {
                    // Mute event sounds for the whole sleep. Device teardown on
                    // the way down (network down, USB/PCI removal, default-sink
                    // change) would otherwise start a fresh stream into an audio
                    // controller about to be frozen -- the HDA DMA then loops
                    // that buffer audibly until the codec amp is cut.
                    log::info!("session: preparing for sleep, muting event sounds");
                    suspending.store(true, Ordering::SeqCst);
                } else {
                    // Resume: re-enable sounds first, then play the wake sound.
                    suspending.store(false, Ordering::SeqCst);
                    log::info!("session: resumed from suspend");
                    if sleep_tx.send(SoundEvent::SuspendResume).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Try to watch Unlock on the current session
    match manager.get_session("auto").await {
        Ok(session_path) => {
            let session = Login1SessionProxy::builder(&connection)
                .path(session_path)?
                .build()
                .await?;

            let mut unlock_stream = session.receive_unlock().await?;
            let unlock_tx = tx.clone();

            tokio::spawn(async move {
                while let Some(_signal) = unlock_stream.next().await {
                    log::info!("session: unlocked");
                    if unlock_tx.send(SoundEvent::SessionLogin).await.is_err() {
                        break;
                    }
                }
            });
        }
        Err(e) => {
            log::warn!("session: could not get session path: {e}");
        }
    }

    // Keep alive
    std::future::pending::<()>().await;
    Ok(())
}
