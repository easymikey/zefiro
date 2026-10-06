use std::{collections::HashMap, path::Path, sync::Arc};

use crate::{
    cmd::{Cmd, Effect, LibraryCmd},
    domain::{
        cue::Cue,
        favorites::Favorites,
        history::HistoryEntry,
        library::{Library, sort_indices},
        model::ScanStatus,
        overlay::Overlay,
        player::Player,
        playlist::{Playlist, PlaylistSource},
        revision::{Freshness, Revision, Revisions},
        toast::Toast,
        track::{Track, TrackRef},
        workspace::Workspace,
    },
    message::{LibraryError, LibraryEvent, LibrarySubject},
    update::machine::Unhandled,
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
}

pub(crate) fn update(
    mut parts: LibraryParts<'_>,
    event: LibraryEvent,
) -> Result<Cmd, Unhandled> {
    if let LibraryEvent::Loaded { revision, .. }
    | LibraryEvent::Listed { revision, .. }
    | LibraryEvent::Tagged { revision, .. } = &event
        && let Freshness::Stale = revision.freshness(parts.revisions.scan)
    {
        return Err(Unhandled);
    }
    match event {
        LibraryEvent::FavoritesLoaded(favorites) => {
            *parts.favorites = favorites;
            Ok(Cmd::none())
        }
        LibraryEvent::Loaded { tracks, .. } => Ok(whole_library(&mut parts, tracks)),
        LibraryEvent::Listed { tracks, revision } => {
            Ok(listed_library(&mut parts, tracks, revision))
        }
        LibraryEvent::Tagged { tracks, .. } => Ok(tagged_tracks(&mut parts, tracks)),
        LibraryEvent::HistoryLoaded(entries) => {
            *parts.history = entries;
            Ok(Cmd::none())
        }
        LibraryEvent::Error(failure) => Ok(library_failed(&mut parts, &failure)),
    }
}

fn library_failed(parts: &mut LibraryParts<'_>, failure: &LibraryError) -> Cmd {
    match failure {
        LibraryError::NoUserDirs
        | LibraryError::Disk {
            subject: LibrarySubject::Scan,
            ..
        } => *parts.scan_status = ScanStatus::Idle,
        LibraryError::Disk { .. } | LibraryError::DecodeCover { .. } => {}
    }
    parts.workspace.show(
        Toast::error("Library error").with_text(failure.to_string()),
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
    listed: Vec<Arc<Track>>,
    revision: Revision,
) -> Cmd {
    *parts.scan_status = ScanStatus::Tagging {
        done: 0,
        total: listed.len(),
    };
    let tagging = tag_request(parts.music_dir, &listed, revision);
    library_loaded(parts, listed);
    tagging_progress(parts.scan_status, 0).then(tagging)
}

fn tag_request(music_dir: &Path, listed: &[Arc<Track>], revision: Revision) -> Cmd {
    if listed.is_empty() {
        return Cmd::none();
    }
    Effect::Library(LibraryCmd::TagTracks {
        music_dir: music_dir.to_path_buf(),
        tracks: listed.iter().map(|track| track.source().clone()).collect(),
        revision,
    })
    .into()
}

fn tagged_tracks(parts: &mut LibraryParts<'_>, tagged: Vec<Arc<Track>>) -> Cmd {
    let read = tagged.len();
    let tagged: Tagged = tagged
        .into_iter()
        .map(|track| (track.source().clone(), track))
        .collect();
    if let Some(ready) = parts.library {
        retag_tracks(&mut ready.tracks, &tagged);
    }
    retag_tracks(&mut parts.playlist.tracks, &tagged);
    retag_player(parts.player, &tagged);
    if let Some(Overlay::Search(search)) = parts.workspace.overlay.as_mut() {
        crate::update::overlay::search::rank(search, &parts.playlist.tracks);
    }
    tagging_progress(parts.scan_status, read)
}

fn library_loaded(parts: &mut LibraryParts<'_>, tracks: Vec<Arc<Track>>) {
    install_library(parts, tracks);
    if let Some(ready) = parts.library {
        crate::update::browse::resync_playlist(
            *parts.playlist_source,
            crate::update::browse::ResyncParts {
                library: ready,
                workspace: &mut *parts.workspace,
                player: parts.player,
                playlist: parts.playlist,
            },
        );
    }
}

type Tagged = HashMap<TrackRef, Arc<Track>>;

fn retag_tracks(tracks: &mut [Arc<Track>], tagged: &Tagged) {
    for track in tracks {
        if let Some(read) = tagged.get(track.source()) {
            *track = Arc::clone(read);
        }
    }
}

fn retag_player(player: &mut Player, tagged: &Tagged) {
    if let Player::Loading(track)
    | Player::Playing { track, .. }
    | Player::Paused { track, .. } = player
    {
        retag_tracks(std::slice::from_mut(track), tagged);
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
    let sort = parts.workspace.browse.sort;
    let view = sort_indices(&tracks, sort, parts.favorites);
    *parts.library = Some(Library { tracks, view });
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
            track::{Track, TrackRef},
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
                view: vec![TrackIndex::new(7)],
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
                .map(|t| t.path())
                .collect::<Vec<_>>()),
            Some(vec![
                std::path::Path::new("/music/a.flac"),
                std::path::Path::new("/music/b.flac")
            ])
        );
        assert_eq!(
            library.map(|ready| ready.view.as_slice()),
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
    fn a_library_error_raises_an_error_toast() {
        let mut model = Model::default();

        let cmd = update(
            crate::update::library_parts(&mut model),
            LibraryEvent::Error(LibraryError::NoUserDirs),
        )
        .unwrap();

        let toast = model.workspace.toasts.first().unwrap();
        assert_eq!(toast.kind, ToastLevel::Error);
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
            source: IoError::Missing,
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
        #[case] failure: LibraryError,
        #[case] expected: ScanStatus,
    ) {
        let mut model = Model {
            scan_status: ScanStatus::Scanning,
            ..Model::default()
        };

        let cmd = update(
            crate::update::library_parts(&mut model),
            LibraryEvent::Error(failure),
        );

        assert!(cmd.is_ok());
        assert_eq!(model.scan_status, expected);
    }

    #[test]
    fn a_loaded_history_replaces_the_history_and_emits_nothing() {
        let mut model = Model {
            history: vec![HistoryEntry {
                track: TrackRef::Local("/music/stale.flac".into()),
                title: "Stale".into(),
                artist: None,
                at: Moment::new(std::time::Duration::from_secs(1)),
            }],
            ..Default::default()
        };

        let fresh = vec![
            HistoryEntry {
                track: TrackRef::Local("/music/b.flac".into()),
                title: "B".into(),
                artist: Some("Artist".into()),
                at: Moment::new(std::time::Duration::from_secs(200)),
            },
            HistoryEntry {
                track: TrackRef::Local("/music/a.flac".into()),
                title: "A".into(),
                artist: None,
                at: Moment::new(std::time::Duration::from_secs(100)),
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
