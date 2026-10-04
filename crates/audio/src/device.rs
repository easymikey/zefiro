use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use crossbeam_channel::{Sender, TrySendError};
use kernel::domain::{
    device::{DeviceDefault, DeviceName, ListedDevice, OutputDevice},
    transport::StreamError,
};
use rodio::{
    cpal,
    cpal::traits::{DeviceTrait, HostTrait},
};

use crate::{
    deck::event::DeckEvent,
    engine::message::AudioMessage,
    error::{DeviceError, stream_error},
};

const CLEAR: u8 = 0;
const DEVICE_GONE: u8 = 1;
const BACKEND: u8 = 2;

#[derive(Debug, Clone, Default)]
pub(crate) struct OutputLoss(Arc<AtomicU8>);

impl OutputLoss {
    pub(crate) fn report(&self, kind: StreamError, sender: &Sender<AudioMessage>) {
        match sender.try_send(AudioMessage::Deck(DeckEvent::OutputLost(kind))) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                self.0.store(loss_code(kind), Ordering::Release);
            }
        }
    }

    pub(crate) fn resend(&self, sender: &Sender<AudioMessage>) {
        if let Some(kind) = latched(self.0.swap(CLEAR, Ordering::AcqRel)) {
            self.report(kind, sender);
        }
    }
}

fn loss_code(kind: StreamError) -> u8 {
    match kind {
        StreamError::DeviceGone => DEVICE_GONE,
        StreamError::Backend => BACKEND,
    }
}

fn latched(code: u8) -> Option<StreamError> {
    match code {
        DEVICE_GONE => Some(StreamError::DeviceGone),
        BACKEND => Some(StreamError::Backend),
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
                DeviceDefault::Default
            } else {
                DeviceDefault::Named
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

fn stream_error_callback(
    sender: &Sender<AudioMessage>,
    lost: &OutputLoss,
) -> impl FnMut(cpal::StreamError) + Clone + Send + 'static {
    let (sender, lost) = (sender.clone(), lost.clone());
    move |error: cpal::StreamError| lost.report(stream_error(&error), &sender)
}

pub(crate) fn open_stream(
    device: &OutputDevice,
    sender: &Sender<AudioMessage>,
    lost: &OutputLoss,
) -> Result<rodio::OutputStream, DeviceError> {
    let callback = stream_error_callback(sender, lost);
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
        .map_err(DeviceError::ListDevices)?
        .ok_or_else(|| DeviceError::NotFound(name.clone()))?;
    rodio::OutputStreamBuilder::from_device(device)
        .map(|builder| builder.with_error_callback(callback))
        .and_then(rodio::OutputStreamBuilder::open_stream)
        .map(silence_drop_log)
        .map_err(|source| DeviceError::NoDevice {
            name: OutputDevice::Named(name.clone()),
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
            name: OutputDevice::SystemDefault,
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
        transport::StreamError,
    };
    use rodio::{cpal, cpal::traits::HostTrait};
    use rstest::rstest;

    use crate::{
        deck::event::DeckEvent,
        device::{OutputLoss, list_output_devices, listed, open_stream},
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
        vec![("A", DeviceDefault::Named), ("B", DeviceDefault::Default)]
    )]
    #[case::empty_names_are_skipped(
        vec![Ok(String::new()), Ok("B".to_owned())],
        vec![("B", DeviceDefault::Default)]
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

    #[test]
    fn a_loss_dropped_on_a_full_inbox_is_resent_later() {
        let (sender, heard) = crossbeam_channel::bounded(1);
        let lost = OutputLoss::default();
        lost.report(StreamError::DeviceGone, &sender);
        lost.report(StreamError::Backend, &sender);
        assert!(matches!(
            heard.try_recv(),
            Ok(AudioMessage::Deck(DeckEvent::OutputLost(
                StreamError::DeviceGone
            )))
        ));
        lost.resend(&sender);
        assert!(matches!(
            heard.try_recv(),
            Ok(AudioMessage::Deck(DeckEvent::OutputLost(
                StreamError::Backend
            )))
        ));
        lost.resend(&sender);
        assert!(matches!(heard.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    #[ignore = "hardware: enumerates real audio devices; run with --include-ignored"]
    fn list_output_devices_reports_at_most_one_default() {
        let devices = list_output_devices().unwrap();
        let default_count = devices
            .iter()
            .filter(|device| matches!(device.default, DeviceDefault::Default))
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

        let (sender, _heard) = crossbeam_channel::bounded(1);
        let lost = OutputLoss::default();
        let unknown = OutputDevice::Named(name("no-such-device-xyz"));
        let refusal = open_stream(&unknown, &sender, &lost).err();
        assert!(matches!(refusal, Some(DeviceError::NotFound(_))));
        assert_eq!(
            refusal.map(|refusal| refusal.to_string()),
            Some(
                "audio device 'no-such-device-xyz' not found, using default".to_owned()
            )
        );

        assert!(open_stream(&OutputDevice::SystemDefault, &sender, &lost).is_ok());
    }
}
