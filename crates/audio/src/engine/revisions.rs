use std::path::PathBuf;

use kernel::domain::revision::Revision;

use crate::deck::{event::DeckEvent, job::AudioJob};

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

    pub(crate) fn decode(&mut self, path: PathBuf) -> AudioJob {
        self.preload = self.issue();
        self.decode = self.issue();
        AudioJob::Decode {
            path,
            revision: self.decode,
        }
    }

    pub(crate) fn preload(&mut self, path: PathBuf) -> AudioJob {
        self.preload = self.issue();
        AudioJob::Preload {
            path,
            revision: self.preload,
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.preload = self.issue();
        self.decode = self.issue();
    }

    fn issue(&mut self) -> Revision {
        self.issued = self.issued.next();
        self.issued
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::domain::revision::Revision;
    use rstest::rstest;

    use crate::{
        deck::{event::DeckEvent, job::AudioJob},
        engine::revisions::JobRevisions,
        error::Error,
    };

    fn failed() -> Result<crate::deck::source::TrackDecoder, Error> {
        Err(Error::WorkerPanicked(PathBuf::from("/a")))
    }

    fn revision(issued: u8) -> Revision {
        (0..issued).fold(Revision::default(), |revision, _| revision.next())
    }

    type Step = Box<dyn Fn(&mut JobRevisions)>;

    fn decode(path: &str) -> Step {
        let path = PathBuf::from(path);
        Box::new(move |revisions| {
            revisions.decode(path.clone());
        })
    }

    fn preload(path: &str) -> Step {
        let path = PathBuf::from(path);
        Box::new(move |revisions| {
            revisions.preload(path.clone());
        })
    }

    fn cancel() -> Step {
        Box::new(JobRevisions::cancel)
    }

    #[test]
    fn a_decode_job_carries_the_newest_revision() {
        let mut revisions = JobRevisions::default();
        assert_eq!(
            revisions.decode("/a".into()),
            AudioJob::Decode {
                path: "/a".into(),
                revision: revision(2),
            }
        );
    }

    #[rstest]
    #[case::current_decode(vec![decode("/a")], DeckEvent::Decoded { revision: revision(2), result: failed() }, true)]
    #[case::decode_after_a_second_load(vec![decode("/a"), decode("/b")], DeckEvent::Decoded { revision: revision(2), result: failed() }, false)]
    #[case::decode_after_clear(vec![decode("/a"), cancel()], DeckEvent::Decoded { revision: revision(2), result: failed() }, false)]
    #[case::current_preload(vec![decode("/a"), preload("/b")], DeckEvent::Preloaded { revision: revision(3), result: failed() }, true)]
    #[case::preload_after_a_load(vec![preload("/b"), decode("/a")], DeckEvent::Preloaded { revision: revision(1), result: failed() }, false)]
    #[case::preload_after_a_newer_preload(vec![preload("/b"), preload("/c")], DeckEvent::Preloaded { revision: revision(1), result: failed() }, false)]
    #[case::woke_event(vec![cancel()], DeckEvent::Woke(revision(9)), true)]
    fn a_result_is_current_only_for_the_newest_revision(
        #[case] steps: Vec<Step>,
        #[case] event: DeckEvent,
        #[case] current: bool,
    ) {
        let mut revisions = JobRevisions::default();
        steps.iter().for_each(|step| step(&mut revisions));
        assert_eq!(revisions.current(&event), current);
    }
}
