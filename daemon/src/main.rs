//! soundthemed — Freedesktop sound theme daemon
//!
//! Plays event sounds for USB hotplug, battery state changes,
//! network connectivity, session events, and more. Resolves
//! sounds from the active freedesktop sound theme. Also exposes
//! a D-Bus service for external sound requests.
//!
//! Usage:
//!   soundthemed                  # run daemon
//!   soundthemed --config         # launch config GUI
//!   soundthemed create-theme ... # create theme from folder

mod battery;
mod camera;
mod dbus_service;
mod launches;
mod lid;
mod mounts;
mod network;
mod niri;
mod notifications;
mod pipewire;
mod player;
mod power_profile;
mod session;
mod thermal;
mod udev_monitor;
mod volume;

use clap::{Parser, Subcommand};
use soundthemed_shared::config;
use soundthemed_shared::sound_ids::SoundEvent;
use soundthemed_shared::theme;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(name = "soundthemed", about = "Freedesktop sound theme daemon")]
struct Cli {
    /// Launch the configuration GUI (soundthemed-config)
    #[arg(long)]
    config: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new sound theme from a folder of audio files
    CreateTheme {
        /// Name for the new theme
        #[arg(long)]
        name: String,

        /// Source directory containing audio files
        #[arg(long)]
        from_dir: PathBuf,
    },
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .init();

    let cli = Cli::parse();

    // --config: exec the GUI binary
    if cli.config {
        let err = exec_config_gui();
        log::error!("failed to launch soundthemed-config: {err}");
        std::process::exit(1);
    }

