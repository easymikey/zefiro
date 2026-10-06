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
    message::{CoverBytes, MacosMessage},
};

pub(crate) type MacosLoopCmd = LoopCmd<MacosEffect, MacosJob, MacosMessage, MacosEvent>;

pub(crate) type CoverReader = fn(&Path) -> io::Result<Vec<u8>>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MacosJob {
    ReadCover { track: PathBuf, revision: Revision },
}

impl MacosJob {
    #[must_use]
    pub fn run(self, read: CoverReader) -> MacosMessage {
        match self {
            MacosJob::ReadCover { track, revision } => {
                MacosMessage::CoverRead(CoverBytes {
                    revision,
                    bytes: read(&track)
                        .map_err(|error| MacosError::ReadArtwork(error.kind().into())),
                })
            }
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
        message::{CoverBytes, MacosMessage},
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
        let job = MacosJob::ReadCover {
            track: PathBuf::from("a.flac"),
            revision: revision(1),
        };
        assert!(matches!(
            job.run(unreadable_tags),
            MacosMessage::CoverRead(CoverBytes {
                bytes: Err(MacosError::ReadArtwork(_)),
                ..
            })
        ));
    }

    #[test]
    fn the_cover_job_answers_with_its_revision() {
        let job = MacosJob::ReadCover {
            track: PathBuf::from("a.flac"),
            revision: revision(3),
        };
        let MacosMessage::CoverRead(read) = job.run(embedded) else {
            panic!("a cover job answers with CoverRead");
        };
        assert_eq!(
            read,
            CoverBytes {
                revision: revision(3),
                bytes: Ok(b"embedded".to_vec()),
            }
        );
    }
}
