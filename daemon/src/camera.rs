//! Camera monitoring via inotify on /dev/video*.
//!
//! The kernel reports every open and close of a video device to
//! inotify, whichever way the app reaches the camera (V4L2 directly,
//! or through PipeWire, which then holds the device itself). Each
//! open/close only prompts a rescan of /proc/*/fd for processes
//! holding a camera, so a missed event can't leave the state stuck.
//!
//! The inotify loop runs on a dedicated OS thread, like the udev
//! monitor.

use soundthemed_shared::sound_ids::SoundEvent;
use std::ffi::CString;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

/// Apps and WirePlumber briefly open cameras to list them; only
/// announce once the device has stayed open (or closed) this long.
const SETTLE: Duration = Duration::from_millis(500);

/// Start watching camera devices.
pub fn spawn(tx: mpsc::Sender<SoundEvent>) {
    let (poke_tx, poke_rx) = mpsc::channel::<()>(8);

    std::thread::Builder::new()
        .name("camera-monitor".into())
        .spawn(move || watch_blocking(poke_tx))
        .expect("failed to spawn camera monitor thread");

    tokio::spawn(announce(poke_rx, tx));
}

fn add_watch(fd: i32, path: &str, mask: u32) -> i32 {
    let Ok(c_path) = CString::new(path) else {
        return -1;
    };
    unsafe { libc::inotify_add_watch(fd, c_path.as_ptr(), mask) }
}

fn watch_device(fd: i32, path: &str) {
    let mask = libc::IN_OPEN | libc::IN_CLOSE_WRITE | libc::IN_CLOSE_NOWRITE;
    if add_watch(fd, path, mask) < 0 {
        log::warn!(
            "camera: cannot watch {path}: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Blocking inotify loop. Pokes the announcer on every camera
/// open/close, and starts watching cameras plugged in later.
fn watch_blocking(poke: mpsc::Sender<()>) {
    let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
    if fd < 0 {
        log::error!(
            "camera: inotify_init failed: {}",
            std::io::Error::last_os_error()
        );
        return;
    }

    let dev_wd = add_watch(fd, "/dev", libc::IN_CREATE);

    if let Ok(entries) = std::fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("video") {
                watch_device(fd, &format!("/dev/{name}"));
            }
        }
    }

    log::info!("camera: watching /dev/video*");

    // u64 backing keeps the buffer aligned for inotify_event
    let mut buf = [0u64; 512];
    let header = std::mem::size_of::<libc::inotify_event>();

    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), std::mem::size_of_val(&buf)) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            log::error!("camera: inotify read error: {err}");
            return;
        }

        let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), n as usize) };
        let mut offset = 0;
        let mut camera_touched = false;

        while offset + header <= bytes.len() {
            let event: libc::inotify_event =
                unsafe { std::ptr::read_unaligned(bytes[offset..].as_ptr().cast()) };
            let name_bytes = &bytes[offset + header..offset + header + event.len as usize];
            offset += header + event.len as usize;

            if event.wd == dev_wd {
                let name = String::from_utf8_lossy(name_bytes);
                let name = name.trim_end_matches('\0');
                if name.starts_with("video") {
                    log::info!("camera: new device /dev/{name}");
                    watch_device(fd, &format!("/dev/{name}"));
                }
            } else if event.mask & libc::IN_IGNORED == 0 {
                camera_touched = true;
            }
        }

        if camera_touched && poke.blocking_send(()).is_err() {
            return;
        }
    }
}

/// Whether any process has a /dev/video* device open.
fn camera_in_use() -> bool {
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return false;
    };

    for proc_entry in procs.flatten() {
        let is_pid = proc_entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()));
        if !is_pid {
            continue;
        }

        // Other users' fds are unreadable; skip them quietly
        let Ok(fds) = std::fs::read_dir(proc_entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path()) {
                if target.to_string_lossy().starts_with("/dev/video") {
                    return true;
                }
            }
        }
    }

    false
}

async fn scan() -> bool {
    tokio::task::spawn_blocking(camera_in_use)
        .await
        .unwrap_or(false)
}

/// Rescan after each settled burst of camera opens/closes and
/// announce on/off transitions.
async fn announce(mut poke_rx: mpsc::Receiver<()>, tx: mpsc::Sender<SoundEvent>) {
    let mut active = scan().await;

    while poke_rx.recv().await.is_some() {
        loop {
            match time::timeout(SETTLE, poke_rx.recv()).await {
                Ok(Some(())) => continue,
                Ok(None) => return,
                Err(_) => break,
            }
        }

        let now = scan().await;
        if now == active {
            continue;
        }
        active = now;

        let event = if active {
            log::info!("camera: in use");
            SoundEvent::CameraStarted
        } else {
            log::info!("camera: released");
            SoundEvent::CameraStopped
        };
        if tx.send(event).await.is_err() {
            return;
        }
    }
}
