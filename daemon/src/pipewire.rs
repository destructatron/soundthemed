//! PipeWire graph monitoring via `pw-dump --monitor`.
//!
//! Watches for:
//!   - A microphone starting or stopping capture
//!   - The default audio output (sink) changing
//!   - The default sink's volume or mute changing (handed to volume.rs)
//!
//! pw-dump prints the whole graph once, then a JSON array of every
//! object that changes (with its complete info), or
//! `{"id": N, "info": null}` when an object goes away. It runs on a
//! dedicated OS thread because serde_json reads it synchronously.

use serde_json::Value;
use soundthemed_shared::sound_ids::SoundEvent;
use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::process::{Command, Stdio};
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

/// How long the state must hold still before it is announced, so a
/// stream renegotiating its format doesn't sound like stop + start.
const SETTLE: Duration = Duration::from_millis(400);
const RESTART_DELAY: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Default)]
struct Snapshot {
    mic_active: bool,
    default_sink: Option<String>,
}

/// A node's volume as PipeWire reports it: per-channel levels and mute.
type Volume = (Vec<f64>, bool);

/// State that lives only on the monitor thread.
#[derive(Default)]
struct Graph {
    running_mics: HashSet<u64>,
    default_sink: Option<String>,
    /// node id -> (node.name, last seen volume)
    volumes: HashMap<u64, (String, Volume)>,
}

/// Start watching the PipeWire graph. `volume`, if given, is poked
/// whenever the default sink's volume or mute changes.
pub fn spawn(
    tx: mpsc::Sender<SoundEvent>,
    microphone: bool,
    audio_output: bool,
    volume: Option<mpsc::Sender<()>>,
) {
    let (snap_tx, snap_rx) = mpsc::channel::<Snapshot>(16);

    std::thread::Builder::new()
        .name("pipewire-monitor".into())
        .spawn(move || {
            let mut last = None;
            loop {
                if let Err(e) = read_graph(&snap_tx, volume.as_ref(), &mut last) {
                    log::warn!("pipewire: pw-dump failed: {e}");
                }
                if snap_tx.is_closed() {
                    break;
                }
                // PipeWire restarted or pw-dump died; reconnect.
                std::thread::sleep(RESTART_DELAY);
            }
        })
        .expect("failed to spawn pipewire monitor thread");

    tokio::spawn(announce(snap_rx, tx, microphone, audio_output));
}

