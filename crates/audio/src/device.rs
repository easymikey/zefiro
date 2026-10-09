use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use cpal::{
    FromSample,
    SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use crossbeam_channel::{Sender, TrySendError};
use kernel::domain::{
    device::{DeviceDefault, DeviceName, ListedDevice, OutputDevice},
    speed::Speed,
    transport::OutputError,
};

use crate::{
    deck::{
        event::DeckEvent,
        mixer::{MixerChannel, MixerControl, RetiredVoices, mixer_channel},
        varispeed::OutputFormat,
    },
    engine::message::AudioMessage,
    error::{DeviceError, OpenError, output_error},
    tap::SpectrumBuffers,
};

pub(crate) struct Opening<'a> {
    pub(crate) speed: Speed,
    pub(crate) spectrum_buffers: &'a SpectrumBuffers,
    pub(crate) callback_sender: &'a Sender<AudioMessage>,
    pub(crate) output_loss: &'a OutputLoss,
}

pub(crate) struct OpenedOutput {
    pub(crate) stream: cpal::Stream,
    pub(crate) format: OutputFormat,
    pub(crate) mixer_control: MixerControl,
    pub(crate) retired_voices: RetiredVoices,
    pub(crate) device_name: Option<DeviceName>,
}

const CLEAR: u8 = 0;
const DEVICE_GONE: u8 = 1;
const BACKEND: u8 = 2;

#[derive(Debug, Clone, Default)]
pub(crate) struct OutputLoss(Arc<AtomicU8>);

impl OutputLoss {
    pub(crate) fn report(
        &self,
        error: OutputError,
        callback_sender: &Sender<AudioMessage>,
    ) {
        match callback_sender.try_send(AudioMessage::Deck(DeckEvent::OutputLost(error)))
        {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                self.0.store(loss_code(error), Ordering::Release);
            }
        }
    }

    pub(crate) fn resend(&self, callback_sender: &Sender<AudioMessage>) {
        if let Some(error) = latched(self.0.swap(CLEAR, Ordering::AcqRel)) {
            self.report(error, callback_sender);
        }
    }
}

fn loss_code(error: OutputError) -> u8 {
    match error {
        OutputError::DeviceGone => DEVICE_GONE,
        OutputError::Backend => BACKEND,
    }
}

fn latched(code: u8) -> Option<OutputError> {
    match code {
        DEVICE_GONE => Some(OutputError::DeviceGone),
        BACKEND => Some(OutputError::Backend),
        _ => None,
    }
}

pub(crate) fn list_output_devices() -> Result<Vec<ListedDevice>, cpal::DevicesError> {
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|device| readable(device.name()));
    let names = host.output_devices()?.map(|device| device.name());
    Ok(listed(names, default_name.as_ref()))
}

fn listed(
    names: impl Iterator<Item = Result<String, cpal::DeviceNameError>>,
    default_name: Option<&DeviceName>,
) -> Vec<ListedDevice> {
    names
        .filter_map(readable)
        .map(|name| {
            let default = if default_name == Some(&name) {
                DeviceDefault::Yes
            } else {
                DeviceDefault::No
            };
            ListedDevice { name, default }
        })
        .collect()
}

fn readable(name: Result<String, cpal::DeviceNameError>) -> Option<DeviceName> {
    match name {
        Ok(name) => DeviceName::new(name).ok(),
        Err(cpal::DeviceNameError::BackendSpecific { .. }) => None,
    }
}

fn output_loss_callback(
    callback_sender: &Sender<AudioMessage>,
    output_loss: &OutputLoss,
) -> impl FnMut(cpal::StreamError) + Clone + Send + 'static {
    let (callback_sender, output_loss) = (callback_sender.clone(), output_loss.clone());
    move |error: cpal::StreamError| {
        output_loss.report(output_error(&error), &callback_sender);
    }
}

