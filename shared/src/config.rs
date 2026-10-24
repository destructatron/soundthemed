//! Configuration — gsettings with config file fallback
//!
//! Checks gsettings for the active sound theme first.
//! If gsettings isn't available (no GNOME, no dbus, etc),
//! falls back to ~/.config/soundthemed/config.toml.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

/// Config file structure
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    /// Sound theme name (e.g. "freedesktop", "ocean", "ubuntu")
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Whether sounds are enabled globally
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Battery percentage to trigger low warning
    #[serde(default = "default_battery_low")]
    pub battery_low_percent: u8,

    /// Battery percentage to trigger critical warning
    #[serde(default = "default_battery_critical")]
    pub battery_critical_percent: u8,

    /// Battery percentage that counts as "charged" while charging (0 disables)
    #[serde(default = "default_battery_charged")]
    pub battery_charged_percent: u8,

    /// Also count power-limit throttling (normal under any heavy load),
    /// not just thermal throttling
    #[serde(default)]
    pub throttle_power_limits: bool,

    /// Processor temperature (°C) that counts as running hot, on
    /// machines that don't report throttling
    #[serde(default = "default_thermal_hot")]
    pub thermal_hot_celsius: u32,

    /// Processor temperature (°C) it must drop below to count as cooled
    #[serde(default = "default_thermal_cool")]
    pub thermal_cool_celsius: u32,

    /// Play a sound when the daemon starts (e.g. "service-login", "complete", or a file path)
    /// Empty string or "none" to disable.
    #[serde(default = "default_startup_sound")]
    pub startup_sound: String,

    /// Play a sound when the session is shutting down / logging out
    /// Empty string or "none" to disable.
    #[serde(default = "default_shutdown_sound")]
    pub shutdown_sound: String,

    /// Per-event overrides: event_id -> "default", "none", or a file path
    #[serde(default)]
    pub events: HashMap<String, String>,

    /// Event source toggles
    #[serde(default)]
    pub sources: SourceConfig,
}

/// Toggle individual event sources on/off
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SourceConfig {
    #[serde(default = "bool_true")]
    pub udev: bool,

    #[serde(default = "bool_true")]
    pub battery: bool,

    #[serde(default = "bool_true")]
    pub network: bool,

    #[serde(default = "bool_true")]
    pub session: bool,

    #[serde(default)]
    pub volume: bool,

    #[serde(default = "bool_true")]
    pub notifications: bool,

    #[serde(default = "bool_true")]
    pub dbus_service: bool,

    #[serde(default = "bool_true")]
    pub camera: bool,

    #[serde(default = "bool_true")]
    pub microphone: bool,

    #[serde(default = "bool_true")]
    pub audio_output: bool,

    #[serde(default = "bool_true")]
    pub lid: bool,

    #[serde(default = "bool_true")]
    pub thermal: bool,

    #[serde(default = "bool_true")]
    pub power_profile: bool,

    #[serde(default = "bool_true")]
    pub mounts: bool,

    #[serde(default = "bool_true")]
    pub windows: bool,

    #[serde(default = "bool_true")]
    pub launches: bool,
}

fn default_theme() -> String {
    "freedesktop".into()
}

fn default_enabled() -> bool {
    true
}

fn default_battery_low() -> u8 {
    15
}

fn default_battery_critical() -> u8 {
    5
}

fn default_battery_charged() -> u8 {
    80
}

fn default_thermal_hot() -> u32 {
    95
}

fn default_thermal_cool() -> u32 {
    80
}

fn default_startup_sound() -> String {
    "soundthemed-start".into()
}

fn default_shutdown_sound() -> String {
    "soundthemed-stop".into()
}

fn bool_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            enabled: default_enabled(),
            battery_low_percent: default_battery_low(),
            battery_critical_percent: default_battery_critical(),
            battery_charged_percent: default_battery_charged(),
            throttle_power_limits: false,
            thermal_hot_celsius: default_thermal_hot(),
            thermal_cool_celsius: default_thermal_cool(),
            startup_sound: default_startup_sound(),
            shutdown_sound: default_shutdown_sound(),
            events: HashMap::new(),
            sources: SourceConfig::default(),
        }
    }
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            udev: true,
            battery: true,
            network: true,
            session: true,
            volume: false,
            notifications: true,
            dbus_service: true,
            camera: true,
            microphone: true,
            audio_output: true,
            lid: true,
            thermal: true,
            power_profile: true,
            mounts: true,
            windows: true,
            launches: true,
        }
    }
}

