use crossbeam_channel::Sender;
use kernel::domain::{DeviceDefault, DeviceName, OutputDevice};
use rodio::{
    cpal,
    cpal::traits::{DeviceTrait, HostTrait},
};

use crate::{deck::DeckEvent, error::DeviceError};

pub(crate) fn list_output_devices() -> Result<Vec<OutputDevice>, DeviceError> {
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|device| device.name().ok());
    let devices = host.output_devices().map_err(DeviceError::Unlisted)?;
    Ok(devices
        .filter_map(|device| device.name().ok())
        .filter_map(|name| {
            let default = if default_name.as_deref() == Some(name.as_str()) {
                DeviceDefault::Default
            } else {
                DeviceDefault::Named
            };
            DeviceName::new(name)
                .ok()
                .map(|name| OutputDevice { name, default })
        })
        .collect())
}

fn report(
    wake: &Sender<DeckEvent>,
) -> impl FnMut(cpal::StreamError) + Clone + Send + 'static {
    let wake = wake.clone();
    move |error: cpal::StreamError| {
        let _ = wake.try_send(DeckEvent::Fault(error));
    }
}

pub(crate) fn open_stream(
    device: Option<&DeviceName>,
    wake: &Sender<DeckEvent>,
) -> Result<rodio::OutputStream, DeviceError> {
    device.map_or_else(|| open_default(wake), |name| open_named(name, wake))
}

fn open_named(
    name: &DeviceName,
    wake: &Sender<DeckEvent>,
) -> Result<rodio::OutputStream, DeviceError> {
    let Some(device) = find_by_name(name) else {
        return Err(DeviceError::NotFound { name: name.clone() });
    };
    rodio::OutputStreamBuilder::from_device(device)
        .map(|builder| builder.with_error_callback(report(wake)))
        .and_then(rodio::OutputStreamBuilder::open_stream)
        .map(silence_drop_log)
        .map_err(|source| DeviceError::NoDevice {
            name: Some(name.clone()),
            source,
        })
}

fn open_default(wake: &Sender<DeckEvent>) -> Result<rodio::OutputStream, DeviceError> {
    rodio::OutputStreamBuilder::from_default_device()
        .map(|builder| builder.with_error_callback(report(wake)))
        .and_then(|builder| builder.open_stream_or_fallback())
        .map(silence_drop_log)
        .map_err(|source| DeviceError::NoDevice { name: None, source })
}

fn silence_drop_log(mut stream: rodio::OutputStream) -> rodio::OutputStream {
    stream.log_on_drop(false);
    stream
}

fn find_by_name(name: &DeviceName) -> Option<cpal::Device> {
    cpal::default_host()
        .output_devices()
        .ok()?
        .find(|device| device.name().ok().as_deref() == Some(name.as_str()))
}

#[cfg(test)]
mod tests {
    use kernel::domain::{DeviceDefault, DeviceName};
    use rodio::{cpal, cpal::traits::HostTrait};

    use crate::{
        device::{list_output_devices, open_stream},
        error::DeviceError,
    };

    fn no_output_device_available() -> bool {
        cpal::default_host().default_output_device().is_none()
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

        let (wake, _heard) = crossbeam_channel::bounded(1);
        let name = DeviceName::new("no-such-device-xyz".to_string()).unwrap();
        let refusal = open_stream(Some(&name), &wake).err();
        assert!(matches!(refusal, Some(DeviceError::NotFound { .. })));
        assert_eq!(
            refusal.map(|refusal| refusal.to_string()),
            Some(
                "audio device 'no-such-device-xyz' not found, using default".to_owned()
            )
        );

        assert!(open_stream(None, &wake).is_ok());
    }
}
