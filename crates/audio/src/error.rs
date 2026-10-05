use std::path::PathBuf;

use kernel::{
    domain::{
        config::Diagnostic,
        device::{DeviceName, OutputDevice},
        io_error::IoError,
        transport::StreamError,
    },
    message::{AudioError, DecodeError},
};
use rodio::cpal;

#[derive(Debug, thiserror::Error)]
pub(crate) enum DeviceError {
    #[error("audio device '{0}' not found, using default")]
    NotFound(DeviceName),
    #[error("no output device available: {source}")]
    NoDevice {
        name: OutputDevice,
        #[source]
        source: rodio::StreamError,
    },
    #[error("cannot list output devices: {0}")]
    ListDevices(#[source] cpal::DevicesError),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot open {}: {source}", path.display())]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot decode {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: rodio::decoder::DecoderError,
    },
    #[error("the decode worker panicked on {0}")]
    WorkerPanicked(PathBuf),
}

fn decode_error(source: &rodio::decoder::DecoderError) -> DecodeError {
    match source {
        rodio::decoder::DecoderError::UnrecognizedFormat => DecodeError::Unsupported,
        rodio::decoder::DecoderError::IoError(_) => {
            DecodeError::Unreadable(IoError::Other)
        }
        rodio::decoder::DecoderError::DecodeError(_)
        | rodio::decoder::DecoderError::LimitError(_)
        | rodio::decoder::DecoderError::ResetRequired
        | rodio::decoder::DecoderError::NoStreams => DecodeError::Corrupt,
    }
}

impl From<&Error> for AudioError {
    fn from(error: &Error) -> Self {
        match error {
            Error::Open { path, source } => AudioError::Decode {
                path: path.clone(),
                kind: DecodeError::Unreadable(source.kind().into()),
            },
            Error::Decode { path, source } => AudioError::Decode {
                path: path.clone(),
                kind: decode_error(source),
            },
            Error::WorkerPanicked(path) => AudioError::Decode {
                path: path.clone(),
                kind: DecodeError::Panicked,
            },
        }
    }
}

pub(crate) fn device_error(error: DeviceError) -> AudioError {
    match error {
        DeviceError::NotFound(name) => AudioError::Device {
            requested: OutputDevice::Named(name),
        },
        DeviceError::NoDevice { name, .. } => AudioError::Device { requested: name },
        DeviceError::ListDevices(source) => list_devices_error(&source),
    }
}

pub(crate) fn list_devices_error(error: &cpal::DevicesError) -> AudioError {
    AudioError::ListDevices {
        reason: Diagnostic::from_error(error),
    }
}

pub(crate) fn stream_error(error: &cpal::StreamError) -> StreamError {
    match error {
        cpal::StreamError::DeviceNotAvailable => StreamError::DeviceGone,
        cpal::StreamError::BackendSpecific { .. } => StreamError::Backend,
    }
}

pub(crate) fn seek_error(error: &rodio::source::SeekError) -> AudioError {
    AudioError::Seek {
        reason: Diagnostic::from_error(error),
    }
}

