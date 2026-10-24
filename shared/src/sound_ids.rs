//! Freedesktop sound theme event IDs.
//!
//! Defines all standard event IDs from the freedesktop sound naming spec,
//! plus the SoundEvent enum used by the daemon.

/// A sound event that should trigger playback.
#[derive(Debug, Clone)]
pub enum SoundEvent {
    // Device events
    DeviceAdded,
    DeviceRemoved,

    // Power events
    PowerPlug,
    PowerUnplug,
    BatteryLow,
    BatteryCritical,

    // Network events
    NetworkConnected,
    NetworkDisconnected,

    // Session events
    SessionLogin,
    SessionLogout,
    SuspendResume,

    // Audio events
    AudioVolumeChange,
    AudioOutputChanged,

    // Privacy indicators
    CameraStarted,
    CameraStopped,
    MicrophoneStarted,
    MicrophoneStopped,

    // Hardware state
    LidOpened,
    LidClosed,
    ThermalThrottling,
    ThermalCooled,
    PowerSaverOn,
    PowerSaverOff,
    PerformanceOn,
    PerformanceOff,
    BatteryFull,
    BatteryCharged,
    DriveMounted,
    DriveUnmounted,

    // Compositor
    AppLaunching,
    AppLaunched,
    AppClosed,
    AppHidden,
    AppUnhidden,
    WindowNew,
    WindowClose,
    WindowSwitch,
    ScreenCapture,

    // D-Bus requested (any freedesktop sound ID)
    Custom(String),
}

impl SoundEvent {
    /// Get the freedesktop sound theme ID for this event.
    pub fn sound_id(&self) -> &str {
        match self {
            Self::DeviceAdded => "device-added",
            Self::DeviceRemoved => "device-removed",
            Self::PowerPlug => "power-plug",
            Self::PowerUnplug => "power-unplug",
            Self::BatteryLow => "battery-low",
            Self::BatteryCritical => "battery-caution",
            Self::NetworkConnected => "network-connectivity-established",
            Self::NetworkDisconnected => "network-connectivity-lost",
            Self::SessionLogin => "service-login",
            Self::SessionLogout => "service-logout",
            Self::SuspendResume => "service-login",
            Self::AudioVolumeChange => "audio-volume-change",
            Self::AudioOutputChanged => "audio-output-changed",
            Self::CameraStarted => "camera-started",
            Self::CameraStopped => "camera-stopped",
            Self::MicrophoneStarted => "microphone-started",
            Self::MicrophoneStopped => "microphone-stopped",
            Self::LidOpened => "lid-open",
            Self::LidClosed => "lid-close",
            Self::ThermalThrottling => "thermal-throttling",
            Self::ThermalCooled => "thermal-cooled",
            Self::PowerSaverOn => "power-saver-on",
            Self::PowerSaverOff => "power-saver-off",
            Self::PerformanceOn => "performance-on",
            Self::PerformanceOff => "performance-off",
            Self::BatteryFull => "battery-full",
            Self::BatteryCharged => "battery-charged",
            Self::DriveMounted => "drive-mounted",
            Self::DriveUnmounted => "drive-unmounted",
            Self::AppLaunching => "app-launching",
            Self::AppLaunched => "app-launched",
            Self::AppClosed => "app-closed",
            Self::AppHidden => "app-hidden",
            Self::AppUnhidden => "app-unhidden",
            Self::WindowNew => "window-new",
            Self::WindowClose => "window-close",
            Self::WindowSwitch => "window-switch",
            Self::ScreenCapture => "screen-capture",
            Self::Custom(id) => id.as_str(),
        }
    }
}

/// All standard freedesktop sound event IDs with human-readable descriptions.
pub const ALL_SOUND_IDS: &[(&str, &str)] = &[
    // Alerts
    ("alarm-clock-elapsed", "Alarm clock elapsed"),
    ("battery-caution", "Battery level critical"),
    ("battery-low", "Battery level low"),
    ("dialog-error", "Error dialog"),
    ("dialog-information", "Information dialog"),
    ("dialog-warning", "Warning dialog"),
    ("suspend-error", "Suspend failed"),

    // Notifications
    ("bell", "Terminal or system bell"),
    ("complete", "Task completed"),
    ("message", "Generic message notification"),
    ("message-new-instant", "New instant message"),
    ("phone-incoming-call", "Incoming phone call"),
    ("phone-outgoing-busy", "Outgoing call busy"),
    ("phone-outgoing-calling", "Outgoing call ringing"),
    ("window-attention", "Window requests attention"),

    // Actions
    ("camera-shutter", "Camera shutter"),
    ("screen-capture", "Screenshot taken"),
    ("trash-empty", "Trash emptied"),

    // Devices
    ("device-added", "Device plugged in"),
    ("device-removed", "Device unplugged"),

    // Network
    ("network-connectivity-established", "Network connected"),
    ("network-connectivity-lost", "Network disconnected"),

    // Power
    ("power-plug", "Power cable plugged in"),
    ("power-unplug", "Power cable unplugged"),
    ("battery-full", "Battery fully charged"),
    ("battery-charged", "Battery charged to the set level"),
    ("power-saver-on", "Power saver mode turned on"),
    ("power-saver-off", "Power saver mode turned off"),
    ("performance-on", "Performance mode turned on"),
    ("performance-off", "Performance mode turned off"),

    // Privacy
    ("camera-started", "Camera turned on"),
    ("camera-stopped", "Camera turned off"),
    ("microphone-started", "Microphone in use"),
    ("microphone-stopped", "Microphone no longer in use"),

    // Hardware
    ("lid-open", "Laptop lid opened"),
    ("lid-close", "Laptop lid closed"),
    ("thermal-throttling", "Processor throttling"),
    ("thermal-cooled", "Processor no longer throttling"),
    ("drive-mounted", "Drive mounted"),
    ("drive-unmounted", "Drive unmounted"),

    // Programs and windows
    ("app-launching", "Program starting"),
    ("app-launched", "Program opened its first window"),
    ("app-closed", "Program exited"),
    ("app-hidden", "Program closed its windows but kept running"),
    ("app-unhidden", "Background program opened a window again"),
    ("window-new", "Window opened"),
    ("window-close", "Window closed"),
    ("window-switch", "Window focus changed"),

    // Session
    ("service-login", "Session login or unlock"),
    ("service-logout", "Session logout"),

    // Daemon lifecycle
    ("soundthemed-start", "Sound daemon started"),
    ("soundthemed-stop", "Sound daemon stopped"),

    // Audio
    ("audio-volume-change", "Volume level changed"),
    ("audio-output-changed", "Default audio output changed"),
    ("audio-channel-front-left", "Audio test: front left"),
    ("audio-channel-front-right", "Audio test: front right"),
    ("audio-channel-front-center", "Audio test: front center"),
    ("audio-channel-rear-left", "Audio test: rear left"),
    ("audio-channel-rear-right", "Audio test: rear right"),
    ("audio-channel-rear-center", "Audio test: rear center"),
    ("audio-channel-side-left", "Audio test: side left"),
    ("audio-channel-side-right", "Audio test: side right"),
    ("audio-test-signal", "Audio test signal"),
];

/// Look up the human-readable description for a sound ID.
pub fn description_for(sound_id: &str) -> Option<&'static str> {
    ALL_SOUND_IDS
        .iter()
        .find(|(id, _)| *id == sound_id)
        .map(|(_, desc)| *desc)
}
