use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use crossbeam_channel::{Sender, TrySendError};
use kernel::domain::{
    device::{DeviceDefault, DeviceName, ListedDevice, OutputDevice},
    transport::OutputError,
};
use rodio::{
    cpal,
    cpal::traits::{DeviceTrait, HostTrait},
};

use crate::{
    deck::event::DeckEvent,
    engine::message::AudioMessage,
    error::{DeviceError, output_error},
};

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

pub(crate) fn open_stream(
    device: &OutputDevice,
    callback_sender: &Sender<AudioMessage>,
    output_loss: &OutputLoss,
) -> Result<rodio::OutputStream, DeviceError> {
    let callback = output_loss_callback(callback_sender, output_loss);
    match device {
        OutputDevice::Named(name) => open_named(name, callback),
        OutputDevice::SystemDefault => open_default(callback),
    }
}

fn open_named(
    name: &DeviceName,
    callback: impl FnMut(cpal::StreamError) + Clone + Send + 'static,
) -> Result<rodio::OutputStream, DeviceError> {
    let device = find_by_name(name)
        .map_err(|source| DeviceError::Lookup {
            requested_device: OutputDevice::Named(name.clone()),
            source,
        })?
        .ok_or_else(|| DeviceError::NotFound(name.clone()))?;
    rodio::OutputStreamBuilder::from_device(device)
        .map(|builder| builder.with_error_callback(callback))
        .and_then(rodio::OutputStreamBuilder::open_stream)
        .map(silence_drop_log)
        .map_err(|source| DeviceError::NoDevice {
            requested_device: OutputDevice::Named(name.clone()),
            source,
        })
}

fn open_default(
    callback: impl FnMut(cpal::StreamError) + Clone + Send + 'static,
) -> Result<rodio::OutputStream, DeviceError> {
    rodio::OutputStreamBuilder::from_default_device()
        .map(|builder| builder.with_error_callback(callback))
        .and_then(|builder| builder.open_stream_or_fallback())
        .map(silence_drop_log)
        .map_err(|source| DeviceError::NoDevice {
            requested_device: OutputDevice::SystemDefault,
            source,
        })
}

fn silence_drop_log(mut stream: rodio::OutputStream) -> rodio::OutputStream {
    stream.log_on_drop(false);
    stream
}

fn find_by_name(name: &DeviceName) -> Result<Option<cpal::Device>, cpal::DevicesError> {
    Ok(cpal::default_host()
        .output_devices()?
        .find(|device| readable(device.name()).as_ref() == Some(name)))
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::TryRecvError;
    use kernel::domain::{
        device::{DeviceDefault, DeviceName, ListedDevice, OutputDevice},
        transport::OutputError,
    };
    use rodio::{cpal, cpal::traits::HostTrait};
    use rstest::rstest;

    use crate::{
        deck::event::DeckEvent,
        device::{
            OutputLoss,
            latched,
            list_output_devices,
            listed,
            loss_code,
            open_stream,
            readable,
        },
        engine::message::AudioMessage,
        error::DeviceError,
    };

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
    #[case::device_gone(OutputError::DeviceGone)]
    #[case::backend(OutputError::Backend)]
    fn a_latched_loss_code_reads_back_as_its_output_error(#[case] error: OutputError) {
        assert_eq!(latched(loss_code(error)), Some(error));
    }

    #[rstest]
    #[case::clear(0)]
    #[case::unknown(7)]
    fn an_unlatched_code_reads_back_as_nothing(#[case] code: u8) {
        assert_eq!(latched(code), None);
    }

    #[rstest]
    #[case::a_plain_name(Ok("Speakers".to_owned()), Some(name("Speakers")))]
    #[case::an_empty_name(Ok(String::new()), None)]
    #[case::an_unreadable_name(unreadable(), None)]
    fn readable_keeps_only_valid_names(
        #[case] text: Result<String, cpal::DeviceNameError>,
        #[case] expected: Option<DeviceName>,
    ) {
        assert_eq!(readable(text), expected);
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
    fn open_stream_with_unknown_name_reports_not_found_and_default_still_opens() {
        if no_output_device_available() {
            eprintln!("skipping: no default output device in this environment");
            return;
        }

        let (callback_sender, _callback_receiver) = crossbeam_channel::bounded(1);
        let output_loss = OutputLoss::default();
        let unknown_device = OutputDevice::Named(name("no-such-device-xyz"));
        let refusal =
            open_stream(&unknown_device, &callback_sender, &output_loss).err();
        assert!(matches!(refusal, Some(DeviceError::NotFound(_))));
        assert_eq!(
            refusal.map(|refusal| refusal.to_string()),
            Some("audio device 'no-such-device-xyz' not found".to_owned())
        );

        assert!(
            open_stream(&OutputDevice::SystemDefault, &callback_sender, &output_loss)
                .is_ok()
        );
    }
}
