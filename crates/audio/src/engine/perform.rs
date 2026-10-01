use std::{path::PathBuf, time::Duration};

use kernel::{AudioError, AudioEvent, Playback, domain::Speed};
use rodio::Sink;

use crate::{
    deck::{Deck, source::PreloadRequest},
    engine::effect::{EngineEffect, EngineMessage},
    error::seek_error,
};

pub(crate) fn perform(effect: EngineEffect, deck: &mut Deck) -> Option<EngineMessage> {
    match effect {
        EngineEffect::Nothing => None,
        EngineEffect::Batch(steps) => batch(steps, deck),
        EngineEffect::Send(event) => quietly(deck, |deck| deck.send(event)),
        EngineEffect::Mute(error) => mute(deck, error),
        EngineEffect::Open { device, speed } => {
            Some(EngineMessage::Opened(deck.open(device, speed.get())))
        }
        EngineEffect::StartLoad { path, speed } => start_load(deck, path, speed),
        EngineEffect::StartHandover { path, speed } => {
            start_handover(deck, path, speed)
        }
        EngineEffect::Decode(path) => deck.start_decode(path),
        EngineEffect::Start { volume, total } => start(deck, volume, total),
        EngineEffect::Resume {
            volume,
            position,
            playback,
        } => quietly(deck, |deck| {
            resume_primary(deck, position, playback);
            volume_primary(deck, volume);
        }),
        EngineEffect::Play => quietly(deck, |deck| deck.sinks().for_each(Sink::play)),
        EngineEffect::Pause => quietly(deck, |deck| deck.sinks().for_each(Sink::pause)),
        EngineEffect::Seek(target) => quietly(deck, |deck| seek_primary(deck, target)),
        EngineEffect::SetVolume(volume) => {
            quietly(deck, |deck| volume_primary(deck, volume))
        }
        EngineEffect::Arm { cue } => quietly(deck, |deck| deck.cue_primary(cue)),
        EngineEffect::Crossfade { length, incoming } => {
            quietly(deck, |deck| deck.crossfade(length, incoming))
        }
        EngineEffect::CancelCrossfade => quietly(deck, Deck::cancel_crossfade),
        EngineEffect::Ramp { length, playing } => {
            quietly(deck, |deck| deck.ramp_handover(length, playing))
        }
        EngineEffect::DropOutgoing => quietly(deck, Deck::drop_outgoing),
        EngineEffect::SetSpeed(speed) => quietly(deck, |deck| {
            deck.sinks().for_each(|sink| sink.set_speed(speed.get()));
        }),
        EngineEffect::Clear => clear(deck),
        EngineEffect::Preload(request) => deck.start_preload(request),
        EngineEffect::RestartGapless(path) => restart_gapless(deck, path),
        EngineEffect::Promote { volume } => promote(deck, volume),
        EngineEffect::ListDevices => quietly(deck, |deck| deck.list_devices()),
        EngineEffect::Report => report(deck),
        EngineEffect::Advance => quietly(deck, Deck::advance),
    }
}

fn quietly(deck: &mut Deck, act: impl FnOnce(&mut Deck)) -> Option<EngineMessage> {
    act(deck);
    None
}

fn batch(steps: Vec<EngineEffect>, deck: &mut Deck) -> Option<EngineMessage> {
    steps
        .into_iter()
        .fold(None, |landed, effect| landed.or(perform(effect, deck)))
}

fn mute(deck: &mut Deck, error: AudioError) -> Option<EngineMessage> {
    deck.silence();
    deck.send(AudioEvent::Error(error));
    None
}

fn restart_gapless(deck: &mut Deck, path: PathBuf) -> Option<EngineMessage> {
    deck.drop_preload();
    deck.start_preload(PreloadRequest::Gapless(path))
}

fn promote(deck: &mut Deck, volume: f32) -> Option<EngineMessage> {
    deck.promote();
    volume_primary(deck, volume);
    deck.send(AudioEvent::TrackChanged);
    None
}

fn report(deck: &mut Deck) -> Option<EngineMessage> {
    if let Some(position) = deck.playhead() {
        deck.send(AudioEvent::Playhead(position));
    }
    None
}

fn start(
    deck: &mut Deck,
    volume: f32,
    total: Option<Duration>,
) -> Option<EngineMessage> {
    deck.append_staged();
    volume_primary(deck, volume);
    deck.send(AudioEvent::Loaded { total });
    None
}

fn start_load(deck: &mut Deck, path: PathBuf, speed: Speed) -> Option<EngineMessage> {
    deck.drop_preload();
    deck.swap_primary(speed.get());
    deck.start_decode(path)
}

fn start_handover(
    deck: &mut Deck,
    path: PathBuf,
    speed: Speed,
) -> Option<EngineMessage> {
    deck.drop_preload();
    deck.retire_primary(speed.get());
    deck.start_decode(path)
}

fn clear(deck: &mut Deck) -> Option<EngineMessage> {
    deck.drop_preload();
    if let Some(speed) = deck.primary().map(Sink::speed) {
        deck.swap_primary(speed);
    }
    deck.clear_staged();
    None
}

fn volume_primary(deck: &Deck, volume: f32) {
    if let Some(sink) = deck.primary() {
        sink.set_volume(volume);
    }
}

fn seek_primary(deck: &mut Deck, target: Duration) {
    let Some(sink) = deck.primary() else {
        return;
    };
    if let Err(error) = sink.try_seek(target) {
        deck.send(AudioEvent::Error(seek_error(&error)));
    }
}

fn resume_primary(deck: &mut Deck, position: Duration, playback: Playback) {
    deck.append_staged();
    let error = deck
        .primary()
        .and_then(|sink| sink.try_seek(position).err());
    if let Some(error) = error {
        deck.send(AudioEvent::Error(seek_error(&error)));
    }
    if let Playback::Paused = playback
        && let Some(sink) = deck.primary()
    {
        sink.pause();
    }
}
