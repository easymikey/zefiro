use std::{collections::HashMap, path::Path, sync::Arc};

use crate::{
    cmd::{Cmd, Effect, LibraryCmd},
    domain::{
        cue::Cue,
        favorites::Favorites,
        history::HistoryEntry,
        index::TrackIndex,
        library::{Library, sort_indices},
        model::ScanStatus,
        overlay::Overlay,
        player::Player,
        playlist::{Playlist, PlaylistSource},
        revision::{Freshness, Revision, Revisions},
        toast::Toast,
        track::{Track, TrackSource},
        workspace::Workspace,
    },
    message::{LibraryError, LibraryEvent, LibrarySubject},
    update::{
        browse::{ResyncParts, relist, resync_playlist},
        machine::{Unhandled, replace},
    },
};

pub(crate) struct LibraryParts<'a> {
    pub(crate) library: &'a mut Option<Library>,
    pub(crate) favorites: &'a mut Favorites,
    pub(crate) history: &'a mut Vec<HistoryEntry>,
    pub(crate) scan_status: &'a mut ScanStatus,
    pub(crate) music_dir: &'a Path,
    pub(crate) revisions: &'a mut Revisions,
    pub(crate) workspace: &'a mut Workspace,
    pub(crate) playlist: &'a mut Playlist,
    pub(crate) playlist_source: &'a PlaylistSource,
    pub(crate) player: &'a mut Player,
    pub(crate) queue: &'a mut Vec<TrackSource>,
}

pub(crate) fn update(
    mut parts: LibraryParts<'_>,
    event: LibraryEvent,
) -> Result<Cmd, Unhandled> {
    if let LibraryEvent::Loaded {
        revision,
        tracks: _tracks,
    }
    | LibraryEvent::Listed {
        revision,
        tracks: _tracks,
    }
    | LibraryEvent::Tagged {
        revision,
        tracks: _tracks,
    } = &event
        && let Freshness::Stale = revision.freshness(parts.revisions.scan)
    {
        return Err(Unhandled);
    }
    match event {
        LibraryEvent::FavoritesLoaded(favorites) => {
            replace(parts.favorites, favorites).map(|()| Cmd::none())
        }
        LibraryEvent::Loaded {
            tracks,
            revision: _revision,
        } => Ok(whole_library(&mut parts, tracks)),
        LibraryEvent::Listed { tracks, revision } => {
            Ok(listed_library(&mut parts, tracks, revision))
        }
        LibraryEvent::Tagged {
            tracks,
            revision: _revision,
        } => Ok(tagged_tracks(&mut parts, &tracks)),
        LibraryEvent::HistoryLoaded(entries) => {
            replace(parts.history, entries).map(|()| Cmd::none())
        }
        LibraryEvent::Trashed(path) => trashed_track(&mut parts, &path),
        LibraryEvent::Error(error) => Ok(library_failed(&mut parts, &error)),
    }
}

fn trashed_track(
    library_parts: &mut LibraryParts<'_>,
    path: &Path,
) -> Result<Cmd, Unhandled> {
    let library = library_parts.library.as_mut().ok_or(Unhandled)?;
    let removed_position = library
        .tracks
        .iter()
        .position(|track| track.local_path() == Some(path))
        .ok_or(Unhandled)?;
    let track = library.tracks.remove(removed_position);
    library.track_indexes = library
        .track_indexes
        .iter()
        .filter(|index| index.get() != removed_position)
        .map(|index| {
            if index.get() > removed_position {
                TrackIndex::new(index.get() - 1)
            } else {
                *index
            }
        })
        .collect();
    library_parts
        .queue
        .retain(|queued| queued != track.source());
    let resync_parts = ResyncParts {
        workspace: &mut *library_parts.workspace,
        player: library_parts.player,
        playlist: library_parts.playlist,
    };
    match library_parts.playlist_source {
        PlaylistSource::Library => {
            resync_playlist(PlaylistSource::Library, library, resync_parts);
        }
        PlaylistSource::Named => {
            let kept = std::mem::take(&mut resync_parts.playlist.tracks)
                .into_iter()
                .filter(|listed| listed.source() != track.source())
                .collect();
            relist(kept, resync_parts);
        }
    }
    Ok(Cmd::from(Cue::TrackTrashed))
}