/// Try to get the sound theme name from gsettings.
/// Returns None if gsettings isn't available or fails.
fn theme_from_gsettings() -> Option<String> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.sound", "theme-name"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8(output.stdout).ok()?;
    // gsettings wraps the value in single quotes: 'freedesktop'
    let trimmed = raw.trim().trim_matches('\'');

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Try to get the enabled state from gsettings.
fn enabled_from_gsettings() -> Option<bool> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.sound", "event-sounds"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8(output.stdout).ok()?;
    match raw.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Set the sound theme in gsettings.
fn set_theme_gsettings(theme: &str) -> bool {
    Command::new("gsettings")
        .args(["set", "org.gnome.desktop.sound", "theme-name", theme])
        .status()
        .is_ok_and(|s| s.success())
}

/// Set the event-sounds enabled state in gsettings.
fn set_enabled_gsettings(enabled: bool) -> bool {
    let val = if enabled { "true" } else { "false" };
    Command::new("gsettings")
        .args(["set", "org.gnome.desktop.sound", "event-sounds", val])
        .status()
        .is_ok_and(|s| s.success())
}

/// Path to the config file: ~/.config/soundthemed/config.toml
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("soundthemed")
        .join("config.toml")
}

/// Load config. gsettings is the authority for theme and enabled;
/// the TOML config file provides everything else (per-event overrides,
/// source toggles, battery thresholds) and acts as a fallback for
/// theme/enabled when gsettings is unavailable.
pub fn load() -> Config {
    let mut config = if let Ok(contents) = std::fs::read_to_string(config_path()) {
        match toml::from_str::<Config>(&contents) {
            Ok(c) => {
                log::info!("loaded config from {}", config_path().display());
                c
            }
            Err(e) => {
                log::warn!("failed to parse config: {e}, using defaults");
                Config::default()
            }
        }
    } else {
        log::info!("no config file found, using defaults");
        Config::default()
    };

    // gsettings is primary for theme and enabled
    if let Some(theme) = theme_from_gsettings() {
        log::info!("gsettings sound theme: {theme}");
        config.theme = theme;
    }
    if let Some(enabled) = enabled_from_gsettings() {
        config.enabled = enabled;
    }

    log::info!("active theme: {}", config.theme);
    config
}

/// Save config. Writes theme and enabled to gsettings (primary),
/// and everything to the TOML file (for per-event overrides, sources, etc.).
pub fn save(config: &Config) -> std::io::Result<()> {
    // Write theme and enabled to gsettings
    if set_theme_gsettings(&config.theme) {
        log::info!("gsettings: set theme-name = {}", config.theme);
    } else {
        log::warn!("gsettings: failed to set theme-name");
    }
    if set_enabled_gsettings(config.enabled) {
        log::info!("gsettings: set event-sounds = {}", config.enabled);
    } else {
        log::warn!("gsettings: failed to set event-sounds");
    }

    // Write everything to TOML (fallback + extra settings)
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = toml::to_string_pretty(config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&path, contents)?;
    log::info!("saved config to {}", path.display());
    Ok(())
}

/// Resolve the sound for an event, checking per-event overrides.
/// Returns:
///   Some(Some(path)) - play this specific file
///   Some(None) - event is silenced ("none", or the named theme sound is missing)
///   None - use default theme resolution
pub fn resolve_override(config: &Config, sound_id: &str) -> Option<Option<PathBuf>> {
    let value = config.events.get(sound_id)?;
    match value.as_str() {
        "default" => None,
        "none" => Some(None),
        path if path.contains('/') => Some(Some(PathBuf::from(path))),
        // A bare name is another sound ID from the active theme
        // (this is what the config GUI's dropdown stores).
        sound_id => Some(crate::theme::resolve(&config.theme, sound_id)),
    }
}
