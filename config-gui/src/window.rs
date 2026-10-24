//! Main configuration window.

use adw::prelude::*;
use soundthemed_shared::config;
use soundthemed_shared::sound_ids::ALL_SOUND_IDS;
use soundthemed_shared::theme;
use std::cell::RefCell;
use std::rc::Rc;

use crate::theme_creator_ui;

pub fn build(app: &adw::Application) {
    let config = Rc::new(RefCell::new(config::load()));
    let themes = theme::list_themes();

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("soundthemed Configuration")
        .default_width(600)
        .default_height(700)
        .build();

    // Explicit AT-SPI wiring so Orca's "speak title" finds the window name.
    window.set_accessible_role(gtk::AccessibleRole::Window);
    window.update_property(&[gtk::accessible::Property::Label("soundthemed Configuration")]);

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("soundthemed Configuration", "")));

    let content = adw::PreferencesPage::new();

    // --- Theme Selection Group ---
    let theme_group = adw::PreferencesGroup::builder()
        .title("Theme")
        .build();

    let theme_row = adw::ComboRow::builder()
        .title("Sound Theme")
        .subtitle("Select the freedesktop sound theme to use")
        .build();

    // Build theme model
    let theme_model = gtk::StringList::new(&[]);
    let mut active_idx: u32 = 0;
    for (i, t) in themes.iter().enumerate() {
        theme_model.append(&t.display_name);
        if t.id == config.borrow().theme {
            active_idx = i as u32;
        }
    }
    theme_row.set_model(Some(&theme_model));
    theme_row.set_selected(active_idx);

    let config_for_theme = Rc::clone(&config);
    let themes_for_cb = themes.clone();
    theme_row.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        if let Some(t) = themes_for_cb.get(idx) {
            config_for_theme.borrow_mut().theme = t.id.clone();
        }
    });

    theme_group.add(&theme_row);
    content.add(&theme_group);

    // --- Startup / Shutdown Sounds ---
    let lifecycle_group = adw::PreferencesGroup::builder()
        .title("Startup and Shutdown")
        .description("Sounds to play when the daemon starts or the session ends")
        .build();

    // Options for startup/shutdown: curated list of sensible choices,
    // plus any theme sounds not already listed
    let lifecycle_options = {
        let mut opts = vec![
            "none".to_string(),
            "soundthemed-start".to_string(),
            "soundthemed-stop".to_string(),
            "service-login".to_string(),
            "service-logout".to_string(),
            "complete".to_string(),
            "bell".to_string(),
            "dialog-information".to_string(),
        ];
        let current_theme = config.borrow().theme.clone();
        for (id, _) in theme::list_theme_sounds(&current_theme) {
            if !opts.contains(&id) {
                opts.push(id);
            }
        }
        opts
    };

    for (field, title, subtitle) in [
        ("startup", "Startup Sound", "Played when the sound daemon starts"),
        ("shutdown", "Shutdown Sound", "Played when the session ends"),
    ] {
        let model = gtk::StringList::new(&[]);
        for opt in &lifecycle_options {
            let label = if opt == "none" { "None" } else { opt.as_str() };
            model.append(label);
        }

        let row = adw::ComboRow::builder()
            .title(title)
            .subtitle(subtitle)
            .model(&model)
            .build();

        let current_val = match field {
            "startup" => config.borrow().startup_sound.clone(),
            "shutdown" => config.borrow().shutdown_sound.clone(),
            _ => "none".into(),
        };

        // Find current value in options
        if let Some(idx) = lifecycle_options.iter().position(|o| *o == current_val) {
            row.set_selected(idx as u32);
        }

        let config_for_lc = Rc::clone(&config);
        let field_owned = field.to_string();
        let opts_for_cb = lifecycle_options.clone();
        row.connect_selected_notify(move |r| {
            let idx = r.selected() as usize;
            if let Some(val) = opts_for_cb.get(idx) {
                let mut cfg = config_for_lc.borrow_mut();
                match field_owned.as_str() {
                    "startup" => cfg.startup_sound = val.clone(),
                    "shutdown" => cfg.shutdown_sound = val.clone(),
                    _ => {}
                }
            }
        });

        // Play/preview button
        let play_btn = gtk::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();

        play_btn.update_property(&[gtk::accessible::Property::Label(&format!(
            "Play {title}"
        ))]);

        let theme_for_play = config.borrow().theme.clone();
        let current_for_play = current_val.clone();
        play_btn.connect_clicked(move |_| {
            if current_for_play != "none" {
                if let Some(path) = theme::resolve(&theme_for_play, &current_for_play) {
                    std::process::Command::new("pw-play")
                        .arg(&path)
                        .spawn()
                        .ok();
                }
            }
        });

        row.add_suffix(&play_btn);
        lifecycle_group.add(&row);
    }

    content.add(&lifecycle_group);

    // --- Event Sources Group ---
    let sources_group = adw::PreferencesGroup::builder()
        .title("Event Sources")
        .description("Enable or disable event detection for each source")
        .build();

    let source_defs: &[(&str, &str, &str)] = &[
        ("udev", "USB Devices", "Play sounds when USB devices are plugged or unplugged"),
        ("battery", "Battery", "Play sounds for low and critical battery levels"),
        ("network", "Network", "Play sounds when network connectivity changes"),
        ("session", "Session", "Play sounds for login, unlock, and resume from suspend"),
        ("volume", "Volume Changes", "Play a sound when system volume changes"),
        ("notifications", "Notifications", "Play sounds for desktop notifications"),
        ("dbus_service", "D-Bus Service", "Allow other apps to request sounds via D-Bus"),
        ("camera", "Camera", "Play sounds when the camera turns on or off"),
        ("microphone", "Microphone", "Play sounds when a microphone starts or stops recording"),
        ("audio_output", "Audio Output", "Play a sound when the default audio output changes"),
        ("lid", "Lid", "Play sounds when the laptop lid opens or closes"),
        ("thermal", "Throttling", "Play sounds when the processor starts and stops throttling"),
        ("power_profile", "Power Mode", "Play sounds when power saver or performance mode turns on or off"),
        ("mounts", "Drives", "Play sounds when drives, or network and cloud folders in your home folder, are mounted or unmounted"),
        ("windows", "Windows", "Play sounds when programs and windows open, close or gain focus, and for screenshots"),
        ("launches", "Program Launches", "Play a sound as soon as a program starts launching"),
    ];

    for (key, title, subtitle) in source_defs {
        let sources = &config.borrow().sources;
        let active = match *key {
            "udev" => sources.udev,
            "battery" => sources.battery,
            "network" => sources.network,
            "session" => sources.session,
            "volume" => sources.volume,
            "notifications" => sources.notifications,
            "dbus_service" => sources.dbus_service,
            "camera" => sources.camera,
            "microphone" => sources.microphone,
            "audio_output" => sources.audio_output,
            "lid" => sources.lid,
            "thermal" => sources.thermal,
            "power_profile" => sources.power_profile,
            "mounts" => sources.mounts,
            "windows" => sources.windows,
            "launches" => sources.launches,
            _ => true,
        };

        let row = adw::SwitchRow::builder()
            .title(*title)
            .subtitle(*subtitle)
            .active(active)
            .build();

        let config_for_source = Rc::clone(&config);
        let key_owned = key.to_string();
        row.connect_active_notify(move |row| {
            let v = row.is_active();
            let mut cfg = config_for_source.borrow_mut();
            match key_owned.as_str() {
                "udev" => cfg.sources.udev = v,
                "battery" => cfg.sources.battery = v,
                "network" => cfg.sources.network = v,
                "session" => cfg.sources.session = v,
                "volume" => cfg.sources.volume = v,
                "notifications" => cfg.sources.notifications = v,
                "dbus_service" => cfg.sources.dbus_service = v,
                "camera" => cfg.sources.camera = v,
                "microphone" => cfg.sources.microphone = v,
                "audio_output" => cfg.sources.audio_output = v,
                "lid" => cfg.sources.lid = v,
                "thermal" => cfg.sources.thermal = v,
                "power_profile" => cfg.sources.power_profile = v,
                "mounts" => cfg.sources.mounts = v,
                "windows" => cfg.sources.windows = v,
                "launches" => cfg.sources.launches = v,
                _ => {}
            }
        });

        sources_group.add(&row);
    }

    content.add(&sources_group);

    // --- Event Sounds Group ---
    let events_group = adw::PreferencesGroup::builder()
        .title("Event Sounds")
        .description("Override which sound plays for each event")
        .build();

    let current_theme = config.borrow().theme.clone();
    let theme_sounds = theme::list_theme_sounds(&current_theme);
    let theme_sound_ids: Vec<String> = theme_sounds.iter().map(|(id, _)| id.clone()).collect();

    for (event_id, description) in ALL_SOUND_IDS {
        let row = adw::ActionRow::builder()
            .title(*event_id)
            .subtitle(*description)
            .build();

        // Build override options: Default, None, + theme sounds.
        // `choices` holds the config value behind each option after
        // the first two; a file path set in config.toml gets its own
        // option, labelled with the file name.
        let options = gtk::StringList::new(&[]);
        options.append("Default");
        options.append("None");
        let mut choices = theme_sound_ids.clone();
        let current = config.borrow().events.get(*event_id).cloned();
        if let Some(path) = current.as_deref().filter(|v| v.contains('/')) {
            choices.push(path.to_string());
        }
        for choice in &choices {
            options.append(&override_label(choice));
        }

        let dropdown = gtk::DropDown::builder()
            .model(&options)
            .valign(gtk::Align::Center)
            .build();

        // Set accessible label
        dropdown.update_property(&[gtk::accessible::Property::Label(&format!(
            "Sound for {event_id}"
        ))]);

        // Set current value from config
        match current.as_deref() {
            Some("none") => dropdown.set_selected(1),
            Some("default") | None => dropdown.set_selected(0),
            Some(value) => {
                if let Some(idx) = choices.iter().position(|s| s == value) {
                    dropdown.set_selected((idx + 2) as u32);
                } else {
                    dropdown.set_selected(0);
                }
            }
        }

        let config_for_event = Rc::clone(&config);
        let event_id_owned = event_id.to_string();
        let theme_sounds_for_cb = choices;
        dropdown.connect_selected_notify(move |dd| {
            let idx = dd.selected() as usize;
            let mut cfg = config_for_event.borrow_mut();
            match idx {
                0 => {
                    cfg.events.remove(&event_id_owned);
                }
                1 => {
                    cfg.events.insert(event_id_owned.clone(), "none".into());
                }
                n => {
                    if let Some(sound) = theme_sounds_for_cb.get(n - 2) {
                        cfg.events.insert(event_id_owned.clone(), sound.clone());
                    }
                }
            }
        });

        // Play/preview button
        let play_btn = gtk::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();

        play_btn.update_property(&[gtk::accessible::Property::Label(&format!(
            "Play {event_id}"
        ))]);

        let config_for_play = Rc::clone(&config);
        let event_id_for_play = event_id.to_string();
        play_btn.connect_clicked(move |_| {
            // Preview what the daemon would play, overrides included
            let cfg = config_for_play.borrow();
            let path = match config::resolve_override(&cfg, &event_id_for_play) {
                Some(path) => path,
                None => theme::resolve(&cfg.theme, &event_id_for_play),
            };
            if let Some(path) = path {
                std::process::Command::new("pw-play")
                    .arg(&path)
                    .spawn()
                    .ok();
            }
        });

        row.add_suffix(&dropdown);
        row.add_suffix(&play_btn);
        events_group.add(&row);
    }

    content.add(&events_group);

    // --- Actions Group ---
    let actions_group = adw::PreferencesGroup::new();

    let create_theme_row = adw::ActionRow::builder()
        .title("Create Theme from Folder")
        .subtitle("Convert audio files to a freedesktop sound theme")
        .activatable(true)
        .build();

    let go_icon = gtk::Image::from_icon_name("go-next-symbolic");
    create_theme_row.add_suffix(&go_icon);

    let window_for_create = window.clone();
    create_theme_row.connect_activated(move |_| {
        theme_creator_ui::show_dialog(&window_for_create);
    });

    actions_group.add(&create_theme_row);
    content.add(&actions_group);

    // --- Bottom bar with Save/Cancel ---
    let bottom_bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk::Align::End)
        .margin_top(12)
        .margin_bottom(12)
        .margin_end(12)
        .build();

    let cancel_btn = gtk::Button::builder()
        .label("Cancel")
        .build();

    let save_btn = gtk::Button::builder()
        .label("Save")
        .css_classes(["suggested-action"])
        .build();

    let window_for_cancel = window.clone();
    cancel_btn.connect_clicked(move |_| {
        window_for_cancel.close();
    });

    let config_for_save = Rc::clone(&config);
    let window_for_save = window.clone();
    save_btn.connect_clicked(move |_| {
        let cfg = config_for_save.borrow();
        match config::save(&cfg) {
            Ok(()) => {
                log::info!("config saved successfully");
                // Try to notify the daemon to reload
                let _ = std::process::Command::new("dbus-send")
                    .args([
                        "--session",
                        "--type=method_call",
                        "--dest=org.freedesktop.SoundThemed1",
                        "/org/freedesktop/SoundThemed1",
                        "org.freedesktop.SoundThemed1.ReloadConfig",
                    ])
                    .spawn();
                window_for_save.close();
            }
            Err(e) => {
                log::error!("failed to save config: {e}");
                let dialog = adw::AlertDialog::builder()
                    .heading("Error Saving Configuration")
                    .body(&format!("Failed to save: {e}"))
                    .build();
                dialog.add_response("ok", "OK");
                dialog.present(Some(&window_for_save));
            }
        }
    });

    bottom_bar.append(&cancel_btn);
    bottom_bar.append(&save_btn);

    // Wrap in a scrolled window and toolbar view
    let scrolled = gtk::ScrolledWindow::builder()
        .child(&content)
        .vexpand(true)
        .build();

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header);
    toolbar_view.set_content(Some(&scrolled));

    let main_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();

    main_box.append(&toolbar_view);
    main_box.append(&bottom_bar);

    window.set_content(Some(&main_box));
    window.present();
}

/// Dropdown label for an override value: theme sound IDs as-is,
/// file paths by file name.
fn override_label(value: &str) -> String {
    if !value.contains('/') {
        return value.to_string();
    }
    std::path::Path::new(value)
        .file_stem()
        .map(|stem| format!("File: {}", stem.to_string_lossy()))
        .unwrap_or_else(|| value.to_string())
}