fn library_failed(parts: &mut LibraryParts<'_>, error: &LibraryError) -> Cmd {
    match error {
        LibraryError::NoUserDirs
        | LibraryError::Disk {
            subject: LibrarySubject::Scan,
            ..
        } => *parts.scan_status = ScanStatus::Idle,
        LibraryError::Disk { .. } | LibraryError::DecodeCover { .. } => {}
    }
    parts.workspace.show(
        Toast::error("Library error").with_text(error.to_string()),
        parts.revisions,
    )
}

fn whole_library(parts: &mut LibraryParts<'_>, tracks: Vec<Arc<Track>>) -> Cmd {
    *parts.scan_status = ScanStatus::Idle;
    let opening = match parts.library {
        None => Cmd::from(Cue::LibraryOpened),
        Some(_) => Cmd::none(),
    };
    library_loaded(parts, tracks);
    opening
}

fn listed_library(
    parts: &mut LibraryParts<'_>,
    listed_tracks: Vec<Arc<Track>>,
    revision: Revision,
) -> Cmd {
    *parts.scan_status = ScanStatus::Tagging {
        done: 0,
        total: listed_tracks.len(),
    };
    let tagging = tag_request(parts.music_dir, &listed_tracks, revision);
    library_loaded(parts, listed_tracks);
    tagging_progress(parts.scan_status, 0).then(tagging)
}

fn tag_request(
    music_dir: &Path,
    listed_tracks: &[Arc<Track>],
    revision: Revision,
) -> Cmd {
    if listed_tracks.is_empty() {
        return Cmd::none();
    }
    Effect::Library(LibraryCmd::TagTracks {
        music_dir: music_dir.to_path_buf(),
        track_sources: listed_tracks
            .iter()
            .map(|track| track.source().clone())
            .collect(),
        revision,
    })
    .into()
}

fn tagged_tracks(parts: &mut LibraryParts<'_>, tagged_tracks: &[Arc<Track>]) -> Cmd {
    let read = tagged_tracks.len();
    let tagged: Tagged<'_> = tagged_tracks
        .iter()
        .map(|track| (track.source(), track))
        .collect();
    if let Some(ready) = parts.library {
        retag_tracks(&mut ready.tracks, &tagged);
    }
    retag_tracks(&mut parts.playlist.tracks, &tagged);
    retag_player(parts.player, &tagged);
    if let Some(Overlay::Search(search)) = parts.workspace.overlay.as_mut() {
        crate::update::overlay::search::rerank(search, &parts.playlist.tracks);
    }
    tagging_progress(parts.scan_status, read)
}

fn library_loaded(parts: &mut LibraryParts<'_>, tracks: Vec<Arc<Track>>) {
    install_library(parts, tracks);
    if let Some(ready) = parts.library {
        resync_playlist(
            *parts.playlist_source,
            ready,
            ResyncParts {
                workspace: &mut *parts.workspace,
                player: parts.player,
                playlist: parts.playlist,
            },
        );
    }
}

type Tagged<'a> = HashMap<&'a TrackSource, &'a Arc<Track>>;

fn retag_tracks(tracks: &mut [Arc<Track>], tagged: &Tagged<'_>) {
    for track in tracks {
        if let Some(read) = tagged.get(track.source()) {
            *track = Arc::clone(read);
        }
    }
}

fn retag_player(player: &mut Player, tagged: &Tagged<'_>) {
    match player {
        Player::Loading(track) => retag_tracks(std::slice::from_mut(track), tagged),
        Player::Paused {
            track,
            position: _position,
            by: _by,
        } => retag_tracks(std::slice::from_mut(track), tagged),
        Player::Playing {
            track,
            playhead: _playhead,
            preloaded,
        } => {
            retag_tracks(std::slice::from_mut(track), tagged);
            retag_tracks(preloaded.as_mut_slice(), tagged);
        }
        Player::Stopped => {}
    }
}

