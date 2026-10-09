use std::time::Duration;

use crossbeam_channel::Sender;
use kernel::{
    cmd::Playback,
    domain::{revision::Revision, speed::Speed},
    message::DecodeError,
};

use crate::{
    deck::{
        envelope::EnvelopeControl,
        feed::{FeedCmd, feed_channel, play},
        mixer::{MixerControl, MixerOrder, RetiredVoices},
        source::DecodedTrack,
        varispeed::{OutputFormat, Varispeed},
        voice::Voice,
    },
    engine::message::{AudioMessage, SinkRole},
};

pub(crate) struct Fader {
    pub(crate) control: EnvelopeControl,
}

#[derive(Clone, Copy)]
pub(crate) struct Feeding<'a> {
    pub(crate) speed: Speed,
    pub(crate) callback_sender: &'a Sender<AudioMessage>,
    pub(crate) feed_sender: &'a Sender<FeedCmd>,
}

pub(crate) struct Output {
    pub(crate) mixer_control: MixerControl,
    pub(crate) retired_voices: RetiredVoices,
    pub(crate) format: OutputFormat,
    pub(crate) current_control: Option<EnvelopeControl>,
    pub(crate) incoming_control: Option<EnvelopeControl>,
    pub(crate) incoming_fader: Option<Fader>,
    pub(crate) outgoing_fader: Option<Fader>,
    pub(crate) current_playback: Playback,
}

impl Output {
    pub(crate) fn new(
        mixer_control: MixerControl,
        retired_voices: RetiredVoices,
        format: OutputFormat,
    ) -> Self {
        Self {
            mixer_control,
            retired_voices,
            format,
            current_control: None,
            incoming_control: None,
            incoming_fader: None,
            outgoing_fader: None,
            current_playback: Playback::Paused,
        }
    }

    pub(crate) fn voice(
        &self,
        decoded_track: DecodedTrack,
        feeding: Feeding<'_>,
    ) -> Result<(Box<Voice>, EnvelopeControl), DecodeError> {
        let Feeding {
            speed,
            callback_sender,
            feed_sender,
        } = feeding;
        let varispeed =
            Varispeed::new(decoded_track.decoder.sample_rate(), self.format, speed)
                .or(Err(DecodeError::Unsupported))?;
        let (source, feed) =
            feed_channel(decoded_track, self.format.channels, callback_sender.clone());
        let (envelope, control) = play(&source, feed, feed_sender);
        Ok((Box::new(Voice::new(source, envelope, varispeed)), control))
    }

    pub(crate) fn swap_current(&mut self, speed: Speed) {
        self.outgoing_fader = None;
        self.current_control = None;
        self.mixer_control
            .order(MixerOrder::Drop(SinkRole::Outgoing));
        self.mixer_control
            .order(MixerOrder::Drop(SinkRole::Current));
        self.mixer_control.order(MixerOrder::Speed(speed));
    }

    pub(crate) fn retire_current(&mut self, speed: Speed) {
        self.outgoing_fader =
            self.current_control.take().map(|control| Fader { control });
        self.mixer_control.order(MixerOrder::Retire);
        self.mixer_control.order(MixerOrder::Speed(speed));
    }

    pub(crate) fn position(&self) -> (Duration, Playback) {
        (
            self.current_control
                .as_ref()
                .map_or(Duration::ZERO, EnvelopeControl::position),
            self.current_playback,
        )
    }

    pub(crate) fn promote(&mut self) {
        if let Some(Fader { control }) = self.incoming_fader.take() {
            self.mixer_control.order(MixerOrder::Promote);
            self.current_control = Some(control);
        }
    }

    #[must_use]
    pub(crate) fn holder(
        &self,
        revision: Revision,
    ) -> Option<(SinkRole, &EnvelopeControl)> {
        [
            (SinkRole::Current, self.current_control.as_ref()),
            (
                SinkRole::Incoming,
                self.incoming_fader.as_ref().map(|fader| &fader.control),
            ),
            (
                SinkRole::Outgoing,
                self.outgoing_fader.as_ref().map(|fader| &fader.control),
            ),
        ]
        .into_iter()
        .find_map(|(role, control)| {
            control
                .filter(|control| control.revision() == revision)
                .map(|control| (role, control))
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use kernel::domain::speed::Speed;

    use crate::{
        deck::{
            mixer::{Mixer, MixerChannel, mixer_channel},
            output::Output,
            varispeed::OutputFormat,
        },
        tap::{SpectrumBuffers, spectrum_channel},
    };

    pub(crate) fn mixed_output(
        rate: u32,
        spectrum_buffers: &SpectrumBuffers,
    ) -> (Output, Mixer) {
        let format = OutputFormat { channels: 1, rate };
        let MixerChannel {
            mixer,
            control,
            retired_voices,
        } = mixer_channel(format, Speed::default(), spectrum_buffers);
        (Output::new(control, retired_voices, format), mixer)
    }

    pub(crate) fn detached_output() -> Output {
        let (spectrum_buffers, _spectrum_tap) = spectrum_channel();
        mixed_output(44_100, &spectrum_buffers).0
    }
}
