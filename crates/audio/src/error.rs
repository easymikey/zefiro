use std::path::PathBuf;

use kernel::AudioFailure;

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("audio device '{name}' not found, using default")]
    NotFound { name: String },
    #[error("no output device available: {source}")]
    NoDevice {
        name: Option<String>,
        #[source]
        source: rodio::StreamError,
    },
}

impl DeviceError {
    fn requested(&self) -> String {
        match self {
            DeviceError::NotFound { name } => name.clone(),
            DeviceError::NoDevice { name, .. } => {
                name.clone().unwrap_or_else(|| "default".to_owned())
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("cannot decode {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: rodio::decoder::DecoderError,
    },
    #[error("cannot start a decode thread: {0}")]
    Spawn(#[source] std::io::Error),
    #[error(transparent)]
    Device(#[from] DeviceError),
}

impl From<&AudioError> for AudioFailure {
    fn from(error: &AudioError) -> Self {
        match error {
            AudioError::Decode { path, source } => AudioFailure::Decode {
                path: path.clone(),
                reason: source.to_string(),
            },
            AudioError::Spawn(source) => AudioFailure::Stream {
                reason: source.to_string(),
            },
            AudioError::Device(source) => AudioFailure::Device {
                requested: source.requested(),
            },
        }
    }
}

pub(crate) fn device_fault(error: DeviceError) -> AudioFailure {
    AudioFailure::from(&AudioError::from(error))
}

pub(crate) fn preload_fault(error: &AudioError) -> AudioFailure {
    match AudioFailure::from(error) {
        AudioFailure::Decode { path, reason } => AudioFailure::Preload { path, reason },
        other @ (AudioFailure::Device { .. }
        | AudioFailure::Stream { .. }
        | AudioFailure::OutputLost { .. }
        | AudioFailure::Preload { .. }
        | AudioFailure::Seek { .. }) => other,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rstest::rstest;

    use crate::error::{AudioError, DeviceError};

    #[rstest]
    #[case::decode(
        AudioError::Decode {
            path: PathBuf::from("/music/track.flac"),
            source: rodio::decoder::DecoderError::UnrecognizedFormat,
        },
        "cannot decode /music/track.flac: Unrecognized format"
    )]
    #[case::spawn(
        AudioError::Spawn(std::io::Error::other("thread limit reached")),
        "cannot start a decode thread: thread limit reached"
    )]
    #[case::device(
        AudioError::from(DeviceError::NotFound { name: "usb-dac".into() }),
        "audio device 'usb-dac' not found, using default"
    )]
    fn audio_error_display_carries_its_source(
        #[case] error: AudioError,
        #[case] expected: &str,
    ) {
        assert_eq!(error.to_string(), expected);
    }
}
