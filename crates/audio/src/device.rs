use crossbeam_channel::{Receiver, Sender};
use kernel::{
    AudioFailure,
    domain::{DeviceDefault, OutputDevice},
};
use rodio::{
    cpal,
    cpal::traits::{DeviceTrait, HostTrait},
};

use crate::error::DeviceError;

#[must_use]
pub(crate) fn list_output_devices() -> Vec<OutputDevice> {
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|device| device.name().ok());
    host.output_devices()
        .map(|devices| {
            devices
                .filter_map(|device| device.name().ok())
                .map(|name| {
                    let default = if default_name.as_deref() == Some(name.as_str()) {
                        DeviceDefault::Default
                    } else {
                        DeviceDefault::Named
                    };
                    OutputDevice { name, default }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug)]
pub(crate) struct StreamFaults {
    raised: Sender<AudioFailure>,
    heard: Receiver<AudioFailure>,
}

impl Default for StreamFaults {
    fn default() -> Self {
        let (raised, heard) = crossbeam_channel::bounded(1);
        Self { raised, heard }
    }
}

impl StreamFaults {
    #[must_use]
    pub(crate) fn raised(&self) -> &Sender<AudioFailure> {
        &self.raised
    }

    #[must_use]
    pub(crate) fn take(&self) -> Option<AudioFailure> {
        self.heard.try_recv().ok()
    }
}

fn report(
    raised: &Sender<AudioFailure>,
) -> impl FnMut(cpal::StreamError) + Clone + Send + 'static {
    let raised = raised.clone();
    move |error: cpal::StreamError| {
        let _ = raised.try_send(AudioFailure::OutputLost {
            reason: error.to_string(),
        });
    }
}

pub(crate) fn open_stream(
    device: Option<&str>,
    raised: &Sender<AudioFailure>,
) -> Result<rodio::OutputStream, DeviceError> {
    device.map_or_else(|| open_default(raised), |name| open_named(name, raised))
}

fn open_named(
    name: &str,
    raised: &Sender<AudioFailure>,
) -> Result<rodio::OutputStream, DeviceError> {
    let Some(device) = find_by_name(name) else {
        return Err(DeviceError::NotFound {
            name: name.to_string(),
        });
    };
    rodio::OutputStreamBuilder::from_device(device)
        .map(|builder| builder.with_error_callback(report(raised)))
        .and_then(rodio::OutputStreamBuilder::open_stream)
        .map(silence_drop_log)
        .map_err(|source| DeviceError::NoDevice {
            name: Some(name.to_string()),
            source,
        })
}

fn open_default(
    raised: &Sender<AudioFailure>,
) -> Result<rodio::OutputStream, DeviceError> {
    rodio::OutputStreamBuilder::from_default_device()
        .map(|builder| builder.with_error_callback(report(raised)))
        .and_then(|builder| builder.open_stream_or_fallback())
        .map(silence_drop_log)
        .map_err(|source| DeviceError::NoDevice { name: None, source })
}

fn silence_drop_log(mut stream: rodio::OutputStream) -> rodio::OutputStream {
    stream.log_on_drop(false);
    stream
}

fn find_by_name(name: &str) -> Option<cpal::Device> {
    cpal::default_host()
        .output_devices()
        .ok()?
        .find(|device| device.name().ok().as_deref() == Some(name))
}

#[cfg(test)]
mod tests {
    use kernel::domain::DeviceDefault;
    use rodio::{cpal, cpal::traits::HostTrait};

    use crate::{
        device::{StreamFaults, list_output_devices, open_stream},
        error::DeviceError,
    };

    fn no_output_device_available() -> bool {
        cpal::default_host().default_output_device().is_none()
    }

    #[test]
    #[ignore = "hardware: enumerates real audio devices; run with --include-ignored"]
    fn list_output_devices_reports_at_most_one_default() {
        let devices = list_output_devices();
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

        let faults = StreamFaults::default();
        let refusal = open_stream(Some("no-such-device-xyz"), faults.raised()).err();
        assert!(matches!(refusal, Some(DeviceError::NotFound { .. })));
        assert_eq!(
            refusal.map(|refusal| refusal.to_string()),
            Some(
                "audio device 'no-such-device-xyz' not found, using default".to_owned()
            )
        );

        assert!(open_stream(None, faults.raised()).is_ok());
    }
}
