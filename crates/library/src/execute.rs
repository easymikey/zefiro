use kernel::{cmd::DiskCmd, update::machine::Driver};

use crate::{
    cover::CoverDecoded,
    dirs::LibraryDirs,
    driver::{LibraryDriver, LibraryEffect},
    error::Error,
    favorites,
    history,
    message::LibraryMessage,
    playlists,
    trash,
};

impl<P: FnMut(CoverDecoded)> Driver for LibraryDriver<P> {
    type Effect = LibraryEffect;

    fn execute(&mut self, effect: LibraryEffect) -> Option<LibraryMessage> {
        match effect {
            LibraryEffect::PublishCover(decoded) => {
                (self.publish_cover)(decoded);
                None
            }
            LibraryEffect::Execute(disk_cmd) => execute_disk(disk_cmd, &self.dirs)
                .unwrap_or_else(|error| Some(LibraryMessage::Error(error))),
        }
    }
}

fn execute_disk(
    disk_cmd: DiskCmd,
    dirs: &LibraryDirs,
) -> Result<Option<LibraryMessage>, Error> {
    match disk_cmd {
        DiskCmd::AppendHistory(entry) => history::append(dirs, &entry).map(|()| None),
        DiskCmd::SaveFavorites(favorites) => {
            favorites::save(dirs, &favorites).map(|()| None)
        }
        DiskCmd::LoadFavorites => favorites::load(dirs)
            .map(|favorites| Some(LibraryMessage::FavoritesLoaded(favorites))),
        DiskCmd::Trash(path) => {
            trash::move_to_trash(&path).map(|()| Some(LibraryMessage::Trashed(path)))
        }
        DiskCmd::LoadHistory(limit) => history::load(dirs, limit).map(
            |history::HistoryRead { entries, skipped }| {
                Some(LibraryMessage::HistoryLoaded { entries, skipped })
            },
        ),
        DiskCmd::SavePlaylist { name, tracks } => {
            playlists::save(dirs, &name, &tracks).map(|()| None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use kernel::{
        cmd::DiskCmd,
        domain::{
            favorites::Favorites,
            geometry::Pixels,
            history::HistoryEntry,
            playlist::PlaylistFileName,
            revision::Revision,
            time::Moment,
            track::TrackSource,
        },
        message::LibraryEvent,
        update::machine::{Driver, LoopEffect, Machine},
    };
    use rstest::{fixture, rstest};
    use tempfile::TempDir;

    use crate::{
        cover::{CoverDecoded, CoverLookup},
        dirs::LibraryDirs,
        driver::{LibraryDriver, LibraryEffect},
        job::LibraryJob,
        message::LibraryMessage,
        test_support::{self, temp_dir_filters},
    };

    const AUDIO_EXTENSIONS: &[&str] = &["flac", "wav"];

    #[fixture]
    fn dirs() -> (TempDir, LibraryDirs) {
        let directory = tempfile::tempdir().unwrap();
        let library_dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        (directory, library_dirs)
    }

    fn unpublished(decoded: CoverDecoded) {
        let CoverDecoded { path, .. } = decoded;
        panic!("nothing is published here: {path:?}");
    }

    fn transitioned_event(
        message: Option<LibraryMessage>,
        library_driver: &mut LibraryDriver<fn(CoverDecoded)>,
    ) -> Option<LibraryEvent> {
        message.and_then(|message| {
            library_driver
                .transition(message)
                .unwrap()
                .into_parts()
                .1
                .into_iter()
                .next()
        })
    }

    fn execute(disk_cmd: DiskCmd, library_dirs: &LibraryDirs) -> Option<LibraryEvent> {
        let mut library_driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(library_dirs.clone(), AUDIO_EXTENSIONS, unpublished);
        transitioned_event(
            library_driver.execute(LibraryEffect::Execute(disk_cmd)),
            &mut library_driver,
        )
    }

    fn scan(
        music_dir: PathBuf,
        revision: Revision,
        library_dirs: &LibraryDirs,
    ) -> Option<LibraryEvent> {
        let mut library_driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(library_dirs.clone(), AUDIO_EXTENSIONS, unpublished);
        let message = LibraryJob::Scan {
            music_dir,
            revision,
            dirs: Arc::new(library_dirs.clone()),
            audio_extensions: AUDIO_EXTENSIONS,
        }
        .run();
        let (effects, events) =
            library_driver.transition(message).unwrap().into_parts();
        let listed_events: Vec<LibraryEvent> = effects
            .into_iter()
            .flat_map(|effect| {
                let LoopEffect::Run(job) = effect else {
                    panic!("expected a job, got {effect:?}");
                };
                library_driver.transition(job.run()).unwrap().into_parts().1
            })
            .collect();
        let library_events: Vec<LibraryEvent> =
            events.into_iter().chain(listed_events).collect();
        match <[LibraryEvent; 1]>::try_from(library_events) {
            Ok([event]) => Some(event),
            Err(library_events) => panic!("expected one event, got {library_events:?}"),
        }
    }

    fn played_song() -> HistoryEntry {
        HistoryEntry::from_track(
            &test_support::titled("/music/song.flac", "Song"),
            Moment::default(),
        )
    }

    fn saved_favorites() -> Favorites {
        [TrackSource::Local("/music/a.flac".into())]
            .into_iter()
            .collect()
    }

    #[rstest]
    #[case::append_history(vec![DiskCmd::AppendHistory(played_song())], None)]
    #[case::save_favorites(vec![DiskCmd::SaveFavorites(saved_favorites())], None)]
    #[case::save_playlist(
        vec![DiskCmd::SavePlaylist {
            name: PlaylistFileName::new("My Mix").unwrap(),
            tracks: vec![test_support::titled("/music/song.flac", "Song")],
        }],
        None
    )]
    #[case::load_favorites(
        vec![DiskCmd::SaveFavorites(saved_favorites()), DiskCmd::LoadFavorites],
        Some(LibraryEvent::FavoritesLoaded(saved_favorites()))
    )]
    #[case::load_history(
        vec![DiskCmd::AppendHistory(played_song()), DiskCmd::LoadHistory(10)],
        Some(LibraryEvent::HistoryLoaded(vec![played_song()]))
    )]
    fn a_disk_command_answers_what_it_did(
        dirs: (TempDir, LibraryDirs),
        #[case] disk_cmds: Vec<DiskCmd>,
        #[case] expected: Option<LibraryEvent>,
    ) {
        let (_directory, library_dirs) = dirs;

        let events: Vec<Option<LibraryEvent>> = disk_cmds
            .into_iter()
            .map(|disk_cmd| execute(disk_cmd, &library_dirs))
            .collect();

        assert_eq!(events.last(), Some(&expected));
    }

    #[rstest]
    fn trash_of_a_missing_file(dirs: (TempDir, LibraryDirs)) {
        let (directory, library_dirs) = dirs;
        let missing = directory.path().join("never-existed.flac");

        let event = execute(DiskCmd::Trash(missing.clone()), &library_dirs);

        assert_eq!(event, Some(LibraryEvent::Trashed(missing)));
    }

    #[rstest]
    fn a_published_cover_reaches_the_sink(dirs: (TempDir, LibraryDirs)) {
        let (_directory, library_dirs) = dirs;
        let mut published = Vec::new();
        {
            let mut library_driver = LibraryDriver::new(
                library_dirs,
                AUDIO_EXTENSIONS,
                |decoded: CoverDecoded| {
                    published.push((decoded.path, decoded.side));
                },
            );
            let answer =
                library_driver.execute(LibraryEffect::PublishCover(CoverDecoded {
                    path: PathBuf::from("/music/one.flac"),
                    side: Pixels(64),
                    cover_lookup: CoverLookup::Missing,
                }));
            assert!(answer.is_none());
        }

        assert_eq!(
            published,
            vec![(PathBuf::from("/music/one.flac"), Pixels(64))]
        );
    }

    const TONE: &[u8] = include_bytes!("../tests/fixtures/tone.wav");

    fn scanned_music_dir(directory: &TempDir) -> PathBuf {
        let music_dir = directory.path().join("music");
        std::fs::create_dir_all(&music_dir).unwrap();
        std::fs::write(music_dir.join("a.wav"), TONE).unwrap();
        music_dir
    }

    #[rstest]
    fn a_fresh_scan_replies_with_the_tracks_it_found_under_the_revision_it_was_given(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, library_dirs) = dirs;
        let music_dir = scanned_music_dir(&directory);

        let event = scan(music_dir, Revision::default().next(), &library_dirs);

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[rstest]
    fn tag_tracks_answers_every_listed_path_under_the_revision_it_was_given(
        dirs: (TempDir, LibraryDirs),
    ) {
        let (directory, library_dirs) = dirs;
        let music_dir = scanned_music_dir(&directory);
        let listed = vec![
            TrackSource::Local(music_dir.join("a.wav")),
            TrackSource::Local(music_dir.join("never-existed.flac")),
        ];

        let mut library_driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(library_dirs.clone(), AUDIO_EXTENSIONS, unpublished);
        let event = transitioned_event(
            Some(
                LibraryJob::Tag {
                    music_dir,
                    track_sources: listed,
                    revision: Revision::default().next(),
                    dirs: Arc::new(library_dirs),
                }
                .run(),
            ),
            &mut library_driver,
        );

        insta::with_settings!({ filters => temp_dir_filters() }, {
            insta::assert_debug_snapshot!(event);
        });
    }

    #[test]
    fn a_failed_disk_command_answers_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = directory.path().join("blocker");
        std::fs::write(&blocker, b"a file, not a directory").unwrap();
        let library_dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: blocker.join("data"),
            playlists_dir: blocker.join("playlists"),
        };
        let mut library_driver: LibraryDriver<fn(CoverDecoded)> =
            LibraryDriver::new(library_dirs, AUDIO_EXTENSIONS, unpublished);

        let message = library_driver.execute(LibraryEffect::Execute(
            DiskCmd::SaveFavorites(Favorites::default()),
        ));

        assert!(
            matches!(message, Some(LibraryMessage::Error(_))),
            "{message:?}"
        );
    }
}
