use std::{ops::ControlFlow, time::Duration};

use cpal::{FromSample, SizedSample};
use kernel::{cmd::Playback, domain::speed::Speed};
use rtrb::{Consumer, PopError, Producer, PushError, RingBuffer};

use crate::{
    deck::{
        varispeed::{OutputFormat, VARISPEED_FRAMES},
        voice::Voice,
    },
    engine::message::SinkRole,
    tap::{SpectrumBuffers, SpectrumWriter},
};

pub(crate) const MIXER_ORDERS: usize = 32;
pub(crate) const RETIRED_SLOTS: usize = MIXER_ORDERS + 4;

#[derive(Debug)]
pub(crate) enum MixerOrder {
    Attach { role: SinkRole, voice: Box<Voice> },
    Queue(Box<Voice>),
    Promote,
    Retire,
    Drop(SinkRole),
    Transport(Playback),
    RolePlayback { role: SinkRole, playback: Playback },
    Speed(Speed),
    Seek(Duration),
}

#[derive(Debug)]
struct Voices {
    current: Option<Box<Voice>>,
    incoming: Option<Box<Voice>>,
    outgoing: Option<Box<Voice>>,
}

impl Voices {
    fn get(&mut self, role: SinkRole) -> &mut Option<Box<Voice>> {
        match role {
            SinkRole::Current => &mut self.current,
            SinkRole::Incoming => &mut self.incoming,
            SinkRole::Outgoing => &mut self.outgoing,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Mixer {
    voices: Voices,
    queued_voice: Option<Box<Voice>>,
    speed: Speed,
    channels: usize,
    orders: Consumer<MixerOrder>,
    retired: Producer<Box<Voice>>,
    spectrum_writer: SpectrumWriter,
    block: Box<[f32]>,
    scratch: Box<[f32]>,
}

impl Mixer {
    #[sanitize(realtime = "nonblocking")]
    pub(crate) fn mix<T: SizedSample + FromSample<f32>>(&mut self, out: &mut [T]) {
        for _ in 0..MIXER_ORDERS {
            match self.orders.pop() {
                Ok(order) => self.order(order),
                Err(PopError::Empty) => break,
            }
        }
        for chunk in out.chunks_mut(self.block.len()) {
            let len = chunk.len();
            self.block.fill(0.0);
            for role in [SinkRole::Current, SinkRole::Incoming, SinkRole::Outgoing] {
                let voice = self.voices.get(role);
                let mut written = 0;
                while let Some(playing) = voice.as_mut() {
                    let rest = self.scratch.get_mut(written..len).unwrap_or(&mut []);
                    match playing.mix(rest) {
                        ControlFlow::Continue(frames) => {
                            written += frames * self.channels;
                            break;
                        }
                        ControlFlow::Break(frames) => {
                            written += frames * self.channels;
                            let next = match role {
                                SinkRole::Current => {
                                    self.queued_voice.take().map(|mut queued_voice| {
                                        queued_voice.playback = playing.playback;
                                        queued_voice.declick = playing.declick;
                                        queued_voice
                                    })
                                }
                                SinkRole::Incoming | SinkRole::Outgoing => None,
                            };
                            retire(&mut self.retired, std::mem::replace(voice, next));
                        }
                    }
                }
                let mixed = self.scratch.get(..written).unwrap_or(&[]);
                for (sum, sample) in self.block.iter_mut().zip(mixed) {
                    *sum += sample;
                }
            }
            let block = self.block.get(..len).unwrap_or(&[]);
            self.spectrum_writer.push(block);
            for (cell, sample) in chunk.iter_mut().zip(block) {
                *cell = T::from_sample(*sample);
            }
        }
    }

    fn all_voices(&mut self) -> impl Iterator<Item = &mut Box<Voice>> {
        [
            &mut self.voices.current,
            &mut self.voices.incoming,
            &mut self.voices.outgoing,
            &mut self.queued_voice,
        ]
        .into_iter()
        .flatten()
    }

    fn order(&mut self, order: MixerOrder) {
        match order {
            MixerOrder::Attach { role, mut voice } => {
                voice.varispeed.set_speed(self.speed);
                let replaced = self.voices.get(role).replace(voice);
                retire(&mut self.retired, replaced);
            }
            MixerOrder::Queue(mut voice) => {
                voice.varispeed.set_speed(self.speed);
                let replaced = self.queued_voice.replace(voice);
                retire(&mut self.retired, replaced);
            }
            MixerOrder::Promote => {
                if let Some(incoming_voice) = self.voices.incoming.take() {
                    let replaced = self.voices.current.replace(incoming_voice);
                    retire(&mut self.retired, replaced);
                    retire(&mut self.retired, self.queued_voice.take());
                }
            }
            MixerOrder::Retire => {
                let replaced = std::mem::replace(
                    &mut self.voices.outgoing,
                    self.voices.current.take(),
                );
                retire(&mut self.retired, replaced);
                retire(&mut self.retired, self.queued_voice.take());
            }
            MixerOrder::Drop(role) => {
                let dropped = self.voices.get(role).take();
                retire(&mut self.retired, dropped);
                match role {
                    SinkRole::Current | SinkRole::Incoming => {
                        retire(&mut self.retired, self.queued_voice.take());
                    }
                    SinkRole::Outgoing => {}
                }
            }
            MixerOrder::Transport(playback) => {
                self.all_voices()
                    .for_each(|voice| voice.playback = playback);
            }
            MixerOrder::RolePlayback { role, playback } => {
                if let Some(voice) = self.voices.get(role) {
                    voice.playback = playback;
                }
            }
            MixerOrder::Speed(speed) => {
                self.speed = speed;
                self.all_voices()
                    .for_each(|voice| voice.varispeed.set_speed(speed));
            }
            MixerOrder::Seek(target) => {
                if let Some(voice) = self.voices.current.as_mut() {
                    voice.seek(target);
                }
            }
        }
    }
}

fn retire(retired: &mut Producer<Box<Voice>>, voice: Option<Box<Voice>>) {
    if let Some(voice) = voice {
        match retired.push(voice) {
            Ok(()) | Err(PushError::Full(_)) => {}
        }
    }
}

#[derive(Debug)]
pub(crate) struct MixerControl {
    orders: Producer<MixerOrder>,
}

impl MixerControl {
    pub(crate) fn order(&mut self, order: MixerOrder) {
        match self.orders.push(order) {
            Ok(()) | Err(PushError::Full(_)) => {}
        }
    }
}

#[derive(Debug)]
pub(crate) struct RetiredVoices {
    retired: Consumer<Box<Voice>>,
}

impl Iterator for RetiredVoices {
    type Item = Box<Voice>;

    fn next(&mut self) -> Option<Box<Voice>> {
        self.retired.pop().ok()
    }
}

#[derive(Debug)]
pub(crate) struct MixerChannel {
    pub(crate) mixer: Mixer,
    pub(crate) control: MixerControl,
    pub(crate) retired_voices: RetiredVoices,
}

#[must_use]
pub(crate) fn mixer_channel(
    format: OutputFormat,
    speed: Speed,
    spectrum_buffers: &SpectrumBuffers,
) -> MixerChannel {
    let (sender, receiver) = RingBuffer::new(MIXER_ORDERS);
    let (retired_sender, retired_receiver) = RingBuffer::new(RETIRED_SLOTS);
    let channels = usize::from(format.channels.max(1));
    let block = vec![0.0; VARISPEED_FRAMES * channels].into_boxed_slice();
    MixerChannel {
        mixer: Mixer {
            voices: Voices {
                current: None,
                incoming: None,
                outgoing: None,
            },
            queued_voice: None,
            speed,
            channels,
            orders: receiver,
            retired: retired_sender,
            spectrum_writer: SpectrumWriter::new(spectrum_buffers, format.channels),
            scratch: block.clone(),
            block,
        },
        control: MixerControl { orders: sender },
        retired_voices: RetiredVoices {
            retired: retired_receiver,
        },
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{thread, time::Duration};

    use crossbeam_channel::Sender;
    use kernel::{
        cmd::Playback,
        domain::{bounded::Bounded, revision::Revision, speed::Speed},
    };
    use tempfile::NamedTempFile;

    use crate::{
        deck::{
            envelope::{EnvelopeControl, Ramp},
            feed::{FeedCmd, feed_channel, play, serve::serve},
            mixer::{Mixer, MixerChannel, MixerControl, MixerOrder, mixer_channel},
            source::{
                DecodedTrack,
                decode,
                tests::{decoded, ramp_file},
            },
            varispeed::{OutputFormat, Varispeed},
            voice::{DECLICK_FRAMES, Voice},
        },
        engine::message::SinkRole,
        tap::spectrum_channel,
    };

    pub(crate) const MONO_8K: OutputFormat = OutputFormat {
        channels: 1,
        rate: 8_000,
    };

    pub(crate) fn served(test: impl FnOnce(&Sender<FeedCmd>)) {
        let (feed_sender, feed_receiver) = crossbeam_channel::unbounded();
        thread::scope(|scope| {
            scope.spawn(|| serve(&feed_receiver));
            test(&feed_sender);
            drop(feed_sender);
        });
    }

    const MONO_12K: OutputFormat = OutputFormat {
        channels: 1,
        rate: 12_000,
    };

    pub(crate) fn voice(
        file: &NamedTempFile,
        format: OutputFormat,
        feed_sender: &Sender<FeedCmd>,
    ) -> (Box<Voice>, EnvelopeControl) {
        let (callback_sender, _callback_receiver) = crossbeam_channel::unbounded();
        let decoded_track = DecodedTrack {
            revision: Revision::default(),
            decoder: decode(file.path()).unwrap(),
        };
        let (source, feed) =
            feed_channel(decoded_track, format.channels, callback_sender);
        let (envelope, control) = play(&source, feed, feed_sender);
        let varispeed =
            Varispeed::new(source.sample_rate(), format, Speed::default()).unwrap();
        let voice = Voice::new(source, envelope, varispeed);
        (Box::new(voice), control)
    }

    pub(crate) fn channel(format: OutputFormat) -> MixerChannel {
        let (spectrum_buffers, _spectrum_tap) = spectrum_channel();
        mixer_channel(format, Speed::default(), &spectrum_buffers)
    }

    pub(crate) fn attach(
        control: &mut MixerControl,
        role: SinkRole,
        voice: Box<Voice>,
    ) {
        control.order(MixerOrder::Attach { role, voice });
    }

    pub(crate) fn mixed(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
        let mut out = vec![1.0_f32; frames];
        mixer.mix(&mut out);
        out
    }

    #[test]
    fn a_mixer_without_a_voice_writes_silence() {
        let MixerChannel {
            mut mixer,
            control: _control,
            retired_voices: _retired_voices,
        } = channel(MONO_8K);
        assert!(mixed(&mut mixer, 1_000).iter().all(|&sample| sample == 0.0));
        let mut out = [1_i16; 64];
        mixer.mix(&mut out);
        assert_eq!(out, [0_i16; 64]);
    }

    #[test]
    fn a_queued_voice_continues_in_the_same_buffer_without_a_zero_frame() {
        let first = ramp_file(1, 1_000);
        let second = ramp_file(1, 3_000);
        let (first_expected, second_expected) = (decoded(&first), decoded(&second));
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                mut retired_voices,
            } = channel(MONO_8K);
            let (current_voice, _current_control) = voice(&first, MONO_8K, feed_sender);
            let (queued_voice, _queued_control) = voice(&second, MONO_8K, feed_sender);
            attach(&mut control, SinkRole::Current, current_voice);
            control.order(MixerOrder::Queue(queued_voice));
            control.order(MixerOrder::Transport(Playback::Playing));

            let out = mixed(&mut mixer, 2_048);

            let declick = usize::from(DECLICK_FRAMES);
            assert_eq!(out.get(declick..1_000), first_expected.get(declick..));
            assert_eq!(out.get(1_000..), second_expected.get(..1_048));
            assert!(out.iter().all(|&sample| sample != 0.0));
            assert_eq!(retired_voices.by_ref().count(), 1);
        });
    }

    #[test]
    fn the_retired_ring_keeps_one_full_order_queue_of_voices_and_four_more() {
        let file = ramp_file(1, 64);
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                retired_voices,
            } = channel(MONO_8K);
            for _ in 0..2 {
                for _ in 0..32 {
                    let (attached, _attached_control) =
                        voice(&file, MONO_8K, feed_sender);
                    attach(&mut control, SinkRole::Current, attached);
                }
                mixed(&mut mixer, 64);
            }
            assert_eq!(retired_voices.count(), 36);
        });
    }

    #[test]
    fn dropping_the_incoming_role_retires_a_queued_voice() {
        served(|feed_sender| {
            let mut channel = channel(MONO_8K);
            let (queued, _control) = voice(&ramp_file(1, 8_000), MONO_8K, feed_sender);
            channel.control.order(MixerOrder::Queue(queued));
            channel.control.order(MixerOrder::Drop(SinkRole::Incoming));
            mixed(&mut channel.mixer, 64);
            assert_eq!(channel.retired_voices.count(), 1);
        });
    }

    #[test]
    fn a_resampled_voice_plays_its_tail_before_the_queued_voice_starts() {
        let first = ramp_file(1, 1_000);
        let second = ramp_file(1, 3_000);
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                retired_voices: _retired_voices,
            } = channel(MONO_12K);
            let (current_voice, _current_control) =
                voice(&first, MONO_12K, feed_sender);
            let (queued_voice, _queued_control) = voice(&second, MONO_12K, feed_sender);
            attach(&mut control, SinkRole::Current, current_voice);
            control.order(MixerOrder::Queue(queued_voice));
            control.order(MixerOrder::Transport(Playback::Playing));

            let out = mixed(&mut mixer, 2_400);

            let peak = out.iter().copied().fold(0.0_f32, f32::max);
            let join = out
                .iter()
                .skip(1_000)
                .position(|sample| sample.abs() < peak / 10.0)
                .map(|offset| offset + 1_000);
            assert!(
                join.is_some_and(|join| (1_495..1_505).contains(&join)),
                "{join:?}"
            );
            assert!(out.iter().all(|&sample| sample != 0.0));
        });
    }

    #[test]
    fn promote_retire_and_drop_move_voices_and_retire_the_replaced_ones() {
        let files = [
            ramp_file(1, 8_000),
            ramp_file(1, 8_000),
            ramp_file(1, 8_000),
        ];
        let [first_file, second_file, third_file] = &files;
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                mut retired_voices,
            } = channel(MONO_8K);
            let (first, first_control) = voice(first_file, MONO_8K, feed_sender);
            let (second, second_control) = voice(second_file, MONO_8K, feed_sender);
            let (third, third_control) = voice(third_file, MONO_8K, feed_sender);
            attach(&mut control, SinkRole::Current, first);
            attach(&mut control, SinkRole::Incoming, second);
            control.order(MixerOrder::Transport(Playback::Playing));
            mixed(&mut mixer, 512);

            control.order(MixerOrder::Retire);
            control.order(MixerOrder::Promote);
            mixed(&mut mixer, 512);
            assert_eq!(
                (
                    first_control.position(),
                    second_control.position(),
                    retired_voices.by_ref().count()
                ),
                (Duration::from_millis(128), Duration::from_millis(128), 0)
            );

            control.order(MixerOrder::Drop(SinkRole::Outgoing));
            mixed(&mut mixer, 512);
            assert_eq!(
                (
                    first_control.position(),
                    second_control.position(),
                    retired_voices.by_ref().count()
                ),
                (Duration::from_millis(128), Duration::from_millis(192), 1)
            );

            attach(&mut control, SinkRole::Current, third);
            mixed(&mut mixer, 512);
            assert_eq!(
                (
                    second_control.position(),
                    third_control.position(),
                    retired_voices.by_ref().count()
                ),
                (Duration::from_millis(192), Duration::ZERO, 1)
            );

            control.order(MixerOrder::Drop(SinkRole::Current));
            mixed(&mut mixer, 64);
            assert_eq!(retired_voices.by_ref().count(), 1);
        });
    }

    #[test]
    fn the_mixer_renders_a_second_without_locking_or_allocating() {
        let short = ramp_file(1, 3_000);
        let long = ramp_file(1, 16_000);
        let crossing = ramp_file(1, 16_000);
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                mut retired_voices,
            } = channel(MONO_8K);
            let (current_voice, _current_control) = voice(&short, MONO_8K, feed_sender);
            let (queued_voice, _queued_control) = voice(&long, MONO_8K, feed_sender);
            let (incoming_voice, mut incoming_control) =
                voice(&crossing, MONO_8K, feed_sender);
            attach(&mut control, SinkRole::Current, current_voice);
            control.order(MixerOrder::Queue(queued_voice));
            attach(&mut control, SinkRole::Incoming, incoming_voice);
            control.order(MixerOrder::RolePlayback {
                role: SinkRole::Current,
                playback: Playback::Playing,
            });
            let mut out = vec![0.0_f32; 512];
            let mut rendered = Vec::with_capacity(16 * out.len());
            for block in 0..16 {
                match block {
                    4 => control.order(MixerOrder::Speed(Speed::clamped(1.5))),
                    8 => {
                        let length =
                            incoming_control.frames(Duration::from_millis(500));
                        incoming_control.ramp(Ramp::fade_in(length));
                        control.order(MixerOrder::RolePlayback {
                            role: SinkRole::Incoming,
                            playback: Playback::Playing,
                        });
                    }
                    _ => {}
                }
                mixer.mix(&mut out);
                rendered.extend_from_slice(&out);
                thread::sleep(Duration::from_millis(40));
            }
            assert!(rendered.iter().all(|sample| sample.is_finite()));
            assert!(rendered.iter().skip(8 * 512).any(|&sample| sample != 0.0));
            assert_eq!(retired_voices.by_ref().count(), 1);
        });
    }
}
