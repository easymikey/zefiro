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

#[derive(Debug, thiserror::Error)]
pub(crate) enum OpenError {
    #[error("cannot read the device's output config: {0}")]
    Config(#[source] cpal::DefaultStreamConfigError),
    #[error("cannot list the device's output configs: {0}")]
    Configs(#[source] cpal::SupportedStreamConfigsError),
    #[error("cannot build the output stream: {0}")]
    Build(#[source] cpal::BuildStreamError),
    #[error("cannot start the output stream: {0}")]
    Play(#[source] cpal::PlayStreamError),
    #[error("unsupported sample format {0}")]
    UnsupportedFormat(cpal::SampleFormat),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DeviceError {
    #[error("audio device '{0}' not found")]
    NotFound(DeviceName),
    #[error("no output device available: {source}")]
    NoDevice {
        requested_device: OutputDevice,
        #[source]
        source: OpenError,
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
        source: symphonia::core::errors::Error,
    },
    #[error("the decode worker panicked on {0}")]
    WorkerPanicked(PathBuf),
}

fn decode_error(source: &symphonia::core::errors::Error) -> DecodeError {
    match source {
        symphonia::core::errors::Error::Unsupported(_) => DecodeError::Unsupported,
        symphonia::core::errors::Error::IoError(_) => {
            DecodeError::Unreadable(IoError::Other)
        }
        symphonia::core::errors::Error::DecodeError(_)
        | symphonia::core::errors::Error::SeekError(_)
        | symphonia::core::errors::Error::LimitError(_)
        | symphonia::core::errors::Error::ResetRequired => DecodeError::Corrupt,
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

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub(crate) struct SeekError(#[from] symphonia::core::errors::Error);

pub(crate) fn seek_error(error: &impl std::error::Error) -> AudioError {
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
        OpenError,
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
            source: symphonia::core::errors::Error::Unsupported("core (probe): no suitable format reader found"),
        },
        "cannot decode /music/track.flac: unsupported feature: core (probe): no suitable format reader found"
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
        symphonia::core::errors::Error::Unsupported("no suitable format reader"),
        DecodeError::Unsupported
    )]
    #[case::io_error(
        symphonia::core::errors::Error::IoError(std::io::Error::from(
            std::io::ErrorKind::BrokenPipe
        )),
        DecodeError::Unreadable(IoError::Other)
    )]
    #[case::decode_error(
        symphonia::core::errors::Error::DecodeError("bad frame"),
        DecodeError::Corrupt
    )]
    #[case::limit_error(
        symphonia::core::errors::Error::LimitError("too large"),
        DecodeError::Corrupt
    )]
    #[case::reset_required(
        symphonia::core::errors::Error::ResetRequired,
        DecodeError::Corrupt
    )]
    #[case::seek_error(
        symphonia::core::errors::Error::SeekError(
            symphonia::core::errors::SeekErrorKind::OutOfRange
        ),
        DecodeError::Corrupt
    )]
    fn a_decode_error_maps_to_its_audio_error(
        #[case] source: symphonia::core::errors::Error,
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
        let source =
            OpenError::Config(cpal::DefaultStreamConfigError::DeviceNotAvailable);
        let cause = source.to_string();
        let error = device_error(&DeviceError::NoDevice {
            requested_device: OutputDevice::SystemDefault,
            source,
        });
        assert!(error.to_string().contains(&cause));
    }

    fn backend_error() -> cpal::BackendSpecificError {
        cpal::BackendSpecificError {
            description: "host gone".to_owned(),
        }
    }

    #[test]
    fn a_lookup_failure_while_opening_maps_to_an_open_failure() {
        let source = cpal::DevicesError::BackendSpecific {
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
    #[case::no_config_on_a_named_device(
        OutputDevice::Named(device_name("usb")),
        || OpenError::Config(cpal::DefaultStreamConfigError::DeviceNotAvailable)
    )]
    #[case::no_config_list_on_the_default_device(
        OutputDevice::SystemDefault,
        || OpenError::Configs(cpal::SupportedStreamConfigsError::DeviceNotAvailable)
    )]
    #[case::a_stream_that_cannot_be_built(
        OutputDevice::SystemDefault,
        || OpenError::Build(cpal::BuildStreamError::StreamConfigNotSupported)
    )]
    #[case::a_stream_that_cannot_start(
        OutputDevice::Named(device_name("usb")),
        || OpenError::Play(cpal::PlayStreamError::DeviceNotAvailable)
    )]
    #[case::an_unsupported_sample_format(
        OutputDevice::SystemDefault,
        || OpenError::UnsupportedFormat(cpal::SampleFormat::U8)
    )]
    fn every_open_error_maps_to_an_open_device_error_with_its_diagnostic(
        #[case] requested_device: OutputDevice,
        #[case] open_error: fn() -> OpenError,
    ) {
        let error = DeviceError::NoDevice {
            requested_device: requested_device.clone(),
            source: open_error(),
        };
        assert_eq!(
            device_error(&error),
            AudioError::OpenDevice {
                requested_device,
                diagnostic: Diagnostic::from_error(&open_error()),
            }
        );
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
            source: symphonia::core::errors::Error::Unsupported("no suitable format reader"),
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
        cpal::StreamError::DeviceNotAvailable,
        OutputError::DeviceGone
    )]
    #[case::backend_specific(
        cpal::StreamError::BackendSpecific {
            err: cpal::BackendSpecificError {
                description: "underrun".to_string(),
            },
        },
        OutputError::Backend
    )]
    fn a_stream_error_maps_to_its_output_error(
        #[case] error: cpal::StreamError,
        #[case] expected: OutputError,
    ) {
        assert_eq!(output_error(&error), expected);
    }
}
