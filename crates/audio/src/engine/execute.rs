use std::{path::PathBuf, time::Duration};

use kernel::{Playback, domain::Speed, update::Driver};
use rodio::Sink;

use crate::{
    AudioDriver,
    deck::{Deck, source::PreloadMode},
    engine::effect::{AudioMessage, EngineEffect},
    error::seek_error,
    gain::Gain,
};

impl Driver for AudioDriver {
    type Effect = EngineEffect;

    fn execute(&mut self, effect: EngineEffect) -> Option<AudioMessage> {
        execute(effect, &mut self.deck)
    }
}

fn execute(effect: EngineEffect, deck: &mut Deck) -> Option<AudioMessage> {
    match effect {
        EngineEffect::Mute => quietly(deck, Deck::silence),
        EngineEffect::Open { device, speed } => Some(
            deck.open(device, speed)
                .map_or_else(AudioMessage::Error, AudioMessage::Opened),
        ),
        EngineEffect::StartLoad { speed, .. } => {
            quietly(deck, |deck| start_load(deck, speed))
        }
        EngineEffect::StartHandover { speed, .. } => {
            quietly(deck, |deck| start_handover(deck, speed))
        }
        EngineEffect::Decode(_) => quietly(deck, Deck::start_decode),
        EngineEffect::Start(gain) => quietly(deck, |deck| start(deck, gain)),
        EngineEffect::Resume {
            gain,
            position,
            playback,
        } => {
            let failed = resume_primary(deck, position, playback);
            gain_primary(deck, gain);
            failed
        }
        EngineEffect::Play => quietly(deck, |deck| deck.sinks().for_each(Sink::play)),
        EngineEffect::Pause => quietly(deck, |deck| deck.sinks().for_each(Sink::pause)),
        EngineEffect::Seek(target) => seek_primary(deck, target),
        EngineEffect::SetGain(gain) => quietly(deck, |deck| gain_primary(deck, gain)),
        EngineEffect::Arm(cue) => quietly(deck, |deck| deck.cue_primary(cue)),
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
        EngineEffect::Clear(speed) => quietly(deck, |deck| clear(deck, speed)),
        EngineEffect::Preload { path, mode } => {
            quietly(deck, |deck| deck.start_preload(path, mode))
        }
        EngineEffect::RestartGapless(path) => {
            quietly(deck, |deck| restart_gapless(deck, path))
        }
        EngineEffect::Promote(gain) => quietly(deck, |deck| promote(deck, gain)),
        EngineEffect::Run(_) => None,
        EngineEffect::Report => {
            deck.resend_lost();
            Some(AudioMessage::Reported(deck.playhead()))
        }
        EngineEffect::Advance => quietly(deck, Deck::advance),
        EngineEffect::Stage(track) => quietly(deck, |deck| deck.stage(track)),
        EngineEffect::Attach(track) => deck.attach(track),
        EngineEffect::TakeSignals(revision) => {
            deck.resend_lost();
            deck.take_signals(revision)
        }
    }
}

fn quietly(deck: &mut Deck, act: impl FnOnce(&mut Deck)) -> Option<AudioMessage> {
    act(deck);
    None
}

fn restart_gapless(deck: &mut Deck, path: PathBuf) {
    deck.drop_preload();
    deck.start_preload(path, PreloadMode::Gapless);
}

fn promote(deck: &mut Deck, gain: Gain) {
    deck.promote();
    gain_primary(deck, gain);
}

fn start(deck: &mut Deck, gain: Gain) {
    deck.append_staged();
    gain_primary(deck, gain);
}

fn start_load(deck: &mut Deck, speed: Speed) {
    deck.drop_preload();
    deck.swap_primary(speed);
    deck.start_decode();
}

fn start_handover(deck: &mut Deck, speed: Speed) {
    deck.drop_preload();
    deck.retire_primary(speed);
    deck.start_decode();
}

fn clear(deck: &mut Deck, speed: Speed) {
    deck.drop_preload();
    if deck.primary().is_some() {
        deck.swap_primary(speed);
    }
    deck.clear_staged();
}

fn gain_primary(deck: &Deck, gain: Gain) {
    if let Some(sink) = deck.primary() {
        sink.set_volume(gain.amplitude());
    }
}

fn seek_primary(deck: &Deck, target: Duration) -> Option<AudioMessage> {
    let sink = deck.primary()?;
    sink.try_seek(target)
        .err()
        .map(|error| AudioMessage::Error(seek_error(&error)))
}

fn resume_primary(
    deck: &mut Deck,
    position: Duration,
    playback: Playback,
) -> Option<AudioMessage> {
    deck.append_staged();
    let failed = seek_primary(deck, position);
    if let Playback::Paused = playback
        && let Some(sink) = deck.primary()
    {
        sink.pause();
    }
    failed
}
