use std::path::PathBuf;

use kernel::{
    domain::{
        config::Diagnostic,
        device::{DeviceName, OutputDevice},
        io_error::IoError,
        transport::OutputError,
    },
    message::{AudioError, DecodeError},
};
use rodio::cpal;

#[derive(Debug, thiserror::Error)]
pub(crate) enum DeviceError {
    #[error("audio device '{0}' not found")]
    NotFound(DeviceName),
    #[error("no output device available: {source}")]
    NoDevice {
        requested_device: OutputDevice,
        #[source]
        source: rodio::StreamError,
    },
    #[error("cannot look up output device {requested_device}: {source}")]
    Lookup {
        requested_device: OutputDevice,
        #[source]
        source: cpal::DevicesError,
    },
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

pub(crate) fn decode_error_of(error: Error) -> AudioError {
    match error {
        Error::Open { path, source } => AudioError::Decode {
            path,
            error: DecodeError::Unreadable(source.kind().into()),
        },
        Error::Decode { path, source } => AudioError::Decode {
            path,
            error: decode_error(&source),
        },
        Error::WorkerPanicked(path) => AudioError::Decode {
            path,
            error: DecodeError::Panicked,
        },
    }
}

pub(crate) fn device_error(error: &DeviceError) -> AudioError {
    match error {
        DeviceError::NotFound(name) => AudioError::OpenDevice {
            requested_device: OutputDevice::Named(name.clone()),
            diagnostic: Diagnostic::from_error(error),
        },
        DeviceError::NoDevice {
            requested_device,
            source,
        } => AudioError::OpenDevice {
            requested_device: requested_device.clone(),
            diagnostic: Diagnostic::from_error(source),
        },
        DeviceError::Lookup {
            requested_device,
            source,
        } => AudioError::OpenDevice {
            requested_device: requested_device.clone(),
            diagnostic: Diagnostic::from_error(source),
        },
    }
}

pub(crate) fn list_devices_error(error: &cpal::DevicesError) -> AudioError {
    AudioError::ListDevices {
        diagnostic: Diagnostic::from_error(error),
    }
}

pub(crate) fn output_error(error: &cpal::StreamError) -> OutputError {
    match error {
        cpal::StreamError::DeviceNotAvailable => OutputError::DeviceGone,
        cpal::StreamError::BackendSpecific { .. } => OutputError::Backend,
    }
}

pub(crate) fn seek_error(error: &rodio::source::SeekError) -> AudioError {
    AudioError::Seek {
        diagnostic: Diagnostic::from_error(error),
    }
}

pub(crate) fn preload_error(error: Error) -> AudioError {
    match error {
        Error::Open { path, source } => AudioError::Preload {
            path,
            error: DecodeError::Unreadable(source.kind().into()),
        },
        Error::Decode { path, source } => AudioError::Preload {
            path,
            error: decode_error(&source),
        },
        Error::WorkerPanicked(path) => AudioError::Preload {
            path,
            error: DecodeError::Panicked,
        },
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
            transport::OutputError,
        },
        message::{AudioError, DecodeError},
    };
    use rstest::rstest;

