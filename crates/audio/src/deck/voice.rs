use std::{
    ops::{ControlFlow, Range},
    time::Duration,
};

use kernel::cmd::Playback;

use crate::deck::{
    envelope::Envelope,
    feed::FeedSource,
    varispeed::{VARISPEED_FRAMES, Varispeed},
};

pub(crate) const DECLICK_FRAMES: u16 = 256;

pub(crate) struct Voice {
    source: FeedSource,
    envelope: Envelope,
    pub(crate) varispeed: Varispeed,
    input: Box<[f32]>,
    pending: Range<usize>,
    channels: usize,
    pub(crate) playback: Playback,
    pub(crate) declick: u16,
}

impl std::fmt::Debug for Voice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Voice")
            .field("playback", &self.playback)
            .field("declick", &self.declick)
            .finish_non_exhaustive()
    }
}

impl Voice {
    pub(crate) fn new(
        source: FeedSource,
        envelope: Envelope,
        varispeed: Varispeed,
    ) -> Self {
        let channels = usize::from(source.channels().max(1));
        Self {
            source,
            envelope,
            varispeed,
            input: vec![0.0; VARISPEED_FRAMES * channels].into_boxed_slice(),
            pending: 0..0,
            channels,
            playback: Playback::Paused,
            declick: 0,
        }
    }

    pub(crate) fn seek(&mut self, target: Duration) {
        self.source.seek(target);
        self.envelope.seek();
        self.varispeed.seek();
        match self.playback {
            Playback::Playing => {}
            Playback::Paused => self.pending = 0..0,
        }
    }

    pub(crate) fn mix(&mut self, out: &mut [f32]) -> ControlFlow<usize, usize> {
        let frames = out.len() / self.channels;
        let wanted = match self.playback {
            Playback::Playing => frames,
            Playback::Paused => frames.min(usize::from(self.declick)),
        };
        let (out, _) = out.split_at_mut(wanted * self.channels);
        let flow = self.fill(out);
        let (ControlFlow::Continue(written) | ControlFlow::Break(written)) = flow;
        self.declick(out.split_at_mut(written * self.channels).0);
        flow
    }

    fn fill(&mut self, out: &mut [f32]) -> ControlFlow<usize, usize> {
        let wanted = out.len() / self.channels;
        let mut written = 0;
        while written < wanted {
            if self.pending.is_empty() && self.pull() == 0 {
                let rest = out.split_at_mut(written * self.channels).1;
                return match self.varispeed.flush(rest) {
                    Ok(flushed) if written + flushed == wanted => {
                        ControlFlow::Continue(wanted)
                    }
                    Ok(flushed) => ControlFlow::Break(written + flushed),
                    Err(_) => ControlFlow::Break(written),
                };
            }
            let input = self.input.get(self.pending.clone()).unwrap_or(&[]);
            let rest = out.split_at_mut(written * self.channels).1;
            match self.varispeed.fill(input, rest) {
                Ok((read, filled)) => {
                    self.pending.start += read * self.channels;
                    written += filled;
                }
                Err(_) => return ControlFlow::Break(written),
            }
        }
        ControlFlow::Continue(written)
    }

    fn pull(&mut self) -> usize {
        let frames = self
            .varispeed
            .input_frames_next()
            .clamp(1, VARISPEED_FRAMES);
        let input = self
            .input
            .get_mut(..frames * self.channels)
            .unwrap_or(&mut []);
        let wanted = input.len();
        let read = self.source.read(input);
        self.envelope.read(input.get_mut(..read).unwrap_or(&mut []));
        let pulled = read / self.channels;
        if read < wanted {
            self.envelope.end();
        }
        self.pending = 0..pulled * self.channels;
        pulled
    }