pub(crate) fn open_output(
    device: &OutputDevice,
    opening: &Opening<'_>,
) -> Result<OpenedOutput, DeviceError> {
    let no_device = |source: OpenError| DeviceError::NoDevice {
        requested_device: device.clone(),
        source,
    };
    let found = match device {
        OutputDevice::Named(name) => find_by_name(name)
            .map_err(|source| DeviceError::Lookup {
                requested_device: device.clone(),
                source,
            })?
            .ok_or_else(|| DeviceError::NotFound(name.clone()))?,
        OutputDevice::SystemDefault => cpal::default_host()
            .default_output_device()
            .ok_or_else(|| {
                no_device(OpenError::Config(
                    cpal::DefaultStreamConfigError::DeviceNotAvailable,
                ))
            })?,
    };
    let opened_output = found
        .default_output_config()
        .map_err(OpenError::Config)
        .and_then(|config| open_config(&found, &config, opening));
    match (opened_output, device) {
        (Ok(opened_output), _) => Ok(opened_output),
        (Err(error), OutputDevice::Named(_)) => Err(error),
        (Err(error), OutputDevice::SystemDefault) => found
            .supported_output_configs()
            .map_err(OpenError::Configs)
            .and_then(|configs| {
                pick_config(configs.collect())
                    .iter()
                    .map(|config| open_config(&found, config, opening))
                    .find(Result::is_ok)
                    .unwrap_or(Err(error))
            }),
    }
    .map_err(no_device)
}

fn pick_config(
    mut supported: Vec<cpal::SupportedStreamConfigRange>,
) -> Vec<cpal::SupportedStreamConfig> {
    supported.sort_by(|left, right| right.cmp_default_heuristics(left));
    supported
        .into_iter()
        .flat_map(|range| {
            let (min, max) = (range.min_sample_rate(), range.max_sample_rate());
            let cd_rate = cpal::SampleRate(44_100);
            [
                Some(range.with_max_sample_rate()),
                (min < cd_rate && cd_rate < max)
                    .then(|| range.with_sample_rate(cd_rate)),
                (min < max).then(|| range.with_sample_rate(min)),
            ]
            .into_iter()
            .flatten()
        })
        .collect()
}

fn open_config(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    opening: &Opening<'_>,
) -> Result<OpenedOutput, OpenError> {
    match config.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(device, config, opening),
        cpal::SampleFormat::I16 => build::<i16>(device, config, opening),
        cpal::SampleFormat::U16 => build::<u16>(device, config, opening),
        cpal::SampleFormat::I32 => build::<i32>(device, config, opening),
        cpal::SampleFormat::F64 => build::<f64>(device, config, opening),
        other @ (cpal::SampleFormat::I8
        | cpal::SampleFormat::I24
        | cpal::SampleFormat::I64
        | cpal::SampleFormat::U8
        | cpal::SampleFormat::U32
        | cpal::SampleFormat::U64)
        | other => Err(OpenError::UnsupportedFormat(other)),
    }
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    opening: &Opening<'_>,
) -> Result<OpenedOutput, OpenError> {
    let format = OutputFormat {
        channels: config.channels(),
        rate: config.sample_rate().0,
    };
    let MixerChannel {
        mut mixer,
        control,
        retired_voices,
    } = mixer_channel(format, opening.speed, opening.spectrum_buffers);
    let stream = device
        .build_output_stream::<T, _, _>(
            &config.config(),
            move |out: &mut [T], _: &cpal::OutputCallbackInfo| mixer.mix(out),
            output_loss_callback(opening.callback_sender, opening.output_loss),
            None,
        )
        .map_err(OpenError::Build)?;
    stream.play().map_err(OpenError::Play)?;
    Ok(OpenedOutput {
        stream,
        format,
        mixer_control: control,
        retired_voices,
        device_name: readable(device.name()),
    })
}

fn find_by_name(name: &DeviceName) -> Result<Option<cpal::Device>, cpal::DevicesError> {
    Ok(cpal::default_host()
        .output_devices()?
        .find(|device| readable(device.name()).as_ref() == Some(name)))
}

#[cfg(test)]
mod tests {
    use cpal::traits::HostTrait;
    use crossbeam_channel::TryRecvError;
    use kernel::domain::{
        device::{DeviceDefault, DeviceName, ListedDevice, OutputDevice},
        speed::Speed,
        transport::OutputError,
    };
    use rstest::rstest;

