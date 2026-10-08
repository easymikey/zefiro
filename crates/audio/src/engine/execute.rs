use std::time::Duration;

use kernel::{
    cmd::Playback,
    domain::{device::OutputDevice, speed::Speed},
    update::machine::Driver,
};

use crate::{
    AudioDriver,
    deck::{Deck, mixer::MixerOrder},
    engine::{
        effect::EngineEffect,
        message::{AudioMessage, EngineMessage, SinkRole},
    },
    error::{DeviceError, device_error},
    gain::Gain,
};

impl Driver for AudioDriver {
    type Effect = EngineEffect;

    fn execute(&mut self, effect: EngineEffect) -> Option<AudioMessage> {
        if let Some(output) = self.deck.output.as_mut() {
            output.retired_voices.by_ref().for_each(drop);
        }
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
        EngineEffect::Start(gain) => start(deck, gain),
        EngineEffect::Resume {
            gain,
            position,
            playback,
        } => {
            let refusal = resume_current(deck, position, playback);
            deck.set_current_gain(gain);
            refusal
        }
        EngineEffect::Play => quietly(deck, |deck| deck.transport(Playback::Playing)),
        EngineEffect::Pause => quietly(deck, |deck| deck.transport(Playback::Paused)),
        EngineEffect::Seek(target) => quietly(deck, |deck| deck.seek(target)),
        EngineEffect::SetGain(gain) => {
            quietly(deck, |deck| deck.set_current_gain(gain))
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
        EngineEffect::SetSpeed(speed) => quietly(deck, |deck| deck.set_speed(speed)),
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
    deck.set_current_gain(gain);
}

fn start(deck: &mut Deck, gain: Gain) -> Option<AudioMessage> {
    let refusal = deck.append_staged();
    deck.set_current_gain(gain);
    refusal
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

fn advance(deck: &mut Deck, gain: Gain) -> Option<AudioMessage> {
    let signals = deck.advance();
    deck.set_current_gain(gain);
    signals
}

fn resume_current(
    deck: &mut Deck,
    position: Duration,
    playback: Playback,
) -> Option<AudioMessage> {
    let refusal = deck.append_staged();
    if let (Playback::Paused, Some(output)) = (playback, deck.output.as_mut()) {
        output.current_playback = Playback::Paused;
        output.mixer_control.order(MixerOrder::RolePlayback {
            role: SinkRole::Current,
            playback: Playback::Paused,
        });
    }
    deck.seek(position);
    refusal
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        thread,
        time::{Duration, Instant},
    };

    use kernel::{
        cmd::Playback,
        domain::{revision::Revision, settings::AudioSettings},
        update::machine::Driver,
    };
    use rstest::rstest;

    use crate::{
        AudioDriver,
        FeedChannel,
        deck::{
            Deck,
            envelope::EnvelopeControl,
            feed::serve::serve,
            mixer::{Mixer, RETIRED_SLOTS},
            output::{Fader, Output, tests::mixed_output},
            source::{
                DecodedTrack,
                decode,
                tests::{decoded, ramp_file},
            },
            tests::{deck_with_detached_output, played, pulled, track},
            voice::DECLICK_FRAMES,
        },
        engine::{
            effect::EngineEffect,
            execute::execute,
            message::{AudioMessage, EngineMessage, Signals, SinkRole},
            tests::assert_same,
        },
        gain::Gain,
        tap,
    };

    const RAMP_RATE: u32 = 8_000;
    const RAMP_STEP: f32 = 1.0 / 32_768.0;

    struct Listening {
        deck: Deck,
        mixer: Mixer,
        heard: Vec<f32>,
    }

    fn listen(mixer: &mut Mixer, heard: &mut Vec<f32>, frames: usize) {
        let mut out = vec![0.0_f32; frames];
        mixer.mix(&mut out);
        heard.extend(out.into_iter().filter(|sample| *sample != 0.0));
    }

    impl Listening {
        fn playing_ramp(frames: usize) -> Self {
            let file = ramp_file(1, frames);
            let (feed_sender, feed_receiver) = crossbeam_channel::bounded(4);
            thread::spawn(move || serve(&feed_receiver));
            let (spectrum_buffers, _spectrum_tap) = tap::spectrum_channel();
            let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(64);
            let (output, mixer) = mixed_output(RAMP_RATE, &spectrum_buffers);
            let mut deck = Deck::new(spectrum_buffers, callback_sender, feed_sender);
            deck.output = Some(output);
            deck.stage(DecodedTrack {
                revision: Revision::default().next(),
                decoder: decode(file.path()).unwrap(),
            });
            let mut listening = Self {
                deck,
                mixer,
                heard: Vec::new(),
            };
            assert!(
                listening
                    .execute(EngineEffect::Start(Gain::UNITY))
                    .is_none()
            );
            assert!(listening.execute(EngineEffect::Play).is_none());
            listening.pull(4_000);
            listening
        }

        fn execute(&mut self, effect: EngineEffect) -> Option<AudioMessage> {
            execute(effect, &mut self.deck)
        }

        fn pull(&mut self, frames: usize) {
            listen(&mut self.mixer, &mut self.heard, frames);
        }

        fn executed_while_pulled(
            &mut self,
            effect: EngineEffect,
        ) -> Option<AudioMessage> {
            let Self { deck, mixer, heard } = self;
            let executed = AtomicBool::new(false);
            thread::scope(|scope| {
                scope.spawn(|| {
                    while !executed.load(Ordering::Acquire) {
                        listen(mixer, heard, 16);
                    }
                });
                let answer = execute(effect, deck);
                executed.store(true, Ordering::Release);
                answer
            })
        }

        fn heard_since(&mut self, start: usize, count: usize) -> Vec<f32> {
            let deadline = Instant::now() + Duration::from_secs(10);
            while self.heard.len() < start + count && Instant::now() < deadline {
                self.pull(1_024);
            }
            self.heard.iter().skip(start).take(count).copied().collect()
        }
    }

    fn ramp_from(target: Duration, skipped: usize, count: usize) -> Vec<f32> {
        let first = usize::try_from(target.as_millis() * u128::from(RAMP_RATE) / 1_000)
            .unwrap()
            + skipped;
        (first..first + count)
            .map(|frame| {
                f32::from(i16::try_from(frame % 30_000 + 1).unwrap()) * RAMP_STEP
            })
            .collect()
    }

    fn paused_and_sought(frames: usize, target: Duration) -> Vec<f32> {
        let mut listening = Listening::playing_ramp(frames);
        assert!(listening.execute(EngineEffect::Pause).is_none());
        listening.pull(4_000);
        let start = listening.heard.len();

        assert!(
            listening
                .executed_while_pulled(EngineEffect::Seek(target))
                .is_none()
        );
        assert!(listening.execute(EngineEffect::Play).is_none());
        listening.pull(1);
        let Some(AudioMessage::Engine(EngineMessage::Reported(Some(reported)))) =
            listening.execute(EngineEffect::Report)
        else {
            panic!("a report answers the position");
        };
        assert!(reported >= target && reported < target + Duration::from_millis(1));

        listening.heard_since(start + usize::from(DECLICK_FRAMES), 200)
    }

    #[test]
    fn a_seek_while_paused_plays_from_the_target_after_play() {
        let target = Duration::from_secs(5);
        assert_eq!(
            paused_and_sought(80_000, target),
            ramp_from(target, usize::from(DECLICK_FRAMES), 200)
        );
    }

    #[rstest]
    #[case::decoded_to_the_end(16_000, Duration::from_millis(1_500))]
    #[case::still_decoding(80_000, Duration::from_millis(8_500))]
    fn a_seek_in_the_last_two_seconds_plays_from_the_target_after_play(
        #[case] frames: usize,
        #[case] target: Duration,
    ) {
        assert_eq!(
            paused_and_sought(frames, target),
            ramp_from(target, usize::from(DECLICK_FRAMES), 200)
        );
    }

    #[test]
    fn a_seek_while_playing_plays_from_the_target() {
        let target = Duration::from_secs(5);
        let mut listening = Listening::playing_ramp(80_000);
        let last_before = listening.heard.len() - 1;

        assert!(
            listening
                .executed_while_pulled(EngineEffect::Seek(target))
                .is_none()
        );
        listening.pull(16);
        thread::sleep(Duration::from_millis(100));

        let heard = listening.heard_since(last_before, 10_000);
        let after_the_jump: Vec<f32> = heard
            .windows(2)
            .position(|pair| matches!(pair, [previous, next] if *next != previous + RAMP_STEP))
            .map_or_else(Vec::new, |jump| heard.iter().skip(jump + 1).take(200).copied().collect());
        assert_eq!(after_the_jump, ramp_from(target, 0, 200));
    }

    #[test]
    fn more_retires_than_retired_slots_all_reach_the_driver() {
        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(64);
        let (feed_sender, feed_receiver) = crossbeam_channel::unbounded();
        let (mut driver, _driver_tap) = AudioDriver::new(
            AudioSettings::default(),
            callback_sender,
            FeedChannel {
                feed_sender,
                feed_receiver,
            },
        );
        let (spectrum_buffers, _spectrum_tap) = tap::spectrum_channel();
        let (output, mut mixer) = mixed_output(RAMP_RATE, &spectrum_buffers);
        driver.deck.output = Some(output);

        for _ in 0..RETIRED_SLOTS + 2 {
            driver.deck.stage(track(Revision::default().next()));
            assert!(driver.execute(EngineEffect::Start(Gain::UNITY)).is_none());
            mixer.mix(&mut [0.0_f32; 16]);
        }

        let waiting = driver
            .deck
            .output
            .as_mut()
            .map_or(0, |opened| opened.retired_voices.by_ref().count());
        assert_eq!(waiting, 1);
    }

    #[test]
    fn advance_answers_the_signals_an_incoming_track_raised_before_it_was_promoted() {
        let mut deck = deck_with_detached_output();
        let (_file, mut source, mut envelope, control, _feed) =
            played(1, Revision::default().next());
        assert_eq!(pulled(&mut source, &mut envelope, 16).len(), 8);
        deck.output.as_mut().unwrap().incoming_control = Some(control);

        let answer = execute(EngineEffect::Advance(Gain::UNITY), &mut deck);

        assert!(matches!(
            answer,
            Some(AudioMessage::SignalsTaken { role: SinkRole::Current, signals })
                if signals.contains(Signals::FINISHED)
        ));
    }

    #[test]
    fn advance_sets_the_gain_on_the_new_current_control() {
        let mut deck = deck_with_detached_output();
        let (_file, _source, _envelope, control, _feed) =
            played(1, Revision::default().next());
        deck.output.as_mut().unwrap().incoming_control = Some(control);

        assert!(
            execute(EngineEffect::Advance(Gain::from_amplitude(0.5)), &mut deck)
                .is_some()
        );

        assert_eq!(
            deck.output
                .as_ref()
                .and_then(|output| output.current_control.as_ref())
                .map(EnvelopeControl::volume),
            Some(Gain::from_amplitude(0.5))
        );
    }

    #[test]
    fn a_paused_resume_holds_the_current_voice_at_the_position() {
        let target = Duration::from_secs(5);
        let mut listening = Listening::playing_ramp(80_000);
        let file = ramp_file(1, 80_000);
        listening.deck.stage(DecodedTrack {
            revision: Revision::default().next().next(),
            decoder: decode(file.path()).unwrap(),
        });
        let start = listening.heard.len();

        assert!(
            listening
                .execute(EngineEffect::Resume {
                    gain: Gain::UNITY,
                    position: target,
                    playback: Playback::Paused,
                })
                .is_none()
        );
        listening.pull(8_000);
        thread::sleep(Duration::from_millis(100));
        listening.pull(8_000);

        assert!(listening.heard.len() <= start + usize::from(DECLICK_FRAMES));
        let Some(AudioMessage::Engine(EngineMessage::Reported(Some(reported)))) =
            listening.execute(EngineEffect::Report)
        else {
            panic!("a report answers the position");
        };
        assert!(reported >= target && reported < target + Duration::from_millis(1));
    }

    fn crossfade_revisions() -> [Revision; 2] {
        let fading = Revision::default().next();
        [fading, fading.next()]
    }

    #[test]
    fn a_cancelled_crossfade_holds_the_current_at_unity_and_silences_the_incoming() {
        let [current_revision, incoming_revision] = crossfade_revisions();
        let mut deck = deck_with_detached_output();
        let (file, mut current_source, mut current_envelope, current, _current_feed) =
            played(100, current_revision);
        let (
            _incoming_file,
            mut incoming_source,
            mut incoming_envelope,
            incoming,
            _incoming_feed,
        ) = played(100, incoming_revision);
        let output = deck.output.as_mut().unwrap();
        output.current_control = Some(current);
        output.incoming_fader = Some(Fader { control: incoming });

        for effect in [
            EngineEffect::Crossfade {
                duration: Duration::from_millis(10),
                incoming: Gain::UNITY,
            },
            EngineEffect::CancelCrossfade,
        ] {
            assert!(execute(effect, &mut deck).is_none());
        }

        let held = pulled(&mut current_source, &mut current_envelope, 1_024);
        let silenced = pulled(&mut incoming_source, &mut incoming_envelope, 1_024);
        assert_eq!(held.len(), 800);
        assert!(
            held.iter()
                .zip(&decoded(&file))
                .all(|(sample, unity)| (sample - unity).abs() < 1e-6)
        );
        assert_eq!(silenced.len(), 800);
        assert!(silenced.iter().all(|sample| sample.abs() < 1e-6));
        for (revision, role) in [
            (current_revision, SinkRole::Current),
            (incoming_revision, SinkRole::Incoming),
        ] {
            assert_same(
                deck.take_signals(revision),
                Some(AudioMessage::SignalsTaken {
                    role,
                    signals: Signals::FINISHED,
                }),
            );
        }
    }

    #[test]
    fn a_dropped_gapless_preload_leaves_no_incoming_control() {
        let mut deck = deck_with_detached_output();
        let (
            _incoming_file,
            _incoming_source,
            _incoming_envelope,
            incoming,
            _incoming_feed,
        ) = played(100, Revision::default().next());
        deck.output.as_mut().unwrap().incoming_control = Some(incoming);

        assert!(execute(EngineEffect::DropPreload, &mut deck).is_none());

        assert!(deck.output.as_ref().unwrap().incoming_control.is_none());
    }

    #[rstest]
    #[case(
        |output: &mut Output, fading, rising| {
            output.current_control = Some(fading);
            output.incoming_fader = Some(Fader { control: rising });
        },
        |duration, incoming| EngineEffect::Crossfade { duration, incoming },
        [SinkRole::Current, SinkRole::Incoming]
    )]
    #[case(
        |output: &mut Output, fading, rising| {
            output.outgoing_fader = Some(Fader { control: fading });
            output.current_control = Some(rising);
        },
        |duration, current| EngineEffect::Ramp { duration, current },
        [SinkRole::Outgoing, SinkRole::Current]
    )]
    fn a_crossfade_fades_out_the_current_and_raises_the_incoming_volume(
        #[case] place: fn(&mut Output, EnvelopeControl, EnvelopeControl),
        #[case] effect: fn(Duration, Gain) -> EngineEffect,
        #[case] roles: [SinkRole; 2],
    ) {
        let revisions = crossfade_revisions();
        let mut deck = deck_with_detached_output();
        let (
            _fading_file,
            mut fading_source,
            mut fading_envelope,
            fading,
            _fading_feed,
        ) = played(100, revisions[0]);
        let (rising_file, mut rising_source, mut rising_envelope, rising, _rising_feed) =
            played(100, revisions[1]);
        place(deck.output.as_mut().unwrap(), fading, rising);
        let gain = Gain::from_amplitude(0.5);

        assert!(execute(effect(Duration::from_millis(10), gain), &mut deck).is_none());

        let faded = pulled(&mut fading_source, &mut fading_envelope, 1_024);
        let raised = pulled(&mut rising_source, &mut rising_envelope, 1_024);
        let plain = decoded(&rising_file);
        assert_eq!(faded.len(), 800);
        assert!(faded[..40].iter().any(|sample| sample.abs() > 1e-6));
        assert!(faded[80..].iter().all(|sample| sample.abs() < 1e-6));
        assert_eq!(raised.len(), 800);
        assert!(raised[..40].iter().zip(&plain).all(|(sample, unity)| {
            sample.abs() < unity.abs() * gain.amplitude() + 1e-6
        }));
        assert!(
            raised[80..]
                .iter()
                .zip(&plain[80..])
                .all(|(sample, unity)| {
                    (sample - unity * gain.amplitude()).abs() < 1e-6
                })
        );
        for (revision, role) in revisions.into_iter().zip(roles) {
            assert_same(
                deck.take_signals(revision),
                Some(AudioMessage::SignalsTaken {
                    role,
                    signals: Signals(Signals::RAMPED.0 | Signals::FINISHED.0),
                }),
            );
        }
    }
}
