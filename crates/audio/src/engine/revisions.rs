use std::path::PathBuf;

use kernel::domain::revision::Revision;

use crate::{deck::job::AudioJob, engine::message::AudioMessage};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct JobRevisions {
    issued: Revision,
    decode: Revision,
    preload: Revision,
}

impl JobRevisions {
    pub(crate) fn is_current(&self, message: &AudioMessage) -> bool {
        match message {
            AudioMessage::Decoded { revision, .. } => *revision == self.decode,
            AudioMessage::Preloaded { revision, .. } => {
                self.is_current_preload(*revision)
            }
            AudioMessage::Deck(_)
            | AudioMessage::DevicesListed(_)
            | AudioMessage::SignalsTaken { .. }
            | AudioMessage::Engine(_) => true,
        }
    }

    pub(crate) fn is_current_preload(&self, revision: Revision) -> bool {
        revision == self.preload
    }

    pub(crate) fn decode_job(&mut self, path: PathBuf) -> AudioJob {
        self.preload = self.issue();
        self.decode = self.issue();
        AudioJob::Decode {
            path,
            revision: self.decode,
        }
    }

    pub(crate) fn preload_job(&mut self, path: PathBuf) -> AudioJob {
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
        engine::{message::AudioMessage, revisions::JobRevisions},
        error::Error,
    };

    fn failed() -> Result<crate::deck::source::TrackDecoder, Error> {
        Err(Error::WorkerPanicked(PathBuf::from("/a")))
    }

    fn revision(issued: u8) -> Revision {
        (0..issued).fold(Revision::default(), |revision, _| revision.next())
    }

    type Step = Box<dyn Fn(&mut JobRevisions)>;

    fn decode_job_step(path: &str) -> Step {
        let path = PathBuf::from(path);
        Box::new(move |revisions| {
            revisions.decode_job(path.clone());
        })
    }

    fn preload_job_step(path: &str) -> Step {
        let path = PathBuf::from(path);
        Box::new(move |revisions| {
            revisions.preload_job(path.clone());
        })
    }

    fn cancel_step() -> Step {
        Box::new(JobRevisions::cancel)
    }

    #[test]
    fn a_decode_job_carries_the_newest_revision() {
        let mut job_revisions = JobRevisions::default();
        assert_eq!(
            job_revisions.decode_job("/a".into()),
            AudioJob::Decode {
                path: "/a".into(),
                revision: revision(2),
            }
        );
    }

    #[rstest]
    #[case::current_decode(vec![decode_job_step("/a")], AudioMessage::Decoded { revision: revision(2), result: failed() }, true)]
    #[case::decode_after_a_second_load(vec![decode_job_step("/a"), decode_job_step("/b")], AudioMessage::Decoded { revision: revision(2), result: failed() }, false)]
    #[case::decode_after_cancel(vec![decode_job_step("/a"), cancel_step()], AudioMessage::Decoded { revision: revision(2), result: failed() }, false)]
    #[case::is_current_preload(vec![decode_job_step("/a"), preload_job_step("/b")], AudioMessage::Preloaded { revision: revision(3), result: failed() }, true)]
    #[case::preload_after_a_load(vec![preload_job_step("/b"), decode_job_step("/a")], AudioMessage::Preloaded { revision: revision(1), result: failed() }, false)]
    #[case::preload_after_a_newer_preload(vec![preload_job_step("/b"), preload_job_step("/c")], AudioMessage::Preloaded { revision: revision(1), result: failed() }, false)]
    #[case::woke_event(vec![cancel_step()], AudioMessage::Deck(DeckEvent::Woke(revision(9))), true)]
    fn a_result_is_current_only_for_the_newest_revision(
        #[case] steps: Vec<Step>,
        #[case] message: AudioMessage,
        #[case] is_current: bool,
    ) {
        let mut job_revisions = JobRevisions::default();
        steps.iter().for_each(|step| step(&mut job_revisions));
        assert_eq!(job_revisions.is_current(&message), is_current);
    }
}