    fn declick(&mut self, out: &mut [f32]) {
        for frame in out.chunks_exact_mut(self.channels) {
            self.declick = match self.playback {
                Playback::Playing => (self.declick + 1).min(DECLICK_FRAMES),
                Playback::Paused => self.declick.saturating_sub(1),
            };
            let amplitude = f32::from(self.declick) / f32::from(DECLICK_FRAMES);
            frame.iter_mut().for_each(|sample| *sample *= amplitude);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::{
        cmd::Playback,
        domain::{bounded::Bounded, speed::Speed},
    };

    use crate::{
        deck::{
            mixer::{
                MixerChannel,
                MixerOrder,
                tests::{MONO_8K, attach, channel, mixed, served, voice},
            },
            source::tests::{decoded, ramp_file},
            varispeed::OutputFormat,
            voice::DECLICK_FRAMES,
        },
        engine::message::SinkRole,
    };

    const STEREO_8K: OutputFormat = OutputFormat {
        channels: 2,
        rate: 8_000,
    };

    #[test]
    fn an_attached_playing_voice_renders_its_file_bit_exact_after_the_declick() {
        let file = ramp_file(1, 2_000);
        let expected = decoded(&file);
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                retired_voices: _retired_voices,
            } = channel(MONO_8K);
            let (current_voice, _current_control) = voice(&file, MONO_8K, feed_sender);
            attach(&mut control, SinkRole::Current, current_voice);
            control.order(MixerOrder::Transport(Playback::Playing));

            let out = mixed(&mut mixer, 2_000);

            let declick = usize::from(DECLICK_FRAMES);
            assert_eq!(out.get(declick..), expected.get(declick..));
            assert!(
                out.iter()
                    .zip(&expected)
                    .take(declick)
                    .all(|(sample, source)| sample.abs() <= source.abs())
            );
        });
    }

    #[test]
    fn a_doubled_speed_consumes_twice_the_source_frames() {
        let file = ramp_file(1, 16_000);
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                retired_voices: _retired_voices,
            } = channel(MONO_8K);
            let (current_voice, current_control) = voice(&file, MONO_8K, feed_sender);
            attach(&mut control, SinkRole::Current, current_voice);
            control.order(MixerOrder::Transport(Playback::Playing));
            control.order(MixerOrder::Speed(Speed::clamped(2.0)));

            mixed(&mut mixer, 1_024);

            let consumed = current_control.position();
            assert!(
                (Duration::from_millis(256)..Duration::from_millis(320))
                    .contains(&consumed),
                "{consumed:?}"
            );
        });
    }

    #[test]
    fn a_mono_file_on_a_stereo_device_plays_each_frame_once_in_both_channels() {
        let file = ramp_file(1, 2_000);
        let expected: Vec<[f32; 2]> =
            decoded(&file).iter().map(|sample| [*sample; 2]).collect();
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                retired_voices: _retired_voices,
            } = channel(STEREO_8K);
            let (current_voice, _current_control) =
                voice(&file, STEREO_8K, feed_sender);
            attach(&mut control, SinkRole::Current, current_voice);
            control.order(MixerOrder::Transport(Playback::Playing));

            let out = mixed(&mut mixer, 4_000);

            let frames = out.as_chunks::<2>().0;
            let declick = usize::from(DECLICK_FRAMES);
            assert_eq!(frames.get(declick..), expected.get(declick..));
        });
    }

    #[test]
    fn a_stereo_file_on_a_mono_device_plays_the_average_of_its_channels() {
        let file = ramp_file(2, 2_000);
        let expected: Vec<f32> = decoded(&file)
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[left, right]| (left + right) / 2.0)
            .collect();
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                retired_voices: _retired_voices,
            } = channel(MONO_8K);
            let (current_voice, _current_control) = voice(&file, MONO_8K, feed_sender);
            attach(&mut control, SinkRole::Current, current_voice);
            control.order(MixerOrder::Transport(Playback::Playing));

            let out = mixed(&mut mixer, 2_000);

            let declick = usize::from(DECLICK_FRAMES);
            assert_eq!(out.get(declick..), expected.get(declick..));
        });
    }

    #[test]
    fn a_paused_incoming_voice_freezes_after_the_declick() {
        let file = ramp_file(1, 8_000);
        served(|feed_sender| {
            let MixerChannel {
                mut mixer,
                mut control,
                retired_voices: _retired_voices,
            } = channel(MONO_8K);
            let (incoming_voice, incoming_control) = voice(&file, MONO_8K, feed_sender);
            attach(&mut control, SinkRole::Incoming, incoming_voice);
            control.order(MixerOrder::RolePlayback {
                role: SinkRole::Incoming,
                playback: Playback::Playing,
            });
            mixed(&mut mixer, 1_024);
            assert_eq!(incoming_control.position(), Duration::from_millis(128));

            control.order(MixerOrder::RolePlayback {
                role: SinkRole::Incoming,
                playback: Playback::Paused,
            });
            let out = mixed(&mut mixer, 1_024);
            let declick = usize::from(DECLICK_FRAMES);
            assert_eq!(incoming_control.position(), Duration::from_millis(160));
            assert!(out.iter().take(declick - 1).all(|&sample| sample != 0.0));
            assert!(out.iter().skip(declick).all(|&sample| sample == 0.0));

            mixed(&mut mixer, 1_024);
            assert_eq!(incoming_control.position(), Duration::from_millis(160));
        });
    }
}