    use crate::error::{
        DeviceError,
        Error,
        decode_error_of,
        device_error,
        output_error,
        preload_error,
    };

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
    fn an_error_display_carries_its_source(
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
        let error = decode_error_of(error);
        assert_eq!(
            error,
            AudioError::Decode {
                path: PathBuf::from("/music/track.flac"),
                error: expected,
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
            error: DecodeError::Unreadable(IoError::Missing),
        }
    )]
    #[case::worker_panicked(
        Error::WorkerPanicked(PathBuf::from("/a")),
        AudioError::Decode { path: PathBuf::from("/a"), error: DecodeError::Panicked }
    )]
    fn an_error_becomes_an_audio_error_with_its_path(
        #[case] error: Error,
        #[case] expected: AudioError,
    ) {
        assert_eq!(decode_error_of(error), expected);
    }

    #[test]
    fn a_device_error_names_the_requested_device() {
        let error = DeviceError::NotFound(device_name("usb"));
        assert_eq!(
            device_error(&error),
            AudioError::OpenDevice {
                requested_device: OutputDevice::Named(device_name("usb")),
                diagnostic: Diagnostic::from_error(&DeviceError::NotFound(
                    device_name("usb")
                )),
            }
        );
    }

    #[test]
    fn an_open_failure_keeps_its_cause() {
        let source = rodio::StreamError::NoDevice;
        let cause = source.to_string();
        let error = device_error(&DeviceError::NoDevice {
            requested_device: OutputDevice::SystemDefault,
            source,
        });
        assert!(error.to_string().contains(&cause));
    }

    fn backend_error() -> rodio::cpal::BackendSpecificError {
        rodio::cpal::BackendSpecificError {
            description: "host gone".to_owned(),
        }
    }

    #[test]
    fn a_lookup_failure_while_opening_maps_to_an_open_failure() {
        let source = rodio::cpal::DevicesError::BackendSpecific {
            err: backend_error(),
        };
        let diagnostic = Diagnostic::from_error(&source);
        let error = DeviceError::Lookup {
            requested_device: OutputDevice::Named(device_name("usb")),
            source,
        };
        assert_eq!(
            device_error(&error),
            AudioError::OpenDevice {
                requested_device: OutputDevice::Named(device_name("usb")),
                diagnostic,
            }
        );
    }

    #[rstest]
    #[case::no_named_device(
        DeviceError::NoDevice {
            requested_device: OutputDevice::Named(device_name("usb")),
            source: rodio::StreamError::NoDevice,
        },
        AudioError::OpenDevice {
            requested_device: OutputDevice::Named(device_name("usb")),
            diagnostic: Diagnostic::from_error(&rodio::StreamError::NoDevice),
        }
    )]
    #[case::no_default_device(
        DeviceError::NoDevice {
            requested_device: OutputDevice::SystemDefault,
            source: rodio::StreamError::NoDevice,
        },
        AudioError::OpenDevice {
            requested_device: OutputDevice::SystemDefault,
            diagnostic: Diagnostic::from_error(&rodio::StreamError::NoDevice),
        }
    )]
    fn every_device_error_maps_to_its_audio_error(
        #[case] error: DeviceError,
        #[case] expected: AudioError,
    ) {
        assert_eq!(device_error(&error), expected);
    }

    #[rstest]
    #[case::open_denied(
        Error::Open {
            path: PathBuf::from("/a"),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        },
        AudioError::Preload {
            path: PathBuf::from("/a"),
            error: DecodeError::Unreadable(IoError::Denied),
        }
    )]
    #[case::decode_unsupported(
        Error::Decode {
            path: PathBuf::from("/a"),
            source: rodio::decoder::DecoderError::UnrecognizedFormat,
        },
        AudioError::Preload { path: PathBuf::from("/a"), error: DecodeError::Unsupported }
    )]
    #[case::worker_panicked(
        Error::WorkerPanicked(PathBuf::from("/a")),
        AudioError::Preload { path: PathBuf::from("/a"), error: DecodeError::Panicked }
    )]
    fn a_preload_error_maps_to_its_audio_error(
        #[case] error: Error,
        #[case] expected: AudioError,
    ) {
        assert_eq!(preload_error(error), expected);
    }

    #[rstest]
    #[case::device_not_available(
        rodio::cpal::StreamError::DeviceNotAvailable,
        OutputError::DeviceGone
    )]
    #[case::backend_specific(
        rodio::cpal::StreamError::BackendSpecific {
            err: rodio::cpal::BackendSpecificError {
                description: "underrun".to_string(),
            },
        },
        OutputError::Backend
    )]
    fn a_stream_error_maps_to_its_output_error(
        #[case] error: rodio::cpal::StreamError,
        #[case] expected: OutputError,
    ) {
        assert_eq!(output_error(&error), expected);
    }
}
