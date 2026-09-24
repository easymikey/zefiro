use std::{path::PathBuf, time::Duration};

use kernel::{AudioEvent, AudioFailure, Playback, domain::Speed};

use crate::{
    deck::{Deck, source::PreloadRequest},
    engine::effect::{EngineEffect, EngineMessage},
};

pub(crate) fn perform(io: EngineEffect, deck: &mut Deck) -> Option<EngineMessage> {
    match io {
        EngineEffect::Nothing => None,
        EngineEffect::Many(steps) => in_turn(steps, deck),
        EngineEffect::Send(event) => send(deck, event),
        EngineEffect::Mute(fault) => mute(deck, fault),
        EngineEffect::Open { device, speed } => open_stream(deck, device, speed),
        EngineEffect::StartLoad { path, speed } => start_load(deck, path, speed),
        EngineEffect::StartFade { path, speed } => start_fade(deck, path, speed),
        EngineEffect::Decode(path) => decode(deck, path),
        EngineEffect::Start { volume, total } => start_track(deck, volume, total),
        EngineEffect::Resume {
            volume,
            position,
            paused,
        } => resumed(deck, volume, (position, paused)),
        EngineEffect::Play => play(deck),
        EngineEffect::Pause => pause(deck),
        EngineEffect::Seek(target) => seek_effect(deck, target),
        EngineEffect::SetVolume(volume) => apply_volume(deck, volume),
        EngineEffect::Fade {
            outgoing,
            incoming,
            at,
        } => fade(deck, (outgoing, incoming), at),
        EngineEffect::Retire {
            playing,
            retiring,
            at,
        } => retire(deck, (playing, retiring), at),
        EngineEffect::Retired { playing, at } => retired(deck, playing, at),
        EngineEffect::SetSpeed(speed) => set_speed(deck, speed),
        EngineEffect::Clear => clear(deck),
        EngineEffect::PreloadGapless(path) => preload_gapless(deck, path),
        EngineEffect::PreloadCrossfade { path, gain, speed } => {
            preload_crossfade(deck, (path, gain, speed))
        }
        EngineEffect::RestartGapless(path) => restart_gapless(deck, path),
        EngineEffect::Promote { volume } => promoted(deck, volume),
        EngineEffect::ListDevices => list_devices(deck),
    }
}

fn in_turn(steps: Vec<EngineEffect>, deck: &mut Deck) -> Option<EngineMessage> {
    steps
        .into_iter()
        .fold(None, |landed, io| landed.or(perform(io, deck)))
}

fn send(deck: &Deck, event: AudioEvent) -> Option<EngineMessage> {
    deck.send(event);
    None
}

fn mute(deck: &mut Deck, fault: AudioFailure) -> Option<EngineMessage> {
    deck.silence();
    deck.send(AudioEvent::Error(fault));
    None
}

fn open_stream(
    deck: &mut Deck,
    device: Option<String>,
    speed: Speed,
) -> Option<EngineMessage> {
    Some(EngineMessage::Opened(deck.open(device, speed.value())))
}

fn decode(deck: &mut Deck, path: PathBuf) -> Option<EngineMessage> {
    deck.spawn_decode(path);
    None
}

fn resumed(
    deck: &mut Deck,
    volume: f32,
    resumed_at: (Duration, Playback),
) -> Option<EngineMessage> {
    let (position, paused) = resumed_at;
    resume(deck, position, paused);
    set_volume(deck, volume);
    None
}

fn play(deck: &Deck) -> Option<EngineMessage> {
    deck.sinks().for_each(rodio::Sink::play);
    None
}

fn pause(deck: &Deck) -> Option<EngineMessage> {
    deck.sinks().for_each(rodio::Sink::pause);
    None
}

fn seek_effect(deck: &Deck, target: Duration) -> Option<EngineMessage> {
    seek(deck, target);
    None
}

fn apply_volume(deck: &Deck, volume: f32) -> Option<EngineMessage> {
    set_volume(deck, volume);
    None
}

fn set_speed(deck: &Deck, speed: Speed) -> Option<EngineMessage> {
    deck.sinks().for_each(|sink| sink.set_speed(speed.value()));
    None
}

