use std::time::Duration;

use kernel::{cmd::Playback, domain::speed::Speed, update::machine::Driver};
use rodio::Sink;

use crate::{
    AudioDriver,
    deck::Deck,
    engine::{
        effect::EngineEffect,
        message::{AudioMessage, EngineMessage},
    },
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
        EngineEffect::Silence => quietly(deck, Deck::silence),
        EngineEffect::Open { device, speed } => Some(
            deck.open(device, speed)
                .map_or_else(EngineMessage::Error, EngineMessage::Opened)
                .into(),
        ),
        EngineEffect::StartLoad(speed) => quietly(deck, |deck| start_load(deck, speed)),
        EngineEffect::StartHandover(speed) => {
            quietly(deck, |deck| start_handover(deck, speed))
        }
        EngineEffect::Decode => quietly(deck, Deck::clear_staged),
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
        EngineEffect::ClearStaged => quietly(deck, Deck::drop_preload),
        EngineEffect::Promote(gain) => quietly(deck, |deck| promote(deck, gain)),
        EngineEffect::Report => {
            deck.resend_lost();
            Some(EngineMessage::Reported(deck.playhead()).into())
        }
        EngineEffect::Advance(gain) => advance(deck, gain),
        EngineEffect::Stage(track) => quietly(deck, |deck| deck.stage(track)),
        EngineEffect::Attach {
            track_source,
            preload_mode,
        } => deck.attach(track_source, preload_mode),
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
    deck.clear_staged();
}

fn start_handover(deck: &mut Deck, speed: Speed) {
    deck.drop_preload();
    deck.retire_primary(speed);
    deck.clear_staged();
}

fn clear(deck: &mut Deck, speed: Speed) {
    deck.drop_preload();
    if deck.primary().is_some() {
        deck.swap_primary(speed);
    }
    deck.clear_staged();
}

fn advance(deck: &mut Deck, gain: Gain) -> Option<AudioMessage> {
    let signals = deck.advance();
    gain_primary(deck, gain);
    signals
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
        .map(|error| EngineMessage::Error(seek_error(&error)).into())
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
    fn advance_answers_the_signals_a_queued_track_raised_before_it_was_promoted() {
        let mut deck = deck_with_detached_output();
        let (wake, _heard) = crossbeam_channel::bounded(4);
        let (source, control) = envelope(tone(1), Revision::default().next(), wake);
        for _ in source {}
        deck.output.as_mut().unwrap().queued_control = Some(control);

        let answer = execute(EngineEffect::Advance(Gain::UNITY), &mut deck);

        assert!(matches!(
            answer,
            Some(AudioMessage::SignalsTaken { role: SinkRole::Primary, signals })
                if signals.contains(Signals::FINISHED)
        ));
    }
}