fn tagging_progress(scan_status: &mut ScanStatus, read: usize) -> Cmd {
    let ScanStatus::Tagging { done, total } = *scan_status else {
        return Cmd::none();
    };
    let done = done.saturating_add(read).min(total);
    if done >= total {
        *scan_status = ScanStatus::Idle;
        return Cmd::from(Cue::LibraryOpened);
    }
    *scan_status = ScanStatus::Tagging { done, total };
    Cmd::none()
}

fn install_library(parts: &mut LibraryParts<'_>, tracks: Vec<Arc<Track>>) {
    let sort_key = parts.workspace.browse.sort_key;
    let view = sort_indices(&tracks, sort_key, parts.favorites);
    *parts.library = Some(Library {
        tracks,
        track_indexes: view,
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rstest::rstest;

    use crate::{
        cmd::{Cmd, Effect},
        domain::{
            config::Diagnostic,
            cue::Cue,
            history::HistoryEntry,
            index::TrackIndex,
            io_error::IoError,
            library::Library,
            model::{Model, ScanStatus},
            playlist::PlaylistSource,
            revision::Revision,
            time::Moment,
            toast::{TOAST_LIFETIME, ToastLevel},
            track::{Track, TrackSource},
        },
        message::{LibraryError, LibraryEvent, LibrarySubject, Timer},
        update::{library::update, machine::Unhandled},
    };

    fn track(path: &str) -> Arc<Track> {
        Arc::new(Track::listed(std::path::Path::new(path)))
    }

    #[test]
    fn library_loaded_replaces_the_library_and_mirrors_it_into_the_playlist() {
        let mut model = Model {
            library: Some(Library {
                tracks: Vec::new(),
                track_indexes: vec![TrackIndex::new(7)],
            }),
            ..Model::default()
        };

        let cmd = update(
            crate::update::library_parts(&mut model),
            LibraryEvent::Loaded {
                tracks: vec![track("/music/a.flac"), track("/music/b.flac")],
                revision: Revision::default(),
            },
        )
        .unwrap();

        let library = model.library.as_ref();
        assert_eq!(
            library.map(|ready| ready
                .tracks
                .iter()
                .filter_map(|t| t.local_path())
                .collect::<Vec<_>>()),
            Some(vec![
                std::path::Path::new("/music/a.flac"),
                std::path::Path::new("/music/b.flac")
            ])
        );
        assert_eq!(
            library.map(|ready| ready.track_indexes.as_slice()),
            Some([TrackIndex::new(0), TrackIndex::new(1)].as_slice())
        );
        assert_eq!(model.playlist.tracks.len(), 2);
        assert!(cmd == Cmd::none());
    }

    #[test]
    fn a_stale_scan_is_unhandled_and_keeps_the_library() {
        let mut model = Model::default();
        model.revisions.scan = Revision::default().next();

        let cmd = update(
            crate::update::library_parts(&mut model),
            LibraryEvent::Loaded {
                tracks: vec![track("/music/a.flac")],
                revision: Revision::default(),
            },
        );

        assert_eq!(cmd, Err(Unhandled));
        assert!(model.library.is_none());
        assert!(model.playlist.tracks.is_empty());
    }

    #[test]
    fn a_named_playlist_survives_a_full_scan() {
        let named = vec![track("/elsewhere/one.flac")];
        let mut model = Model {
            playlist_source: PlaylistSource::Named,
            playlist: crate::domain::playlist::Playlist::from_tracks(named.clone()),
            ..Model::default()
        };

        let cmd = update(
            crate::update::library_parts(&mut model),
            LibraryEvent::Loaded {
                tracks: vec![track("/music/a.flac"), track("/music/b.flac")],
                revision: Revision::default(),
            },
        )
        .unwrap();

        assert_eq!(model.playlist.tracks, named);
        assert_eq!(
            model.library.as_ref().map(|ready| ready.tracks.len()),
            Some(2)
        );
        assert_eq!(cmd, Cmd::effect(Effect::Animate(Cue::LibraryOpened)));
    }

    #[test]
    fn a_trashed_track_leaves_a_named_playlist() {
        let a = track("/music/a.flac");
        let b = track("/music/b.flac");
        let mut model = Model {
            library: Some(Library {
                tracks: vec![Arc::clone(&a), Arc::clone(&b)],
                track_indexes: vec![TrackIndex::new(0), TrackIndex::new(1)],
            }),
            playlist_source: PlaylistSource::Named,
            playlist: crate::domain::playlist::Playlist::from_tracks(vec![
                Arc::clone(&a),
                Arc::clone(&b),
            ]),
            ..Model::default()
        };
        model.workspace.browse.cursor = crate::domain::cursor::Cursor::at(2, 1);

        let cmd = update(
            crate::update::library_parts(&mut model),
            LibraryEvent::Trashed(a.local_path().unwrap().to_path_buf()),
        )
        .unwrap();

        assert_eq!(model.playlist.tracks, vec![b]);
        assert!(model.workspace.browse.cursor.index() < model.playlist.tracks.len());
        assert_eq!(cmd, Cmd::from(Cue::TrackTrashed));
    }

    #[test]
    fn a_library_error_raises_an_error_toast() {
        let mut model = Model::default();

        let cmd = update(
            crate::update::library_parts(&mut model),
            LibraryEvent::Error(LibraryError::NoUserDirs),
        )
        .unwrap();

        let toast = model.workspace.toasts.first().unwrap();
        assert_eq!(toast.level, ToastLevel::Error);
        assert_eq!(
            toast.text.as_deref(),
            Some(LibraryError::NoUserDirs.to_string().as_str())
        );
        assert_eq!(
            cmd,
            Cmd::from_iter([
                Effect::Animate(Cue::ToastRaised),
                Effect::After {
                    delay: TOAST_LIFETIME,
                    timer: Timer::Toast(Revision::default().next()),
                },
            ])
        );
    }

    fn unreadable(subject: LibrarySubject) -> LibraryError {
        LibraryError::Disk {
            subject,
            path: "/music".into(),
            error: IoError::Missing,
        }
    }

    #[rstest]
    #[case::no_user_dirs_ends_the_scan(LibraryError::NoUserDirs, ScanStatus::Idle)]
    #[case::a_failed_scan_ends_the_scan(
        unreadable(LibrarySubject::Scan),
        ScanStatus::Idle
    )]
    #[case::a_history_failure_keeps_scanning(
        unreadable(LibrarySubject::History),
        ScanStatus::Scanning
    )]
    #[case::a_cover_failure_keeps_scanning(
        LibraryError::DecodeCover {
            path: "/music/one.flac".into(),
            diagnostic: Diagnostic::from_error(&std::io::Error::other("no tag")),
        },
        ScanStatus::Scanning
    )]
    fn a_library_error_settles_the_scan(
        #[case] error: LibraryError,
        #[case] expected: ScanStatus,
    ) {
        let mut model = Model {
            scan_status: ScanStatus::Scanning,
            ..Model::default()
        };

        let cmd = update(
            crate::update::library_parts(&mut model),
            LibraryEvent::Error(error),
        );

        assert!(cmd.is_ok());
        assert_eq!(model.scan_status, expected);
    }

    #[test]
    fn a_loaded_history_replaces_the_history_and_emits_nothing() {
        let mut model = Model {
            history: vec![HistoryEntry {
                track_source: TrackSource::Local("/music/stale.flac".into()),
                title: "Stale".into(),
                artist: None,
                played_at: Moment::new(std::time::Duration::from_secs(1)),
            }],
            ..Default::default()
        };

        let fresh = vec![
            HistoryEntry {
                track_source: TrackSource::Local("/music/b.flac".into()),
                title: "B".into(),
                artist: Some("Artist".into()),
                played_at: Moment::new(std::time::Duration::from_secs(200)),
            },
            HistoryEntry {
                track_source: TrackSource::Local("/music/a.flac".into()),
                title: "A".into(),
                artist: None,
                played_at: Moment::new(std::time::Duration::from_secs(100)),
            },
        ];
        let cmd = update(
            crate::update::library_parts(&mut model),
            LibraryEvent::HistoryLoaded(fresh.clone()),
        )
        .unwrap();

        assert_eq!(model.history, fresh);
        assert!(cmd == Cmd::none());
    }
}
