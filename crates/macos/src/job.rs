#![forbid(unsafe_code)]

use std::{
    io,
    path::{Path, PathBuf},
};

use kernel::{
    domain::revision::Revision,
    message::{MacosError, MacosEvent},
    update::machine::LoopCmd,
};

use crate::{
    effect::MacosEffect,
    message::{ArtworkBytes, MacosMessage},
};

pub(crate) type MacosLoopCmd = LoopCmd<MacosEffect, MacosJob, MacosMessage, MacosEvent>;

pub(crate) type ArtworkReader = fn(&Path) -> io::Result<Vec<u8>>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MacosJob {
    ReadArtwork {
        track_path: PathBuf,
        revision: Revision,
    },
}

impl MacosJob {
    #[must_use]
    pub fn run(self, artwork_reader: ArtworkReader) -> MacosMessage {
        match self {
            MacosJob::ReadArtwork {
                track_path,
                revision,
            } => MacosMessage::ArtworkRead(ArtworkBytes {
                revision,
                bytes: artwork_reader(&track_path)
                    .map_err(|error| MacosError::ReadArtwork(error.kind().into())),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        path::{Path, PathBuf},
    };

    use kernel::{domain::revision::Revision, message::MacosError};

    use crate::{
        job::MacosJob,
        message::{ArtworkBytes, MacosMessage},
    };

    fn revision(count: u8) -> Revision {
        (0..count).fold(Revision::default(), |revision, _| revision.next())
    }

    fn embedded(_track: &Path) -> io::Result<Vec<u8>> {
        Ok(b"embedded".to_vec())
    }

    fn unreadable_tags(_track: &Path) -> io::Result<Vec<u8>> {
        Err(io::Error::other("unreadable tags"))
    }

    #[test]
    fn an_unreadable_tag_is_reported() {
        let job = MacosJob::ReadArtwork {
            track_path: PathBuf::from("a.flac"),
            revision: revision(1),
        };
        assert!(matches!(
            job.run(unreadable_tags),
            MacosMessage::ArtworkRead(ArtworkBytes {
                bytes: Err(MacosError::ReadArtwork(_)),
                ..
            })
        ));
    }

    #[test]
    fn the_artwork_job_answers_with_its_revision() {
        let job = MacosJob::ReadArtwork {
            track_path: PathBuf::from("a.flac"),
            revision: revision(3),
        };
        let MacosMessage::ArtworkRead(read) = job.run(embedded) else {
            panic!("an artwork job answers with ArtworkRead");
        };
        assert_eq!(
            read,
            ArtworkBytes {
                revision: revision(3),
                bytes: Ok(b"embedded".to_vec()),
            }
        );
    }
}