fn preload_gapless(deck: &mut Deck, path: PathBuf) -> Option<EngineMessage> {
    deck.start_preload(PreloadRequest::Gapless(path));
    None
}

fn preload_crossfade(
    deck: &mut Deck,
    request: (PathBuf, Option<f32>, Speed),
) -> Option<EngineMessage> {
    let (path, gain, speed) = request;
    deck.start_preload(PreloadRequest::Crossfade {
        path,
        gain,
        speed: speed.value(),
    });
    None
}

fn restart_gapless(deck: &mut Deck, path: PathBuf) -> Option<EngineMessage> {
    deck.drop_preload();
    deck.start_preload(PreloadRequest::Gapless(path));
    None
}

fn promoted(deck: &mut Deck, volume: f32) -> Option<EngineMessage> {
    deck.promote();
    set_volume(deck, volume);
    deck.send(AudioEvent::TrackChanged);
    None
}

fn list_devices(deck: &Deck) -> Option<EngineMessage> {
    deck.list_devices();
    None
}

fn start_track(
    deck: &mut Deck,
    volume: f32,
    total: Option<Duration>,
) -> Option<EngineMessage> {
    deck.append_staged();
    set_volume(deck, volume);
    deck.send(AudioEvent::Loaded { total });
    None
}

fn start_load(deck: &mut Deck, path: PathBuf, speed: Speed) -> Option<EngineMessage> {
    deck.drop_preload();
    if let Some(output) = deck.output.as_mut() {
        output.outgoing = None;
        output.swap_sink(speed.value());
    }
    deck.spawn_decode(path);
    None
}

fn start_fade(deck: &mut Deck, path: PathBuf, speed: Speed) -> Option<EngineMessage> {
    deck.drop_preload();
    if let Some(output) = deck.output.as_mut() {
        output.retire_sink(speed.value());
    }
    deck.spawn_decode(path);
    None
}

fn retire(deck: &Deck, volumes: (f32, f32), at: Duration) -> Option<EngineMessage> {
    let (playing, retiring) = volumes;
    set_volume(deck, playing);
    if let Some(outgoing) = deck.outgoing() {
        outgoing.set_volume(retiring);
    }
    deck.send(AudioEvent::Position(at));
    None
}

fn retired(deck: &mut Deck, playing: f32, at: Duration) -> Option<EngineMessage> {
    if let Some(output) = deck.output.as_mut() {
        output.outgoing = None;
    }
    set_volume(deck, playing);
    deck.send(AudioEvent::Position(at));
    None
}

fn fade(deck: &Deck, volumes: (f32, f32), at: Duration) -> Option<EngineMessage> {
    let (outgoing, incoming) = volumes;
    set_volume(deck, outgoing);
    if let Some(preload) = deck
        .output
        .as_ref()
        .and_then(|output| output.preload.as_ref())
    {
        preload.set_volume(incoming);
        if incoming > 0.0 {
            preload.play();
        }
    }
    deck.send(AudioEvent::Position(at));
    None
}

fn clear(deck: &mut Deck) -> Option<EngineMessage> {
    deck.drop_preload();
    if let Some(output) = deck.output.as_mut() {
        let speed = output.sink.speed();
        output.swap_sink(speed);
        output.outgoing = None;
    }
    deck.clear_staged();
    None
}

fn set_volume(deck: &Deck, volume: f32) {
    if let Some(sink) = deck.primary() {
        sink.set_volume(volume);
    }
}

fn seek(deck: &Deck, target: Duration) {
    let Some(sink) = deck.primary() else {
        return;
    };
    if let Err(error) = sink.try_seek(target) {
        deck.send(AudioEvent::Error(AudioFailure::Seek {
            reason: error.to_string(),
        }));
    }
}

fn resume(deck: &mut Deck, position: Duration, paused: Playback) {
    deck.append_staged();
    let Some(sink) = deck.primary() else {
        return;
    };
    if let Err(error) = sink.try_seek(position) {
        deck.send(AudioEvent::Error(AudioFailure::Seek {
            reason: error.to_string(),
        }));
    }
    if let Playback::Paused = paused {
        sink.pause();
    }
}
