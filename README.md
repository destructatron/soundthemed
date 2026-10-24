# soundthemed

A freedesktop sound agent that plays [sound theme](https://www.freedesktop.org/wiki/Specifications/sound-theme-spec/) sounds in response to system events. Built for Wayland desktops and compositors that don't ship their own event sounds.

## What it does

- **USB hotplug** — plays `device-added` / `device-removed` when you plug or unplug a USB device
- **Charger events** — plays `power-plug` / `power-unplug` instantly when the AC adapter state changes
- **Battery warnings** — plays `battery-low` and `battery-caution` when battery drops below configurable thresholds, and `battery-charged` / `battery-full` while charging (polled once per minute)
- **Camera and microphone** — plays `camera-started` / `camera-stopped` when any app opens a `/dev/video*` camera (inotify, works for V4L2 and PipeWire apps alike), and `microphone-started` / `microphone-stopped` when a sound card or Bluetooth headset microphone starts capturing (PipeWire)
- **Audio output** — plays `audio-output-changed` when the default PipeWire sink changes
- **Lid** — `lid-open` / `lid-close`
- **Throttling** — `thermal-throttling` / `thermal-cooled` when the processor starts and stops throttling. On AMD APUs with amdgpu `gpu_metrics` format 3.0 (Strix and later) this reads the firmware's throttle residency counters (thermal causes, plus power limits if `throttle_power_limits` is set); elsewhere it falls back to the CPU temperature crossing configurable hot/cool thresholds
- **Power mode** — via power-profiles-daemon: `power-saver-on` / `performance-on` when entering those profiles, `power-saver-off` / `performance-off` when returning to balanced
- **Drives** — plays `drive-mounted` / `drive-unmounted` for mounts under `/run/media`, `/media`, `/mnt` and inside your home folder (rclone, sshfs), whoever made them
- **Windows** — on Niri, plays `window-new`, `window-close`, `window-switch` and `screen-capture`, plus `app-launched` when a program opens its first window, or, for programs that start in the system tray, when a freshly started process registers a tray icon with no window. When its last window closes, its process is watched: `app-closed` plays when it exits; if it keeps running (e.g. in the system tray) `app-hidden` plays instead, then `app-unhidden` if a window comes back and `app-closed` once it really exits
- **Program launches** — plays `app-launching` as soon as a program starts, from systemd `app-*.scope` units (niri `spawn` keybindings count only for commands with a desktop entry). Launchers that start programs directly can call `PlaySound("app-launching")` on the D-Bus service instead
- **Network connectivity** — plays sounds on connect/disconnect via NetworkManager
- **Session events** — plays sounds on suspend resume and screen unlock via systemd-logind
- **Desktop notifications** — plays `message` or `dialog-warning` for incoming notifications (respects `suppress-sound` hint)
- **Volume changes** — optionally plays a sound when the default audio sink volume changes (disabled by default)
- **Compositor bell** — detects terminal/application bell events on Niri (auto-detected, always active)
- **Startup and shutdown sounds** — configurable sounds played when the daemon starts and stops
- **D-Bus service** — exposes `org.freedesktop.SoundThemed1` on the session bus so external apps and scripts can trigger sounds by event ID

## How it works

1. Detects system events from multiple sources (udev, sysfs, D-Bus, compositor IPC)
2. Resolves the event to a sound file from your active freedesktop sound theme
3. Plays it with `pw-play` (PipeWire) — fire-and-forget, so multiple sounds can overlap naturally

Sound theme is read from gsettings (`org.gnome.desktop.sound` → `theme-name`). If gsettings isn't available, falls back to a config file or the `freedesktop` default theme.

## Project structure

soundthemed is a Cargo workspace with three crates:

- **`daemon/`** — the main daemon binary (`soundthemed`)
- **`config-gui/`** — a GTK4/Libadwaita configuration GUI (`soundthemed-config`)
- **`shared/`** — shared library for config, theme resolution, and sound IDs

## Requirements

- Linux with udev
- PipeWire (`pw-play` in PATH; `pw-dump` for microphone, audio output and volume sounds)
- playerctl (optional; volume sounds stay quiet while media is playing)
- A freedesktop-compatible sound theme installed (most distros ship one)
- D-Bus session bus (for network, session, notification, and D-Bus service features)
- NetworkManager (for network connectivity sounds)
- GTK4 and Libadwaita (for the configuration GUI)
- ffmpeg (for theme creation from audio files)

## Building

```sh
cargo build --release
```

This produces two binaries:

- `target/release/soundthemed` — the daemon
- `target/release/soundthemed-config` — the configuration GUI

## Installation

Copy both binaries to somewhere in your PATH:

```sh
cp target/release/soundthemed ~/.local/bin/
cp target/release/soundthemed-config ~/.local/bin/
```

The `soundthemed-config` binary must be in PATH because the daemon launches it via `soundthemed --config`.

## Usage

```sh
# Run the daemon in the foreground
soundthemed

# Launch the configuration GUI
soundthemed --config

# Create a sound theme from a folder of audio files
soundthemed create-theme --name my-theme --from-dir ~/sounds/

# With debug logging
RUST_LOG=debug soundthemed
```

### Systemd user service

```ini
# ~/.config/systemd/user/soundthemed.service
[Unit]
Description=Freedesktop sound theme daemon
After=pipewire.service

[Service]
ExecStart=%h/.local/bin/soundthemed
Restart=on-failure

[Install]
WantedBy=default.target
```

```sh
systemctl --user enable --now soundthemed
```

### Signal handling

- **SIGHUP** — reloads configuration from disk
- **SIGTERM / Ctrl+C** — plays shutdown sound, then exits cleanly

## Configuration

Optionally create `~/.config/soundthemed/config.toml`:

```toml
# Sound theme name (overridden by gsettings if available)
theme = "ocean"

# Enable/disable all sounds
enabled = true

# Battery warning thresholds (percentage)
battery_low_percent = 15
battery_critical_percent = 5
battery_charged_percent = 80    # 0 disables battery-charged

# Count power-limit throttling too (engages on any heavy load)
throttle_power_limits = false

# Processor temperature thresholds (°C), used where throttling
# can't be read directly
thermal_hot_celsius = 95
thermal_cool_celsius = 80

# Startup/shutdown sounds (sound event ID, file path, or "none")
startup_sound = "soundthemed-start"
shutdown_sound = "soundthemed-stop"

# Toggle event sources on/off
[sources]
udev = true
battery = true
network = true
session = true
volume = false          # disabled by default
notifications = true
dbus_service = true
camera = true
microphone = true
audio_output = true
lid = true
thermal = true
power_profile = true
mounts = true
windows = true          # Niri window and screenshot sounds (the bell is always on)
launches = true

# Per-event overrides: "default", "none", a file path, or another
# sound ID from the active theme
[events]
device-added = "default"
device-removed = "none"       # silence this event
battery-low = "/path/to/custom-alert.oga"
```

gsettings takes priority over the config file for the theme name and enabled state, so if you're on GNOME or a gsettings-compatible desktop, it'll pick up your system theme automatically.

## Configuration GUI

The `soundthemed-config` GUI provides a graphical way to manage all settings:

- Theme selection from installed themes
- Startup/shutdown sound selection
- Per-event sound overrides and previews
- Theme creation from folders of audio files

Launch it with `soundthemed --config` or run `soundthemed-config` directly.

## D-Bus service

soundthemed exposes `org.freedesktop.SoundThemed1` on the session bus with the following interface:

**Methods:**
- `PlaySound(event_id: string)` — play a sound by freedesktop event ID (fire-and-forget)
- `PlaySoundSync(event_id: string) → bool` — play a sound and wait for completion
- `HasSound(event_id: string) → bool` — check if a sound exists for the given event
- `ReloadConfig()` — reload configuration from disk

**Properties:**
- `Enabled` (bool) — whether sounds are currently enabled
- `ThemeName` (string) — the active sound theme name

Example usage from the command line:

```sh
busctl --user call org.freedesktop.SoundThemed1 \
  /org/freedesktop/SoundThemed1 \
  org.freedesktop.SoundThemed1 \
  PlaySound s "bell"
```

## Bell shim

For compositors that don't support the `xdg-system-bell-v1` Wayland protocol (e.g. Niri, Sway, Hyprland), a bell shim is provided in `shim/`. This is an `LD_PRELOAD` library that intercepts `gtk_widget_error_bell()` and routes it through soundthemed's D-Bus service.

Build:

```sh
gcc -shared -fPIC -O2 -o shim/bell_shim.so shim/bell_shim.c \
    $(pkg-config --cflags --libs dbus-1)
```

Use:

```sh
LD_PRELOAD=/path/to/bell_shim.so your-gtk-app
```

## Theme creation

soundthemed can create freedesktop-compatible sound themes from a folder of audio files:

```sh
soundthemed create-theme --name my-theme --from-dir ~/sounds/
```

This converts audio files (oga, ogg, mp3, wav, m4a, flac, opus, wma, aac) to Ogg Vorbis and installs them to `~/.local/share/sounds/<name>/stereo/` with a proper `index.theme`. File names should match freedesktop sound event IDs (e.g. `device-added.mp3`).

The same functionality is available through the configuration GUI.

## Sound theme lookup

Sounds are resolved following the freedesktop spec search order:

1. `~/.local/share/sounds/<theme>/stereo/`
2. `/usr/share/sounds/<theme>/stereo/`
3. Fallback to `freedesktop` theme if the configured theme doesn't have the sound
4. Supported formats: `.oga`, `.ogg`, `.wav`

## License

MIT
