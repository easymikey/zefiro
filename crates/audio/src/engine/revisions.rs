use kernel::{cmd::Cmd, domain::revision::Revision, message::AudioEvent};

use crate::{
    deck::{event::DeckEvent, job::AudioJob, source::PreloadMode},
    engine::{effect::EngineEffect, phase::CurrentTrack},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct JobRevisions {
    issued: Revision,
    decode: Revision,
    preload: Revision,
}

impl JobRevisions {
    pub(crate) fn current(&self, event: &DeckEvent) -> bool {
        match event {
            DeckEvent::Decoded { revision, .. } => *revision == self.decode,
            DeckEvent::Preloaded { revision, .. } => *revision == self.preload,
            DeckEvent::OutputLost(_)
            | DeckEvent::DevicesListed(_)
            | DeckEvent::Woke(_) => true,
        }
    }

    pub(crate) fn with_jobs(
        &mut self,
        cmd: Cmd<EngineEffect, AudioEvent>,
    ) -> Cmd<EngineEffect, AudioEvent> {
        let (effects, messages) = cmd.into_parts();
        let with_jobs: Cmd<EngineEffect, AudioEvent> = effects
            .into_iter()
            .flat_map(|effect| {
                let job = self.job_for(&effect);
                [effect].into_iter().chain(job.map(EngineEffect::Run))
            })
            .collect();
        messages
            .into_iter()
            .map(Cmd::message)
            .fold(with_jobs, Cmd::then)
    }

    fn issue(&mut self) -> Revision {
        self.issued = self.issued.next();
        self.issued
    }

    fn job_for(&mut self, effect: &EngineEffect) -> Option<AudioJob> {
        match effect {
            EngineEffect::StartLoad { path, .. }
            | EngineEffect::StartHandover { path, .. }
            | EngineEffect::Decode(path) => {
                self.preload = self.issue();
                Some(self.decode_job(path))
            }
            EngineEffect::Preload(
                PreloadMode::Gapless(path)
                | PreloadMode::Crossfade {
                    track: CurrentTrack { path, .. },
                    ..
                },
            )
            | EngineEffect::RestartGapless(path) => {
                self.preload = self.issue();
                Some(AudioJob::Preload {
                    path: path.clone(),
                    revision: self.preload,
                })
            }
            EngineEffect::Silence | EngineEffect::Clear(_) => {
                self.preload = self.issue();
                self.decode = self.issue();
                None
            }
            EngineEffect::Open { .. }
            | EngineEffect::Start(_)
            | EngineEffect::Resume { .. }
            | EngineEffect::Play
            | EngineEffect::Pause
            | EngineEffect::Seek(_)
            | EngineEffect::SetGain(_)
            | EngineEffect::Arm(_)
            | EngineEffect::Crossfade { .. }
            | EngineEffect::CancelCrossfade
            | EngineEffect::Ramp { .. }
            | EngineEffect::DropOutgoing
            | EngineEffect::SetSpeed(_)
            | EngineEffect::Promote(_)
            | EngineEffect::Run(_)
            | EngineEffect::Report
            | EngineEffect::Advance(_)
            | EngineEffect::Stage(_)
            | EngineEffect::Attach(_)
            | EngineEffect::TakeSignals(_) => None,
        }
    }

    fn decode_job(&mut self, path: &std::path::Path) -> AudioJob {
        self.decode = self.issue();
        AudioJob::Decode {
            path: path.to_path_buf(),
            revision: self.decode,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{
        cmd::Cmd,
        domain::{revision::Revision, speed::Speed},
    };
    use rstest::rstest;

    use crate::{
        deck::{event::DeckEvent, job::AudioJob, source::PreloadMode},
        engine::{effect::EngineEffect, revisions::JobRevisions},
        error::Error,
    };

    fn failed() -> Result<crate::deck::source::TrackDecoder, Error> {
        Err(Error::WorkerPanicked(PathBuf::from("/a")))
    }

    fn revision(issued: u8) -> Revision {
        (0..issued).fold(Revision::default(), |revision, _| revision.next())
    }

    fn load(path: &str) -> EngineEffect {
        EngineEffect::StartLoad {
            path: path.into(),
            speed: Speed::default(),
        }
    }

    fn gapless(path: &str) -> EngineEffect {
        EngineEffect::Preload(PreloadMode::Gapless(path.into()))
    }

    #[test]
    fn a_load_runs_a_decode_job_after_the_load() {
        let mut revisions = JobRevisions::default();
        let cmd = revisions.with_jobs(Cmd::effect(load("/a")));
        let job = AudioJob::Decode {
            path: "/a".into(),
            revision: revision(2),
        };
        assert_eq!(
            cmd,
            Cmd::effect(load("/a")).then(Cmd::effect(EngineEffect::Run(job)))
        );
    }

    #[rstest]
    #[case::current_decode(vec![load("/a")], DeckEvent::Decoded { revision: revision(2), result: failed() }, true)]
    #[case::decode_after_a_second_load(vec![load("/a"), load("/b")], DeckEvent::Decoded { revision: revision(2), result: failed() }, false)]
    #[case::decode_after_clear(vec![load("/a"), EngineEffect::Clear(Speed::default())], DeckEvent::Decoded { revision: revision(2), result: failed() }, false)]
    #[case::current_preload(vec![load("/a"), gapless("/b")], DeckEvent::Preloaded { revision: revision(3), result: failed() }, true)]
    #[case::preload_after_a_load(vec![gapless("/b"), load("/a")], DeckEvent::Preloaded { revision: revision(1), result: failed() }, false)]
    #[case::preload_after_a_reopen_decode(vec![gapless("/b"), EngineEffect::Decode("/a".into())], DeckEvent::Preloaded { revision: revision(1), result: failed() }, false)]
    #[case::woke_event(vec![EngineEffect::Silence], DeckEvent::Woke(revision(9)), true)]
    fn a_result_is_current_only_for_the_newest_revision(
        #[case] effects: Vec<EngineEffect>,
        #[case] event: DeckEvent,
        #[case] current: bool,
    ) {
        let mut revisions = JobRevisions::default();
        for effect in effects {
            assert!(revisions.with_jobs(Cmd::effect(effect)).effects().count() > 0);
        }
        assert_eq!(revisions.current(&event), current);
    }
}
