use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use kernel::{
    cmd::{GrowingMedia, Media},
    domain::revision::Revision,
    update::machine::Unhandled,
};

use crate::{
    deck::{job::AudioJob, source::GrowingDownload},
    engine::message::{AudioMessage, EngineMessage},
};

#[derive(Debug, Default)]
pub(crate) struct JobRevisions {
    issued: Revision,
    decode: Revision,
    preload: Revision,
    preloaded_revision: Option<Revision>,
    downloaded: HashMap<Revision, Arc<AtomicU64>>,
}

impl JobRevisions {
    pub(crate) fn is_current(&self, message: &AudioMessage) -> bool {
        match message {
            AudioMessage::Decoded {
                revision,
                result: _result,
            } => *revision == self.decode,
            AudioMessage::Preloaded {
                revision,
                result: _result,
            } => self.is_current_preload(*revision),
            AudioMessage::Engine(EngineMessage::Interrupted(revision, _)) => {
                *revision >= self.decode
            }
            AudioMessage::Cmds(_)
            | AudioMessage::Deck(_)
            | AudioMessage::DevicesListed(_)
            | AudioMessage::SignalsTaken { .. }
            | AudioMessage::Engine(_)
            | AudioMessage::Started
            | AudioMessage::Fed => true,
        }
    }

    pub(crate) fn is_current_preload(&self, revision: Revision) -> bool {
        revision == self.preload
    }

    pub(crate) fn decode_job(&mut self, media: Media) -> AudioJob {
        self.preload = self.issue();
        self.decode = self.issue();
        self.preloaded_revision = None;
        let (media_path, download) = self.download(media);
        AudioJob::Decode {
            media_path,
            download,
            revision: self.decode,
        }
    }

    pub(crate) fn preload_job(&mut self, media: Media) -> AudioJob {
        self.preload = self.issue();
        let (media_path, download) = self.download(media);
        self.preloaded_revision = download.as_ref().map(|download| download.revision);
        AudioJob::Preload {
            media_path,
            download,
            revision: self.preload,
        }
    }

    pub(crate) fn grow(
        &self,
        revision: Revision,
        downloaded: u64,
    ) -> Result<(), Unhandled> {
        self.downloaded
            .get(&revision)
            .ok_or(Unhandled)?
            .fetch_max(downloaded, Ordering::Release);
        Ok(())
    }

    pub(crate) fn decoded(&mut self, download_revision: Option<Revision>) {
        let preloaded_revision = self.preloaded_revision;
        self.downloaded.retain(|revision, _bound| {
            Some(*revision) == download_revision
                || Some(*revision) == preloaded_revision
        });
    }

    pub(crate) fn drop_preload(&mut self, current: &Media) {
        self.preloaded_revision = None;
        self.decoded(match current {
            Media::Local(_media_path) => None,
            Media::Growing(GrowingMedia {
                media_path: _media_path,
                downloaded: _downloaded,
                byte_len: _byte_len,
                revision,
            }) => Some(*revision),
        });
    }