/// Run one pw-dump session, sending a snapshot whenever the
/// state we care about changes.
fn read_graph(
    snap_tx: &mpsc::Sender<Snapshot>,
    volume: Option<&mpsc::Sender<()>>,
    last: &mut Option<Snapshot>,
) -> std::io::Result<()> {
    let mut child = Command::new("pw-dump")
        .args(["--monitor", "--no-colors"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    log::info!("pipewire: watching microphones, default output and volume");

    let stdout = child.stdout.take().expect("piped stdout");
    let batches =
        serde_json::Deserializer::from_reader(BufReader::new(stdout)).into_iter::<Vec<Value>>();

    let mut graph = Graph::default();

    for batch in batches {
        let batch = match batch {
            Ok(b) => b,
            Err(e) => {
                log::warn!("pipewire: bad pw-dump output: {e}");
                break;
            }
        };

        let mut volume_changed = false;
        for obj in &batch {
            volume_changed |= apply(obj, &mut graph);
        }

        if volume_changed {
            if let Some(poke) = volume {
                // A full queue already means "changed"; dropping is fine
                let _ = poke.try_send(());
            }
        }

        let snap = Snapshot {
            mic_active: !graph.running_mics.is_empty(),
            default_sink: graph.default_sink.clone(),
        };

        if last.as_ref() != Some(&snap) {
            *last = Some(snap.clone());
            if snap_tx.blocking_send(snap).is_err() {
                break;
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

/// Fold one changed object into the tracked state. Returns true if
/// the default sink's volume or mute changed.
fn apply(obj: &Value, graph: &mut Graph) -> bool {
    let Some(id) = obj["id"].as_u64() else {
        return false;
    };
    let info = &obj["info"];

    if info.is_null() && obj.get("metadata").is_none() {
        // Object removed
        graph.running_mics.remove(&id);
        graph.volumes.remove(&id);
        return false;
    }

    match obj["type"].as_str() {
        Some("PipeWire:Interface:Node") => {
            let props = &info["props"];
            if is_microphone(props) && info["state"] == "running" {
                graph.running_mics.insert(id);
            } else {
                graph.running_mics.remove(&id);
            }

            let Some(volume) = node_volume(info) else {
                return false;
            };
            let name = props["node.name"].as_str().unwrap_or_default().to_owned();
            // pw-dump repeats unchanged params, so compare values
            let changed = graph
                .volumes
                .get(&id)
                .is_some_and(|(_, previous)| *previous != volume);
            let is_default = graph.default_sink.as_deref() == Some(name.as_str());
            graph.volumes.insert(id, (name, volume));
            changed && is_default
        }
        Some("PipeWire:Interface:Metadata") if obj["props"]["metadata.name"] == "default" => {
            let entries = obj["metadata"].as_array().into_iter().flatten();
            for entry in entries {
                if entry["key"] == "default.audio.sink" {
                    graph.default_sink = entry["value"]["name"].as_str().map(str::to_owned);
                }
            }
            false
        }
        _ => false,
    }
}

/// The node's channel volumes and mute, from its Props param.
fn node_volume(info: &Value) -> Option<Volume> {
    let props = info["params"]["Props"]
        .as_array()?
        .iter()
        .find(|p| p.get("channelVolumes").is_some())?;
    let channels = props["channelVolumes"]
        .as_array()?
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    Some((channels, props["mute"] == true))
}

/// A real capture device: a sound card input, or a Bluetooth headset's
/// own microphone. Bluetooth sources in the phone-facing profiles
/// ("a2dp-source", "headset-audio-gateway") carry the phone's music or
/// call audio, not a microphone, so they don't count.
fn is_microphone(props: &Value) -> bool {
    if props["media.class"] != "Audio/Source" {
        return false;
    }
    match props["device.api"].as_str() {
        Some("alsa") => true,
        Some("bluez5") => props["api.bluez5.profile"]
            .as_str()
            .is_some_and(|p| p.starts_with("headset-head-unit")),
        _ => false,
    }
}

/// Wait until no new snapshot has arrived for SETTLE, returning the
/// last one seen (`None` once the monitor has gone away).
async fn settle(rx: &mut mpsc::Receiver<Snapshot>, mut latest: Snapshot) -> Option<Snapshot> {
    loop {
        match time::timeout(SETTLE, rx.recv()).await {
            Ok(Some(s)) => latest = s,
            Ok(None) => return None,
            Err(_) => return Some(latest),
        }
    }
}

/// Turn settled state changes into sound events. The starting
/// state plays nothing; pw-dump can deliver it over several
/// batches, so it is whatever the first snapshots settle on.
async fn announce(
    mut rx: mpsc::Receiver<Snapshot>,
    tx: mpsc::Sender<SoundEvent>,
    microphone: bool,
    audio_output: bool,
) {
    let Some(first) = rx.recv().await else { return };
    let Some(mut announced) = settle(&mut rx, first).await else {
        return;
    };

    while let Some(changed) = rx.recv().await {
        let Some(latest) = settle(&mut rx, changed).await else {
            return;
        };

        if microphone && latest.mic_active != announced.mic_active {
            let event = if latest.mic_active {
                log::info!("pipewire: microphone in use");
                SoundEvent::MicrophoneStarted
            } else {
                log::info!("pipewire: microphone released");
                SoundEvent::MicrophoneStopped
            };
            if tx.send(event).await.is_err() {
                return;
            }
        }

        if audio_output
            && latest.default_sink.is_some()
            && announced.default_sink.is_some()
            && latest.default_sink != announced.default_sink
        {
            log::info!(
                "pipewire: default output now {}",
                latest.default_sink.as_deref().unwrap_or("?")
            );
            if tx.send(SoundEvent::AudioOutputChanged).await.is_err() {
                return;
            }
        }

        announced = latest;
    }
}
