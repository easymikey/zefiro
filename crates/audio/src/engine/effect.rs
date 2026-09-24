use std::{path::PathBuf, time::Duration};

use kernel::{AudioCmd, AudioEvent, AudioFailure, Playback, domain::Speed};

use crate::deck::{Landed, Reopening};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Preload {
    Gapless(PathBuf),
    Crossfade(PreloadedTrack),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreloadedTrack {
    pub(crate) path: PathBuf,
    pub(crate) gain: Option<f32>,
    pub(crate) total: Option<Duration>,
}

#[derive(Debug, Clone)]
pub(crate) enum EngineMessage {
    Cmd(AudioCmd),
    Opened(Result<Reopening, AudioFailure>),
    Decoded(Result<Option<Duration>, AudioFailure>),
    Preloaded(Result<Preload, AudioFailure>),
    Failed(AudioFailure),
    Observed {
        queue_len: usize,
        position: Duration,
    },
}

#[derive(Debug, Default, PartialEq)]
pub(crate) enum EngineEffect {
    #[default]
    Nothing,
    Many(Vec<EngineEffect>),
    Send(AudioEvent),
    Mute(AudioFailure),
    Open {
        device: Option<String>,
        speed: Speed,
    },
    StartLoad {
        path: PathBuf,
        speed: Speed,
    },
    StartFade {
        path: PathBuf,
        speed: Speed,
    },
    Decode(PathBuf),
    Start {
        volume: f32,
        total: Option<Duration>,
    },
    Resume {
        volume: f32,
        position: Duration,
        paused: Playback,
    },
    Play,
    Pause,
    Seek(Duration),
    SetVolume(f32),
    Fade {
        outgoing: f32,
        incoming: f32,
        at: Duration,
    },
    Retire {
        playing: f32,
        retiring: f32,
        at: Duration,
    },
    Retired {
        playing: f32,
        at: Duration,
    },
    SetSpeed(Speed),
    Clear,
    PreloadGapless(PathBuf),
    PreloadCrossfade {
        path: PathBuf,
        gain: Option<f32>,
        speed: Speed,
    },
    RestartGapless(PathBuf),
    Promote {
        volume: f32,
    },
    ListDevices,
}

impl From<Landed> for Preload {
    fn from(landed: Landed) -> Self {
        match landed {
            Landed::Gapless(path) => Preload::Gapless(path),
            Landed::Crossfade { path, gain, total } => {
                Preload::Crossfade(PreloadedTrack { path, gain, total })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use rstest::rstest;

    use crate::{
        deck::Landed,
        engine::effect::{Preload, PreloadedTrack},
    };

    #[rstest]
    #[case::gapless(
        Landed::Gapless(PathBuf::from("/a")),
        Preload::Gapless(PathBuf::from("/a"))
    )]
    #[case::crossfade(
        Landed::Crossfade {
            path: PathBuf::from("/b"),
            gain: Some(0.5),
            total: Some(Duration::from_secs(10)),
        },
        Preload::Crossfade(PreloadedTrack {
            path: PathBuf::from("/b"),
            gain: Some(0.5),
            total: Some(Duration::from_secs(10)),
        })
    )]
    fn a_landed_preload_becomes_a_fact(
        #[case] landed: Landed,
        #[case] expected: Preload,
    ) {
        assert_eq!(Preload::from(landed), expected);
    }
}
