use std::time::Duration;

use kernel::{
    cmd::Playback,
    domain::{device::OutputDevice, speed::Speed},
    update::machine::Driver,
};
use rodio::Sink;

use crate::{
    AudioDriver,
    deck::Deck,
    engine::{
        effect::EngineEffect,
        message::{AudioMessage, EngineMessage},
    },
    error::{DeviceError, device_error, seek_error},
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
        EngineEffect::Silence => quietly(deck, Deck::silence),
        EngineEffect::Open { device, speed } => Some(open(deck, device, speed)),
        EngineEffect::StartLoad(speed) => quietly(deck, |deck| start_load(deck, speed)),
        EngineEffect::StartHandover(speed) => {
            quietly(deck, |deck| start_handover(deck, speed))
        }
        EngineEffect::ClearStaged => quietly(deck, Deck::clear_staged),
        EngineEffect::Start(gain) => quietly(deck, |deck| start(deck, gain)),
        EngineEffect::Resume {
            gain,
            position,
            playback,
        } => {
            let failed = resume_current(deck, position, playback);
            set_current_gain(deck, gain);
            failed
        }
        EngineEffect::Play => quietly(deck, |deck| deck.sinks().for_each(Sink::play)),
        EngineEffect::Pause => quietly(deck, |deck| deck.sinks().for_each(Sink::pause)),
        EngineEffect::Seek(target) => seek_current(deck, target),
        EngineEffect::SetGain(gain) => {
            quietly(deck, |deck| set_current_gain(deck, gain))
        }
        EngineEffect::SetFadeStart(fade_start) => {
            quietly(deck, |deck| deck.set_fade_start(fade_start))
        }
        EngineEffect::Crossfade { duration, incoming } => {
            quietly(deck, |deck| deck.crossfade(duration, incoming))
        }
        EngineEffect::CancelCrossfade => quietly(deck, Deck::cancel_crossfade),
        EngineEffect::Ramp { duration, current } => {
            quietly(deck, |deck| deck.ramp_handover(duration, current))
        }
        EngineEffect::DropOutgoing => quietly(deck, Deck::drop_outgoing),
        EngineEffect::SetSpeed(speed) => quietly(deck, |deck| {
            deck.sinks().for_each(|sink| sink.set_speed(speed.get()));
        }),
        EngineEffect::Clear(speed) => quietly(deck, |deck| clear(deck, speed)),
        EngineEffect::DropPreload => quietly(deck, Deck::drop_preload),
        EngineEffect::Promote(gain) => quietly(deck, |deck| promote(deck, gain)),
        EngineEffect::Report => report(deck),
        EngineEffect::Advance(gain) => advance(deck, gain),
        EngineEffect::Stage(decoded_track) => {
            quietly(deck, |deck| deck.stage(decoded_track))
        }
        EngineEffect::Attach {
            decoded_track,
            preload_mode,
        } => deck.attach(decoded_track, preload_mode),
        EngineEffect::TakeSignals(revision) => {
            deck.resend_output_loss();
            deck.take_signals(revision)
        }
    }
}

fn open(deck: &mut Deck, device: OutputDevice, speed: Speed) -> AudioMessage {
    match deck.open(device, speed) {
        Ok(device_opened) => EngineMessage::Opened(device_opened),
        Err(DeviceError::NotFound(_)) => EngineMessage::NotFound,
        Err(error) => EngineMessage::Error(device_error(&error)),
    }
    .into()
}

fn quietly(deck: &mut Deck, act: impl FnOnce(&mut Deck)) -> Option<AudioMessage> {
    act(deck);
    None
}

fn report(deck: &Deck) -> Option<AudioMessage> {
    deck.resend_output_loss();
    Some(EngineMessage::Reported(deck.position()).into())
}

fn promote(deck: &mut Deck, gain: Gain) {
    deck.promote();
    set_current_gain(deck, gain);
}

fn start(deck: &mut Deck, gain: Gain) {
    deck.append_staged();
    set_current_gain(deck, gain);
}

fn start_load(deck: &mut Deck, speed: Speed) {
    deck.drop_preload();
    deck.swap_current(speed);
    deck.clear_staged();
}

fn start_handover(deck: &mut Deck, speed: Speed) {
    deck.drop_preload();
    deck.retire_current(speed);
    deck.clear_staged();
}

fn clear(deck: &mut Deck, speed: Speed) {
    deck.drop_preload();
    if deck.current().is_some() {
        deck.swap_current(speed);
    }
    deck.clear_staged();
}

fn advance(deck: &mut Deck, gain: Gain) -> Option<AudioMessage> {
    let signals = deck.advance();
    set_current_gain(deck, gain);
    signals
}

fn set_current_gain(deck: &Deck, gain: Gain) {
    if let Some(sink) = deck.current() {
        sink.set_volume(gain.amplitude());
    }
}

fn seek_current(deck: &Deck, target: Duration) -> Option<AudioMessage> {
    let sink = deck.current()?;
    sink.try_seek(target)
        .err()
        .map(|error| EngineMessage::Error(seek_error(&error)).into())
}

fn resume_current(
    deck: &mut Deck,
    position: Duration,
    playback: Playback,
) -> Option<AudioMessage> {
    deck.append_staged();
    let failed = seek_current(deck, position);
    if let Playback::Paused = playback
        && let Some(sink) = deck.current()
    {
        sink.pause();
    }
    failed
}

#[cfg(test)]
mod tests {
    use kernel::domain::revision::Revision;

    use crate::{
        deck::{
            envelope::envelope,
            tests::{deck_with_detached_output, tone},
        },
        engine::{
            effect::EngineEffect,
            execute::execute,
            message::{AudioMessage, Signals, SinkRole},
        },
        gain::Gain,
    };

    #[test]
    fn advance_answers_the_signals_an_incoming_track_raised_before_it_was_promoted() {
        let mut deck = deck_with_detached_output();
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(4);
        let (source, control) =
            envelope(tone(1), Revision::default().next(), callback_sender);
        for _ in source {}
        deck.output.as_mut().unwrap().incoming_control = Some(control);

        let answer = execute(EngineEffect::Advance(Gain::UNITY), &mut deck);

        assert!(matches!(
            answer,
            Some(AudioMessage::SignalsTaken { role: SinkRole::Current, signals })
                if signals.contains(Signals::FINISHED)
        ));
    }
}