    fn download(&mut self, media: Media) -> (PathBuf, Option<GrowingDownload>) {
        match media {
            Media::Local(media_path) => (media_path, None),
            Media::Growing(GrowingMedia {
                media_path,
                downloaded,
                byte_len,
                revision,
            }) => {
                let bound = Arc::clone(self.downloaded.entry(revision).or_default());
                bound.fetch_max(downloaded, Ordering::Release);
                let download = GrowingDownload {
                    downloaded: bound,
                    byte_len,
                    revision,
                    read_byte: Arc::default(),
                };
                (media_path, Some(download))
            }
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.preload = self.issue();
        self.decode = self.issue();
        self.preloaded_revision = None;
        self.downloaded.clear();
    }

    fn issue(&mut self) -> Revision {
        self.issued = self.issued.next();
        self.issued
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::atomic::Ordering};

    use kernel::{
        cmd::{GrowingMedia, Media},
        domain::revision::Revision,
        message::{AudioError, DecodeError},
        update::machine::Unhandled,
    };
    use rstest::rstest;

    use crate::{
        deck::{
            event::DeckEvent,
            job::AudioJob,
            source::tests::{decoded, ramp_file},
        },
        engine::{
            message::{AudioMessage, EngineMessage},
            revisions::JobRevisions,
            tests::assert_same,
        },
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
        let media = Media::Local(PathBuf::from(path));
        Box::new(move |revisions| {
            revisions.decode_job(media.clone());
        })
    }

    fn preload_job_step(path: &str) -> Step {
        let media = Media::Local(PathBuf::from(path));
        Box::new(move |revisions| {
            revisions.preload_job(media.clone());
        })
    }

    fn cancel_step() -> Step {
        Box::new(JobRevisions::cancel)
    }

    #[test]
    fn a_decode_job_carries_the_newest_revision() {
        let mut job_revisions = JobRevisions::default();
        assert_same(
            job_revisions.decode_job(Media::Local("/a".into())),
            AudioJob::Decode {
                media_path: "/a".into(),
                download: None,
                revision: revision(2),
            },
        );
    }

    fn growing(media_path: PathBuf, downloaded: u64, byte_len: u64) -> Media {
        Media::Growing(GrowingMedia {
            media_path,
            downloaded,
            byte_len,
            revision: revision(9),
        })
    }

    fn bound(audio_job: &AudioJob) -> u64 {
        let (AudioJob::Decode {
            media_path: _media_path,
            download: Some(download),
            revision: _revision,
        }
        | AudioJob::Preload {
            media_path: _media_path,
            download: Some(download),
            revision: _revision,
        }) = audio_job
        else {
            panic!("a growing media gives a growing job, got {audio_job:?}");
        };
        download.downloaded.load(Ordering::Acquire)
    }

    #[test]
    fn a_decode_again_of_a_growing_media_starts_at_the_bound_grow_raised() {
        let mut job_revisions = JobRevisions::default();
        let loaded = job_revisions.decode_job(growing("/a".into(), 100, 1_000));

        job_revisions.grow(revision(9), 600).unwrap();
        let resumed = job_revisions.decode_job(growing("/a".into(), 100, 1_000));

        assert_eq!((bound(&loaded), bound(&resumed)), (600, 600));
    }

    #[test]
    fn a_grow_for_media_no_job_holds_is_refused() {
        let mut job_revisions = JobRevisions::default();
        drop(job_revisions.decode_job(growing("/a".into(), 100, 1_000)));
        job_revisions.cancel();

        assert_eq!(
            (
                job_revisions.grow(revision(9), 600),
                job_revisions.grow(revision(8), 600)
            ),
            (Err(Unhandled), Err(Unhandled))
        );
    }

    #[test]
    fn a_decoded_track_keeps_only_its_own_download_growing() {
        let mut job_revisions = JobRevisions::default();
        drop(job_revisions.decode_job(growing("/a".into(), 100, 1_000)));
        let mut other_job_revisions = JobRevisions::default();
        drop(other_job_revisions.decode_job(growing("/a".into(), 100, 1_000)));

        job_revisions.decoded(Some(revision(9)));
        other_job_revisions.decoded(None);

        assert_eq!(
            (
                job_revisions.grow(revision(9), 600),
                other_job_revisions.grow(revision(9), 600)
            ),
            (Ok(()), Err(Unhandled))
        );
    }

    fn growing_preload(media_path: PathBuf, revision: Revision) -> Media {
        Media::Growing(GrowingMedia {
            media_path,
            downloaded: 100,
            byte_len: 1_000,
            revision,
        })
    }

    #[test]
    fn a_decoded_track_keeps_the_download_of_its_pending_preload_growing() {
        let mut job_revisions = JobRevisions::default();
        drop(job_revisions.decode_job(growing("/a".into(), 100, 1_000)));
        drop(job_revisions.preload_job(growing_preload("/b".into(), revision(7))));

        job_revisions.decoded(Some(revision(9)));

        assert_eq!(
            (
                job_revisions.grow(revision(9), 600),
                job_revisions.grow(revision(7), 600)
            ),
            (Ok(()), Ok(()))
        );
    }

    #[test]
    fn a_dropped_preload_clears_its_download_and_keeps_the_current_one() {
        let mut job_revisions = JobRevisions::default();
        let current = growing("/a".into(), 100, 1_000);
        drop(job_revisions.decode_job(current.clone()));
        job_revisions.decoded(Some(revision(9)));
        drop(job_revisions.preload_job(growing_preload("/b".into(), revision(7))));

        job_revisions.drop_preload(&current);

        assert_eq!(
            (
                job_revisions.grow(revision(9), 600),
                job_revisions.grow(revision(7), 600)
            ),
            (Ok(()), Err(Unhandled))
        );
    }

    #[test]
    fn a_decoded_track_clears_the_download_of_a_superseded_preload() {
        let mut job_revisions = JobRevisions::default();
        drop(job_revisions.decode_job(growing("/a".into(), 100, 1_000)));
        drop(job_revisions.preload_job(growing_preload("/b".into(), revision(7))));
        drop(job_revisions.preload_job(growing_preload("/c".into(), revision(6))));

        job_revisions.decoded(Some(revision(9)));

        assert_eq!(
            (
                job_revisions.grow(revision(7), 600),
                job_revisions.grow(revision(6), 600)
            ),
            (Err(Unhandled), Ok(()))
        );
    }

    #[test]
    fn a_download_grown_in_steps_through_grow_decodes_like_the_whole_file() {
        const MARGIN: u64 = 64 * 1024;
        let file = ramp_file(2, 200_000);
        let byte_len = file.as_file().metadata().unwrap().len();
        let mut job_revisions = JobRevisions::default();
        let audio_job =
            job_revisions.decode_job(growing(file.path().into(), 44, byte_len));
        let AudioJob::Decode {
            media_path,
            download: Some(download),
            revision: _revision,
        } = audio_job
        else {
            panic!("a load of a growing media gives a growing decode job");
        };
        let mut decoder = download.decode(&media_path).unwrap();
        let mut samples = Vec::new();

        for downloaded in [byte_len / 4, byte_len / 2, byte_len * 3 / 4, byte_len] {
            job_revisions.grow(revision(9), downloaded).unwrap();
            while (44 + 2 * u64::try_from(samples.len()).unwrap() + MARGIN
                <= downloaded
                || downloaded == byte_len)
                && let available @ [_, ..] = decoder.frames().unwrap()
            {
                samples.extend_from_slice(available);
                let frames = available.len() / 2;
                decoder.consume(frames);
            }
        }

        assert_eq!(samples, decoded(&file));
    }

    #[rstest]
    #[case::current_decode(vec![decode_job_step("/a")], AudioMessage::Decoded { revision: revision(2), result: failed() }, true)]
    #[case::decode_after_a_second_load(vec![decode_job_step("/a"), decode_job_step("/b")], AudioMessage::Decoded { revision: revision(2), result: failed() }, false)]
    #[case::decode_after_cancel(vec![decode_job_step("/a"), cancel_step()], AudioMessage::Decoded { revision: revision(2), result: failed() }, false)]
    #[case::is_current_preload(vec![decode_job_step("/a"), preload_job_step("/b")], AudioMessage::Preloaded { revision: revision(3), result: failed() }, true)]
    #[case::preload_after_a_load(vec![preload_job_step("/b"), decode_job_step("/a")], AudioMessage::Preloaded { revision: revision(1), result: failed() }, false)]
    #[case::preload_after_a_newer_preload(vec![preload_job_step("/b"), preload_job_step("/c")], AudioMessage::Preloaded { revision: revision(1), result: failed() }, false)]
    #[case::woke_event(vec![cancel_step()], AudioMessage::Deck(DeckEvent::Woke(revision(9))), true)]
    #[case::interruption_of_the_loaded_feed(vec![decode_job_step("/a")], AudioMessage::Engine(EngineMessage::Interrupted(revision(2), AudioError::Decode { path: "/a".into(), error: DecodeError::Corrupt })), true)]
    #[case::interruption_of_a_preloaded_feed(vec![decode_job_step("/a"), preload_job_step("/b")], AudioMessage::Engine(EngineMessage::Interrupted(revision(3), AudioError::Decode { path: "/a".into(), error: DecodeError::Corrupt })), true)]
    #[case::interruption_of_the_playing_preload_after_a_newer_preload(vec![decode_job_step("/a"), preload_job_step("/b"), preload_job_step("/c")], AudioMessage::Engine(EngineMessage::Interrupted(revision(3), AudioError::Decode { path: "/a".into(), error: DecodeError::Corrupt })), true)]
    #[case::interruption_after_a_second_load(vec![decode_job_step("/a"), decode_job_step("/b")], AudioMessage::Engine(EngineMessage::Interrupted(revision(2), AudioError::Decode { path: "/a".into(), error: DecodeError::Corrupt })), false)]
    #[case::interruption_after_cancel(vec![decode_job_step("/a"), cancel_step()], AudioMessage::Engine(EngineMessage::Interrupted(revision(2), AudioError::Decode { path: "/a".into(), error: DecodeError::Corrupt })), false)]
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
