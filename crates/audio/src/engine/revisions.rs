use kernel::{AudioEvent, Cmd};

use crate::{
    deck::{AudioJob, DeckEvent, Revision},
    engine::effect::EngineEffect,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Revisions {
    issued: Revision,
    decode: Revision,
    preload: Revision,
}

impl Revisions {
    pub(crate) fn current(&self, event: &DeckEvent) -> bool {
        match event {
            DeckEvent::Decoded { revision, .. } => *revision == self.decode,
            DeckEvent::Preloaded { revision, .. } => *revision == self.preload,
            DeckEvent::OutputLost(_)
            | DeckEvent::DevicesListed(_)
            | DeckEvent::Track(_) => true,
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
            | EngineEffect::StartHandover { path, .. } => {
                self.preload = self.issue();
                Some(self.decode_job(path))
            }
            EngineEffect::Decode(path) => Some(self.decode_job(path)),
            EngineEffect::Preload { path, .. } | EngineEffect::RestartGapless(path) => {
                self.preload = self.issue();
                Some(AudioJob::Preload {
                    path: path.clone(),
                    revision: self.preload,
                })
            }
            EngineEffect::Mute | EngineEffect::Clear => {
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
            | EngineEffect::SetVolume(_)
            | EngineEffect::Arm(_)
            | EngineEffect::Crossfade { .. }
            | EngineEffect::CancelCrossfade
            | EngineEffect::Ramp { .. }
            | EngineEffect::DropOutgoing
            | EngineEffect::SetSpeed(_)
            | EngineEffect::Promote(_)
            | EngineEffect::Run(_)
            | EngineEffect::Report
            | EngineEffect::Advance
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

    use kernel::{Cmd, domain::Speed};
    use rstest::rstest;

    use crate::{
        deck::{AudioJob, DeckEvent, Revision, source::PreloadMode},
        engine::{effect::EngineEffect, revisions::Revisions},
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
        EngineEffect::Preload {
            path: path.into(),
            mode: PreloadMode::Gapless,
        }
    }

    #[test]
    fn a_load_runs_a_decode_job_after_the_load() {
        let mut revisions = Revisions::default();
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
    #[case::decode_after_clear(vec![load("/a"), EngineEffect::Clear], DeckEvent::Decoded { revision: revision(2), result: failed() }, false)]
    #[case::current_preload(vec![load("/a"), gapless("/b")], DeckEvent::Preloaded { revision: revision(3), result: failed() }, true)]
    #[case::preload_after_a_load(vec![gapless("/b"), load("/a")], DeckEvent::Preloaded { revision: revision(1), result: failed() }, false)]
    #[case::track_event(vec![EngineEffect::Mute], DeckEvent::Track(revision(9)), true)]
    fn a_result_is_current_only_for_the_newest_ticket(
        #[case] effects: Vec<EngineEffect>,
        #[case] event: DeckEvent,
        #[case] current: bool,
    ) {
        let mut revisions = Revisions::default();
        for effect in effects {
            assert!(revisions.with_jobs(Cmd::effect(effect)).effects().count() > 0);
        }
        assert_eq!(revisions.current(&event), current);
    }
}