    // create-theme subcommand
    if let Some(Commands::CreateTheme { name, from_dir }) = cli.command {
        match soundthemed_shared::theme_creator::create_theme(&name, &from_dir) {
            Ok(result) => {
                println!("Theme '{}' created at {}", name, result.theme_dir.display());
                println!("  Converted: {} sounds", result.converted.len());
                for id in &result.converted {
                    println!("    {id}");
                }
                if !result.skipped.is_empty() {
                    println!("  Skipped:");
                    for (id, reason) in &result.skipped {
                        println!("    {id}: {reason}");
                    }
                }
                for warning in &result.warnings {
                    println!("  Warning: {warning}");
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // Run daemon
    run_daemon().await;
}

async fn run_daemon() {
    log::info!("soundthemed starting");

    let cfg = config::load();

    if !cfg.enabled {
        log::info!("sounds disabled in config, exiting");
        return;
    }

    let theme_name = Arc::new(std::sync::RwLock::new(cfg.theme.clone()));
    let enabled = Arc::new(std::sync::RwLock::new(cfg.enabled));
    let cfg = Arc::new(std::sync::RwLock::new(cfg));

    let (tx, mut rx) = mpsc::channel::<SoundEvent>(32);

    // Set while a suspend/hibernate is in progress (toggled by the session
    // source on logind PrepareForSleep). Mutes event sounds that would otherwise
    // be started into an audio device being frozen for sleep -> audible HDA DMA
    // loop. The wake sound is allowed through on resume.
    let suspending = Arc::new(AtomicBool::new(false));

    // Spawn event sources based on config
    {
        let sources = &cfg.read().unwrap().sources;

        if sources.udev {
            let udev_tx = tx.clone();
            udev_monitor::spawn(udev_tx);
        }

        if sources.battery {
            let battery_tx = tx.clone();
            let low = cfg.read().unwrap().battery_low_percent;
            let crit = cfg.read().unwrap().battery_critical_percent;
            let charged = cfg.read().unwrap().battery_charged_percent;
            tokio::spawn(async move {
                battery::watch(battery_tx, low, crit, charged).await;
            });
        }

        if sources.network {
            let net_tx = tx.clone();
            tokio::spawn(async move {
                network::watch(net_tx).await;
            });
        }

        if sources.session {
            let session_tx = tx.clone();
            let session_suspending = Arc::clone(&suspending);
            tokio::spawn(async move {
                session::watch(session_tx, session_suspending).await;
            });
        }


        if sources.notifications {
            let notif_tx = tx.clone();
            tokio::spawn(async move {
                notifications::watch(notif_tx).await;
            });
        }

        if sources.camera {
            camera::spawn(tx.clone());
        }

        if sources.microphone || sources.audio_output || sources.volume {
            let volume = sources.volume.then(|| volume::spawn(tx.clone()));
            pipewire::spawn(tx.clone(), sources.microphone, sources.audio_output, volume);
        }

        if sources.lid {
            let lid_tx = tx.clone();
            tokio::spawn(async move {
                lid::watch(lid_tx).await;
            });
        }

        if sources.thermal {
            let thermal_tx = tx.clone();
            let power_limits = cfg.read().unwrap().throttle_power_limits;
            let hot = cfg.read().unwrap().thermal_hot_celsius;
            let cool = cfg.read().unwrap().thermal_cool_celsius;
            tokio::spawn(async move {
                thermal::watch(thermal_tx, power_limits, hot, cool).await;
            });
        }

        if sources.power_profile {
            let profile_tx = tx.clone();
            tokio::spawn(async move {
                power_profile::watch(profile_tx).await;
            });
        }

        if sources.mounts {
            mounts::spawn(tx.clone());
        }

        if sources.launches {
            let launch_tx = tx.clone();
            tokio::spawn(async move {
                launches::watch(launch_tx).await;
            });
        }

        // Niri compositor events (auto-detects); the bell is always on,
        // window and screenshot sounds follow the "windows" source.
        {
            let niri_tx = tx.clone();
            let windows = sources.windows;
            tokio::spawn(async move {
                niri::watch(niri_tx, windows).await;
            });
        }

        if sources.dbus_service {
            let dbus_tx = tx.clone();
            let dbus_theme = Arc::clone(&theme_name);
            let dbus_enabled = Arc::clone(&enabled);
            tokio::spawn(async move {
                dbus_service::serve(dbus_tx, dbus_theme, dbus_enabled).await;
            });
        }
    }

    // Drop the original sender so the channel closes when all watchers are done
    drop(tx);

    log::info!(
        "soundthemed running (theme: {})",
        theme_name.read().unwrap()
    );

    // Play startup sound
    play_special_sound(&cfg.read().unwrap(), "startup").await;

    // Set up SIGHUP handler for config reload
    let mut sighup = signal(SignalKind::hangup()).expect("failed to register SIGHUP handler");

    // Set up SIGTERM handler for clean shutdown
    let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");

    // Main event loop
    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                let id = event.sound_id();
                log::debug!("event: {id}");

                // While a suspend/hibernate is in progress, drop event sounds:
                // starting playback into a device being frozen causes an audible
                // HDA DMA loop. The resume (wake) sound is allowed through.
                if suspending.load(Ordering::SeqCst)
                    && !matches!(event, SoundEvent::SuspendResume)
                {
                    log::debug!("suspend in progress, suppressing sound: {id}");
                    continue;
                }

                let cfg_guard = cfg.read().unwrap();

                // Check per-event overrides
                match config::resolve_override(&cfg_guard, id) {
                    Some(None) => {
                        // Event silenced
                        log::debug!("event silenced by config: {id}");
                    }
                    Some(Some(path)) => {
                        drop(cfg_guard);
                        player::play(&path).await;
                    }
                    None => {
                        let theme = cfg_guard.theme.clone();
                        drop(cfg_guard);
                        if let Some(path) = theme::resolve(&theme, id) {
                            player::play(&path).await;
                        } else {
                            log::warn!("no sound file for: {id}");
                        }
                    }
                }
            }
            _ = sighup.recv() => {
                log::info!("SIGHUP received, reloading config");
                let new_cfg = config::load();
                *theme_name.write().unwrap() = new_cfg.theme.clone();
                *enabled.write().unwrap() = new_cfg.enabled;
                *cfg.write().unwrap() = new_cfg;
                log::info!("config reloaded (theme: {})", theme_name.read().unwrap());
            }
            _ = sigterm.recv() => {
                log::info!("SIGTERM received, playing shutdown sound");
                play_special_sound(&cfg.read().unwrap(), "shutdown").await;
                break;
            }
            _ = tokio::signal::ctrl_c() => {
                log::info!("shutting down, playing shutdown sound");
                play_special_sound(&cfg.read().unwrap(), "shutdown").await;
                break;
            }
        }
    }
}

/// Play a startup or shutdown sound. Resolves the sound ID or file path
/// from config, and waits for playback to complete.
async fn play_special_sound(cfg: &config::Config, which: &str) {
    let sound = match which {
        "startup" => &cfg.startup_sound,
        "shutdown" => &cfg.shutdown_sound,
        _ => return,
    };

    if sound.is_empty() || sound == "none" {
        return;
    }

    // If it looks like a file path, use it directly
    let path = if sound.starts_with('/') {
        std::path::PathBuf::from(sound)
    } else {
        // Treat as a sound event ID and resolve from theme
        match theme::resolve(&cfg.theme, sound) {
            Some(p) => p,
            None => {
                log::warn!("no sound file for {which} sound: {sound}");
                return;
            }
        }
    };

    if which == "shutdown" {
        // Wait for shutdown sound to finish before exiting
        player::play_and_wait(&path).await;
    } else {
        player::play(&path).await;
    }
}

/// Try to exec soundthemed-config. Returns the error if it fails.
fn exec_config_gui() -> std::io::Error {
    use std::os::unix::process::CommandExt;
    // exec replaces the current process
    std::process::Command::new("soundthemed-config").exec()
}
