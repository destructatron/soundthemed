//! Removable drive mount monitoring via /proc/self/mountinfo.
//!
//! The kernel flags mountinfo with POLLPRI whenever the mount table
//! changes, whoever mounted what, so background mounts are caught
//! too. Only mounts under the usual removable-media locations or
//! inside the home folder (e.g. rclone/sshfs mounts) count, so
//! system mounts (snapshots, containers) stay quiet.
//!
//! Runs on a dedicated OS thread, like the udev monitor.

use soundthemed_shared::sound_ids::SoundEvent;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::io::AsRawFd;
use tokio::sync::mpsc;

const MEDIA_PREFIXES: &[&str] = &["/run/media/", "/media/", "/mnt/"];

/// Start watching the mount table.
pub fn spawn(tx: mpsc::Sender<SoundEvent>) {
    std::thread::Builder::new()
        .name("mount-monitor".into())
        .spawn(move || {
            if let Err(e) = watch_blocking(tx) {
                log::error!("mounts: {e}");
            }
        })
        .expect("failed to spawn mount monitor thread");
}

/// Where mounts count: removable-media locations and the home folder.
fn watched_prefixes() -> Vec<String> {
    let mut prefixes: Vec<String> = MEDIA_PREFIXES.iter().map(|p| p.to_string()).collect();
    if let Some(home) = dirs::home_dir() {
        prefixes.push(format!("{}/", home.display()));
    }
    prefixes
}

/// Mount points under the watched prefixes.
fn media_mounts(file: &mut File, prefixes: &[String]) -> std::io::Result<HashSet<String>> {
    let mut text = String::new();
    file.seek(SeekFrom::Start(0))?;
    file.read_to_string(&mut text)?;

    // mountinfo field 5 is the mount point
    Ok(text
        .lines()
        .filter_map(|line| line.split(' ').nth(4))
        .filter(|mp| prefixes.iter().any(|p| mp.starts_with(p.as_str())))
        .map(str::to_owned)
        .collect())
}

fn watch_blocking(tx: mpsc::Sender<SoundEvent>) -> std::io::Result<()> {
    let mut file = File::open("/proc/self/mountinfo")?;
    let prefixes = watched_prefixes();
    let mut mounted = media_mounts(&mut file, &prefixes)?;

    log::info!("mounts: watching mounts under {}", prefixes.join(", "));

    loop {
        let mut pollfd = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLPRI,
            revents: 0,
        };

        let ret = unsafe { libc::poll(&mut pollfd, 1, -1) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }

        let now = media_mounts(&mut file, &prefixes)?;

        if now.difference(&mounted).next().is_some() {
            log::info!("mounts: drive mounted");
            if tx.blocking_send(SoundEvent::DriveMounted).is_err() {
                return Ok(());
            }
        }
        if mounted.difference(&now).next().is_some() {
            log::info!("mounts: drive unmounted");
            if tx.blocking_send(SoundEvent::DriveUnmounted).is_err() {
                return Ok(());
            }
        }

        mounted = now;
    }
}
