use std::{path::PathBuf, time::Duration};

use kernel::{
    AudioEvent,
    AudioFailure,
    Playback,
    domain::{DeviceName, Speed},
};

use crate::{
    deck::{Deck, source::PreloadRequest},
    engine::effect::{EngineEffect, EngineMessage},
    error::seek_fault,
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
        EngineEffect::Arm { cue } => arm(deck, cue),
        EngineEffect::Crossfade { length, incoming } => {
            start_crossfade(deck, length, incoming)
        }
        EngineEffect::Unfade => unfade(deck),
        EngineEffect::Ramp { length, playing } => ramp(deck, length, playing),
        EngineEffect::DropOutgoing => drop_outgoing(deck),
        EngineEffect::SetSpeed(speed) => set_speed(deck, speed),
        EngineEffect::Clear => clear(deck),
        EngineEffect::PreloadGapless(path) => preload_gapless(deck, path),
        EngineEffect::PreloadCrossfade { path, gain, speed } => {
            preload_crossfade(deck, (path, gain, speed))
        }
        EngineEffect::RestartGapless(path) => restart_gapless(deck, path),
        EngineEffect::Promote { volume } => promoted(deck, volume),
        EngineEffect::ListDevices => list_devices(deck),
        EngineEffect::Report => report(deck),
        EngineEffect::Advance => advance(deck),
    }
}

fn in_turn(steps: Vec<EngineEffect>, deck: &mut Deck) -> Option<EngineMessage> {
    steps
        .into_iter()
        .fold(None, |landed, io| landed.or(perform(io, deck)))
}

fn send(deck: &mut Deck, event: AudioEvent) -> Option<EngineMessage> {
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
    device: Option<DeviceName>,
    speed: Speed,
) -> Option<EngineMessage> {
    Some(EngineMessage::Opened(deck.open(device, speed.value())))
}

fn decode(deck: &mut Deck, path: PathBuf) -> Option<EngineMessage> {
    deck.spawn_decode(path)
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

fn seek_effect(deck: &mut Deck, target: Duration) -> Option<EngineMessage> {
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
    deck.start_preload(PreloadRequest::Gapless(path))
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
    })
}

fn restart_gapless(deck: &mut Deck, path: PathBuf) -> Option<EngineMessage> {
    deck.drop_preload();
    deck.start_preload(PreloadRequest::Gapless(path))
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

fn report(deck: &mut Deck) -> Option<EngineMessage> {
    if let Some(position) = deck.playhead() {
        deck.send(AudioEvent::Playhead(position));
    }
    None
}

fn advance(deck: &mut Deck) -> Option<EngineMessage> {
    deck.advance();
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
    deck.swap_primary(speed.value());
    deck.spawn_decode(path)
}

fn start_fade(deck: &mut Deck, path: PathBuf, speed: Speed) -> Option<EngineMessage> {
    deck.drop_preload();
    deck.retire_primary(speed.value());
    let from = deck.retiring_gain();
    deck.spawn_decode(path)
        .or(Some(EngineMessage::Retiring { from }))
}

fn ramp(deck: &mut Deck, length: Duration, playing: f32) -> Option<EngineMessage> {
    deck.ramp_handover(length, playing);
    None
}

fn drop_outgoing(deck: &mut Deck) -> Option<EngineMessage> {
    deck.drop_outgoing();
    None
}

fn arm(deck: &mut Deck, cue: Option<Duration>) -> Option<EngineMessage> {
    deck.cue_primary(cue);
    None
}

fn start_crossfade(
    deck: &mut Deck,
    length: Duration,
    incoming: f32,
) -> Option<EngineMessage> {
    deck.crossfade(length, incoming);
    None
}

fn unfade(deck: &mut Deck) -> Option<EngineMessage> {
    deck.unfade();
    None
}

fn clear(deck: &mut Deck) -> Option<EngineMessage> {
    deck.drop_preload();
    if let Some(speed) = deck.primary().map(rodio::Sink::speed) {
        deck.swap_primary(speed);
    }
    deck.clear_staged();
    None
}

fn set_volume(deck: &Deck, volume: f32) {
    if let Some(sink) = deck.primary() {
        sink.set_volume(volume);
    }
}

fn seek(deck: &mut Deck, target: Duration) {
    let Some(sink) = deck.primary() else {
        return;
    };
    if let Err(error) = sink.try_seek(target) {
        deck.send(AudioEvent::Error(seek_fault(&error)));
    }
}

fn resume(deck: &mut Deck, position: Duration, paused: Playback) {
    deck.append_staged();
    let error = deck
        .primary()
        .and_then(|sink| sink.try_seek(position).err());
    if let Some(error) = error {
        deck.send(AudioEvent::Error(seek_fault(&error)));
    }
    if let Playback::Paused = paused
        && let Some(sink) = deck.primary()
    {
        sink.pause();
    }
}