pub(crate) fn preload_error(error: &Error) -> AudioError {
    match AudioError::from(error) {
        AudioError::Decode { path, kind } => AudioError::Preload { path, kind },
        other @ (AudioError::Device { .. }
        | AudioError::ListDevices { .. }
        | AudioError::Stream { .. }
        | AudioError::OutputLost(..)
        | AudioError::Preload { .. }
        | AudioError::Seek { .. }) => other,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{
        domain::{
            config::Diagnostic,
            device::{DeviceName, OutputDevice},
            io_error::IoError,
            transport::StreamError,
        },
        message::{AudioError, DecodeError},
    };
    use rstest::rstest;

    use crate::error::{DeviceError, Error, device_error, preload_error, stream_error};

    fn device_name(name: &str) -> DeviceName {
        DeviceName::new(name.to_string()).unwrap()
    }

    #[rstest]
    #[case::open_not_found(
        Error::Open {
            path: PathBuf::from("/music/track.flac"),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        "cannot open /music/track.flac: entity not found"
    )]
    #[case::decode(
        Error::Decode {
            path: PathBuf::from("/music/track.flac"),
            source: rodio::decoder::DecoderError::UnrecognizedFormat,
        },
        "cannot decode /music/track.flac: Unrecognized format"
    )]
    #[case::worker_panicked(
        Error::WorkerPanicked(PathBuf::from("/music/track.flac")),
        "the decode worker panicked on /music/track.flac"
    )]
    fn audio_error_display_carries_its_source(
        #[case] error: Error,
        #[case] expected: &str,
    ) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::unrecognized_format(
        rodio::decoder::DecoderError::UnrecognizedFormat,
        DecodeError::Unsupported
    )]
    #[case::io_error(
        rodio::decoder::DecoderError::IoError("broken pipe".to_owned()),
        DecodeError::Unreadable(IoError::Other)
    )]
    #[case::decode_error(
        rodio::decoder::DecoderError::DecodeError("bad frame"),
        DecodeError::Corrupt
    )]
    #[case::limit_error(
        rodio::decoder::DecoderError::LimitError("too large"),
        DecodeError::Corrupt
    )]
    #[case::reset_required(
        rodio::decoder::DecoderError::ResetRequired,
        DecodeError::Corrupt
    )]
    #[case::no_streams(rodio::decoder::DecoderError::NoStreams, DecodeError::Corrupt)]
    fn a_decode_error_maps_to_its_audio_error(
        #[case] source: rodio::decoder::DecoderError,
        #[case] expected: DecodeError,
    ) {
        let error = Error::Decode {
            path: PathBuf::from("/music/track.flac"),
            source,
        };
        let failure = AudioError::from(&error);
        assert_eq!(
            failure,
            AudioError::Decode {
                path: PathBuf::from("/music/track.flac"),
                kind: expected,
            }
        );
    }

    #[rstest]
    #[case::open_not_found(
        Error::Open {
            path: PathBuf::from("/a"),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        },
        AudioError::Decode {
            path: PathBuf::from("/a"),
            kind: DecodeError::Unreadable(IoError::Missing),
        }
    )]
    #[case::worker_panicked(
        Error::WorkerPanicked(PathBuf::from("/a")),
        AudioError::Decode { path: PathBuf::from("/a"), kind: DecodeError::Panicked }
    )]
    fn an_error_becomes_an_audio_error_with_its_path(
        #[case] error: Error,
        #[case] expected: AudioError,
    ) {
        assert_eq!(AudioError::from(&error), expected);
    }

    #[test]
    fn a_device_error_names_the_requested_device() {
        let error = DeviceError::NotFound(device_name("usb"));
        assert_eq!(
            device_error(error),
            AudioError::Device {
                requested: OutputDevice::Named(device_name("usb"))
            }
        );
    }

    fn backend_failure() -> rodio::cpal::BackendSpecificError {
        rodio::cpal::BackendSpecificError {
            description: "host gone".to_owned(),
        }
    }

    #[rstest]
    #[case::no_named_device(
        DeviceError::NoDevice {
            name: OutputDevice::Named(device_name("usb")),
            source: rodio::StreamError::NoDevice,
        },
        AudioError::Device { requested: OutputDevice::Named(device_name("usb")) }
    )]
    #[case::no_default_device(
        DeviceError::NoDevice {
            name: OutputDevice::SystemDefault,
            source: rodio::StreamError::NoDevice,
        },
        AudioError::Device { requested: OutputDevice::SystemDefault }
    )]
    #[case::list_devices(
        DeviceError::ListDevices(rodio::cpal::DevicesError::BackendSpecific {
            err: backend_failure(),
        }),
        AudioError::ListDevices {
            reason: Diagnostic::from_error(&rodio::cpal::DevicesError::BackendSpecific {
                err: backend_failure(),
            }),
        }
    )]
    fn every_device_error_maps_to_its_audio_error(
        #[case] error: DeviceError,
        #[case] expected: AudioError,
    ) {
        assert_eq!(device_error(error), expected);
    }

    #[rstest]
    #[case::open_denied(
        Error::Open {
            path: PathBuf::from("/a"),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        },
        AudioError::Preload {
            path: PathBuf::from("/a"),
            kind: DecodeError::Unreadable(IoError::Denied),
        }
    )]
    #[case::decode_unsupported(
        Error::Decode {
            path: PathBuf::from("/a"),
            source: rodio::decoder::DecoderError::UnrecognizedFormat,
        },
        AudioError::Preload { path: PathBuf::from("/a"), kind: DecodeError::Unsupported }
    )]
    #[case::worker_panicked(
        Error::WorkerPanicked(PathBuf::from("/a")),
        AudioError::Preload { path: PathBuf::from("/a"), kind: DecodeError::Panicked }
    )]
    fn a_preload_error_keeps_its_kind_under_preload(
        #[case] error: Error,
        #[case] expected: AudioError,
    ) {
        assert_eq!(preload_error(&error), expected);
    }

    #[rstest]
    #[case::device_not_available(
        rodio::cpal::StreamError::DeviceNotAvailable,
        StreamError::DeviceGone
    )]
    #[case::backend_specific(
        rodio::cpal::StreamError::BackendSpecific {
            err: rodio::cpal::BackendSpecificError {
                description: "underrun".to_string(),
            },
        },
        StreamError::Backend
    )]
    fn a_stream_error_maps_to_its_kind(
        #[case] error: rodio::cpal::StreamError,
        #[case] expected: StreamError,
    ) {
        assert_eq!(stream_error(&error), expected);
    }
}
