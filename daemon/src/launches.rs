//! Program launch monitoring via the systemd user manager.
//!
//! Launchers put each program they start into its own transient
//! unit, named `app-<launcher>-<id>-<suffix>.scope` (or `.service`).
//! Niri does this for every `spawn`, so keybindings for volume keys,
//! playerctl and the like create units too; for niri only commands
//! that some desktop entry runs count as programs. Units from other
//! launchers (GLib/GNOME, flatpak) name a desktop entry already.
//!
//! Launchers that start programs directly (waylauncher) request the
//! sound themselves over D-Bus instead.

use futures_util::StreamExt;
use soundthemed_shared::sound_ids::SoundEvent;
use std::collections::HashSet;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};
use zbus::Connection;

/// How long the list of desktop entry commands is trusted before
/// being rescanned (picks up newly installed programs).
const DESKTOP_CACHE_TTL: Duration = Duration::from_secs(60);

/// Some desktop entries run `sh -c ...`, so shells would otherwise
/// count as programs and every `spawn-sh` keybinding would sound.
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "fish"];

#[zbus::proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait SystemdManager {
    fn subscribe(&self) -> zbus::Result<()>;

    #[zbus(signal)]
    fn unit_new(&self, id: String, unit: zbus::zvariant::OwnedObjectPath) -> zbus::Result<()>;
}

/// Start watching for program launches.
pub async fn watch(tx: mpsc::Sender<SoundEvent>) {
    if let Err(e) = watch_inner(tx).await {
        log::error!("launches: systemd monitor error: {e}");
    }
}

async fn watch_inner(tx: mpsc::Sender<SoundEvent>) -> zbus::Result<()> {
    let connection = Connection::session().await?;
    let manager = SystemdManagerProxy::new(&connection).await?;
    let mut new_units = manager.receive_unit_new().await?;
    // systemd only emits unit signals to subscribed clients
    manager.subscribe().await?;

    log::info!("launches: watching systemd user units");

    let mut desktop = DesktopCommands::default();
    let mut seen: HashSet<String> = HashSet::new();

    while let Some(signal) = new_units.next().await {
        let Ok(args) = signal.args() else { continue };
        let unit = args.id;

        if !seen.insert(unit.clone()) {
            continue;
        }
        if seen.len() > 1000 {
            seen.clear();
        }

        let Some(program) = launched_program(&unit, &mut desktop) else {
            continue;
        };
        log::info!("launches: {program} starting ({unit})");
        if tx.send(SoundEvent::AppLaunching).await.is_err() {
            break;
        }
    }

    Ok(())
}

/// The program a unit was created to launch, if it is a real program.
fn launched_program(unit: &str, desktop: &mut DesktopCommands) -> Option<String> {
    let name = unit.strip_prefix("app-")?;
    let name = name
        .strip_suffix(".scope")
        .or_else(|| name.strip_suffix(".service"))?;
    let (launcher, rest) = name.split_once('-')?;

    if launcher == "niri" {
        // app-niri-<command>-<pid>
        let (command, _pid) = rest.rsplit_once('-')?;
        let command = unescape_unit_name(command);
        let is_program = !SHELLS.contains(&command.as_str()) && desktop.contains(&command);
        return is_program.then_some(command);
    }

    // app-<launcher>-<desktop id>-<random> or app-<launcher>-<desktop id>@<random>
    let id = rest
        .split_once('@')
        .map(|(id, _)| id)
        .or_else(|| rest.rsplit_once('-').map(|(id, _)| id))
        .unwrap_or(rest);
    Some(unescape_unit_name(id))
}

/// Undo systemd unit name escaping (`\x2d` for `-` and so on).
fn unescape_unit_name(escaped: &str) -> String {
    let mut out = String::with_capacity(escaped.len());
    let mut rest = escaped;
    while let Some(pos) = rest.find("\\x") {
        out.push_str(&rest[..pos]);
        let hex = rest.get(pos + 2..pos + 4);
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            Some(byte) => {
                out.push(byte as char);
                rest = &rest[pos + 4..];
            }
            None => {
                out.push_str("\\x");
                rest = &rest[pos + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Command names (and desktop file IDs) of installed desktop entries.
#[derive(Default)]
struct DesktopCommands {
    names: HashSet<String>,
    scanned: Option<Instant>,
}

impl DesktopCommands {
    fn contains(&mut self, command: &str) -> bool {
        let stale = self
            .scanned
            .is_none_or(|t| t.elapsed() >= DESKTOP_CACHE_TTL);
        if stale {
            self.names = scan_desktop_entries();
            self.scanned = Some(Instant::now());
        }
        self.names.contains(command)
    }
}

fn application_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(data_home) = dirs::data_dir() {
        dirs.push(data_home.join("applications"));
    }
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(dir).join("applications"));
    }
    dirs
}

fn scan_desktop_entries() -> HashSet<String> {
    let mut names = HashSet::new();

    for dir in application_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            if let Some(stem) = path.file_stem() {
                names.insert(stem.to_string_lossy().into_owned());
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(command) = text
                .lines()
                .find_map(|l| l.strip_prefix("Exec="))
                .and_then(exec_program)
            {
                names.insert(command);
            }
        }
    }

    log::debug!("launches: {} desktop entry commands", names.len());
    names
}

/// Basename of the program an Exec line runs, skipping `env` and
/// `VAR=value` prefixes.
fn exec_program(exec: &str) -> Option<String> {
    let program = exec
        .split_whitespace()
        .map(|w| w.trim_matches(['"', '\'']))
        .find(|w| *w != "env" && !w.contains('='))?;
    let base = program.rsplit('/').next()?;
    (!base.is_empty()).then(|| base.to_owned())
}
