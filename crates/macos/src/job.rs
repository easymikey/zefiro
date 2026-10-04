#![forbid(unsafe_code)]

use std::{
    fs,
    io,
    path::{Path, PathBuf},
};

use kernel::{domain::revision::Revision, message::MacosError};

use crate::message::{CoverBytes, MacosMessage};

pub(crate) type CoverReader = fn(&Path) -> io::Result<Option<Vec<u8>>>;

const COVER_NAMES: [&str; 6] = [
    "cover.jpg",
    "cover.png",
    "folder.jpg",
    "folder.png",
    "front.jpg",
    "front.png",
];

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
                    bytes: cover_bytes(&track, read)
                        .map_err(|error| MacosError::Cover(error.kind().into())),
                })
            }
        }
    }
}

fn cover_bytes(track: &Path, read: CoverReader) -> io::Result<Vec<u8>> {
    read(track)?.map_or_else(
        || folder_cover(track).map_or_else(|| Ok(Vec::new()), fs::read),
        Ok,
    )
}

fn folder_cover(track: &Path) -> Option<PathBuf> {
    let folder = track.parent()?;
    COVER_NAMES
        .iter()
        .map(|name| folder.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    use kernel::{domain::revision::Revision, message::MacosError};

    use crate::{
        job::{MacosJob, cover_bytes, folder_cover},
        message::{CoverBytes, MacosMessage},
    };

    fn revision(count: u8) -> Revision {
        (0..count).fold(Revision::default(), |revision, _| revision.next())
    }

    fn embedded(_track: &Path) -> io::Result<Option<Vec<u8>>> {
        Ok(Some(b"embedded".to_vec()))
    }

    fn untagged(_track: &Path) -> io::Result<Option<Vec<u8>>> {
        Ok(None)
    }

    #[test]
    fn the_cover_beside_the_track_is_found() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("folder.png"), b"png").unwrap();
        assert_eq!(
            folder_cover(&folder.path().join("track.flac")),
            Some(folder.path().join("folder.png"))
        );
    }

    #[test]
    fn the_embedded_cover_wins_over_the_folder_cover() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("cover.jpg"), b"jpg").unwrap();
        let track = folder.path().join("track.flac");
        assert_eq!(cover_bytes(&track, embedded).unwrap(), b"embedded".to_vec());
        assert_eq!(cover_bytes(&track, untagged).unwrap(), b"jpg".to_vec());
    }

    #[test]
    fn a_track_without_any_cover_gives_no_cover() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            cover_bytes(&folder.path().join("track.flac"), untagged).unwrap(),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn an_unreadable_folder_cover_is_reported() {
        let folder = tempfile::tempdir().unwrap();
        let cover = folder.path().join("cover.jpg");
        fs::write(&cover, b"jpg").unwrap();
        fs::set_permissions(&cover, fs::Permissions::from_mode(0o000)).unwrap();
        let job = MacosJob::ReadCover {
            track: folder.path().join("track.flac"),
            revision: revision(1),
        };
        assert!(matches!(
            job.run(untagged),
            MacosMessage::CoverRead(CoverBytes { revision: read, bytes: Err(MacosError::Cover(_)) })
                if read == revision(1)
        ));
    }

    fn unreadable_tags(_track: &Path) -> io::Result<Option<Vec<u8>>> {
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
                bytes: Err(MacosError::Cover(_)),
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