    use crate::{
        deck::event::DeckEvent,
        device::{
            Opening,
            OutputLoss,
            latched,
            list_output_devices,
            listed,
            loss_code,
            open_output,
            pick_config,
        },
        engine::message::AudioMessage,
        error::DeviceError,
        tap::spectrum_channel,
    };

    fn range(
        channels: u16,
        rates: std::ops::RangeInclusive<u32>,
        sample_format: cpal::SampleFormat,
    ) -> cpal::SupportedStreamConfigRange {
        cpal::SupportedStreamConfigRange::new(
            channels,
            cpal::SampleRate(*rates.start()),
            cpal::SampleRate(*rates.end()),
            cpal::SupportedBufferSize::Unknown,
            sample_format,
        )
    }

    #[rstest]
    #[case::no_configs(Vec::new(), Vec::new())]
    #[case::a_wide_range_tries_its_top_the_cd_rate_and_its_bottom(
        vec![range(2, 8_000..=96_000, cpal::SampleFormat::F32)],
        vec![
            (2, 96_000, cpal::SampleFormat::F32),
            (2, 44_100, cpal::SampleFormat::F32),
            (2, 8_000, cpal::SampleFormat::F32),
        ]
    )]
    #[case::a_range_topping_at_the_cd_rate_tries_it_once(
        vec![range(2, 8_000..=44_100, cpal::SampleFormat::F32)],
        vec![
            (2, 44_100, cpal::SampleFormat::F32),
            (2, 8_000, cpal::SampleFormat::F32),
        ]
    )]
    #[case::a_fixed_rate_is_tried_once(
        vec![range(2, 48_000..=48_000, cpal::SampleFormat::I16)],
        vec![(2, 48_000, cpal::SampleFormat::I16)]
    )]
    #[case::stereo_goes_before_mono(
        vec![
            range(1, 48_000..=48_000, cpal::SampleFormat::F32),
            range(2, 44_100..=48_000, cpal::SampleFormat::I16),
        ],
        vec![
            (2, 48_000, cpal::SampleFormat::I16),
            (2, 44_100, cpal::SampleFormat::I16),
            (1, 48_000, cpal::SampleFormat::F32),
        ]
    )]
    #[case::float_goes_before_integer_at_equal_channels(
        vec![
            range(2, 48_000..=48_000, cpal::SampleFormat::I16),
            range(2, 48_000..=48_000, cpal::SampleFormat::F32),
        ],
        vec![
            (2, 48_000, cpal::SampleFormat::F32),
            (2, 48_000, cpal::SampleFormat::I16),
        ]
    )]
    fn pick_config_orders_the_fallback_configs(
        #[case] supported: Vec<cpal::SupportedStreamConfigRange>,
        #[case] expected: Vec<(u16, u32, cpal::SampleFormat)>,
    ) {
        let picked: Vec<(u16, u32, cpal::SampleFormat)> = pick_config(supported)
            .iter()
            .map(|config| {
                (
                    config.channels(),
                    config.sample_rate().0,
                    config.sample_format(),
                )
            })
            .collect();
        assert_eq!(picked, expected);
    }

    fn no_output_device_available() -> bool {
        cpal::default_host().default_output_device().is_none()
    }

    fn name(text: &str) -> DeviceName {
        DeviceName::new(text.to_owned()).unwrap()
    }

    fn unreadable() -> Result<String, cpal::DeviceNameError> {
        Err(cpal::DeviceNameError::BackendSpecific {
            err: cpal::BackendSpecificError {
                description: "unreadable".to_owned(),
            },
        })
    }

    #[rstest]
    #[case::unreadable_names_are_skipped(
        vec![Ok("A".to_owned()), unreadable(), Ok("B".to_owned())],
        vec![("A", DeviceDefault::No), ("B", DeviceDefault::Yes)]
    )]
    #[case::empty_names_are_skipped(
        vec![Ok(String::new()), Ok("B".to_owned())],
        vec![("B", DeviceDefault::Yes)]
    )]
    fn listing_keeps_only_readable_names(
        #[case] names: Vec<Result<String, cpal::DeviceNameError>>,
        #[case] expected: Vec<(&str, DeviceDefault)>,
    ) {
        let expected: Vec<ListedDevice> = expected
            .into_iter()
            .map(|(text, default)| ListedDevice {
                name: name(text),
                default,
            })
            .collect();
        assert_eq!(listed(names.into_iter(), Some(&name("B"))), expected);
    }

    #[rstest]
    #[case::device_gone(
        loss_code(OutputError::DeviceGone),
        Some(OutputError::DeviceGone)
    )]
    #[case::backend(loss_code(OutputError::Backend), Some(OutputError::Backend))]
    #[case::clear(0, None)]
    #[case::unknown(7, None)]
    fn a_loss_code_reads_back_as_its_latched_error(
        #[case] code: u8,
        #[case] expected: Option<OutputError>,
    ) {
        assert_eq!(latched(code), expected);
    }

    #[test]
    fn a_loss_on_an_open_callback_channel_is_sent_at_once_and_not_latched() {
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(1);
        let output_loss = OutputLoss::default();
        output_loss.report(OutputError::Backend, &callback_sender);
        assert!(matches!(
            callback_receiver.try_recv(),
            Ok(AudioMessage::Deck(DeckEvent::OutputLost(
                OutputError::Backend
            )))
        ));
        output_loss.resend(&callback_sender);
        assert!(matches!(
            callback_receiver.try_recv(),
            Err(TryRecvError::Empty)
        ));
    }

    #[test]
    fn a_loss_on_a_closed_callback_channel_is_not_latched() {
        let (closed_callback_sender, closed_callback_receiver) =
            crossbeam_channel::bounded(1);
        drop(closed_callback_receiver);
        let output_loss = OutputLoss::default();
        output_loss.report(OutputError::DeviceGone, &closed_callback_sender);
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(1);
        output_loss.resend(&callback_sender);
        assert!(matches!(
            callback_receiver.try_recv(),
            Err(TryRecvError::Empty)
        ));
    }

    #[test]
    fn a_loss_dropped_on_a_full_callback_channel_is_resent_later() {
        let (callback_sender, callback_receiver) = crossbeam_channel::bounded(1);
        let output_loss = OutputLoss::default();
        output_loss.report(OutputError::DeviceGone, &callback_sender);
        output_loss.report(OutputError::Backend, &callback_sender);
        assert!(matches!(
            callback_receiver.try_recv(),
            Ok(AudioMessage::Deck(DeckEvent::OutputLost(
                OutputError::DeviceGone
            )))
        ));
        output_loss.resend(&callback_sender);
        assert!(matches!(
            callback_receiver.try_recv(),
            Ok(AudioMessage::Deck(DeckEvent::OutputLost(
                OutputError::Backend
            )))
        ));
        output_loss.resend(&callback_sender);
        assert!(matches!(
            callback_receiver.try_recv(),
            Err(TryRecvError::Empty)
        ));
    }

    #[test]
    #[ignore = "hardware: enumerates real audio devices; run with --include-ignored"]
    fn list_output_devices_reports_at_most_one_default() {
        let devices = list_output_devices().unwrap();
        let default_count = devices
            .iter()
            .filter(|device| matches!(device.default, DeviceDefault::Yes))
            .count();
        assert!(default_count <= 1);
    }

    #[test]
    #[ignore = "hardware: needs a real audio device; run with --include-ignored"]
    fn open_output_with_unknown_name_reports_not_found_and_default_still_opens() {
        if no_output_device_available() {
            eprintln!("skipping: no default output device in this environment");
            return;
        }

        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(1);
        let output_loss = OutputLoss::default();
        let (spectrum_buffers, _spectrum_tap) = spectrum_channel();
        let opening = Opening {
            speed: Speed::default(),
            spectrum_buffers: &spectrum_buffers,
            callback_sender: &callback_sender,
            output_loss: &output_loss,
        };
        let unknown_device = OutputDevice::Named(name("no-such-device-xyz"));
        let refusal = open_output(&unknown_device, &opening).err();
        assert!(matches!(refusal, Some(DeviceError::NotFound(_))));
        assert_eq!(
            refusal.map(|refusal| refusal.to_string()),
            Some("audio device 'no-such-device-xyz' not found".to_owned())
        );

        assert!(open_output(&OutputDevice::SystemDefault, &opening).is_ok());
    }
}
