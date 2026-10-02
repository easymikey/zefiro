use std::{collections::HashMap, sync::Arc};

use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd},
    domain::{
        Freshness,
        Model,
        Moment,
        Player,
        Revision,
        ScanStatus,
        Toast,
        Track,
        TrackIndex,
        library::Library,
        playlist::PlaylistSource,
    },
    message::{LibraryEvent, PlaylistRequest},
    update::{audio, error::UpdateError},
};

pub(crate) fn update(
    model: &mut Model,
    PlaylistRequest::JumpTo(index): PlaylistRequest,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    audio::jump_to(&mut crate::update::playback_parts(model), index, now)
}

pub(crate) fn library(
    model: &mut Model,
    event: LibraryEvent,
) -> Result<Cmd, UpdateError> {
    if let LibraryEvent::Loaded { revision, .. }
    | LibraryEvent::Listed { revision, .. }
    | LibraryEvent::Tagged { revision, .. } = &event
        && let Freshness::Stale = revision.reply(model.revisions.scan)
    {
        return Ok(Cmd::None);
    }
    match event {
        LibraryEvent::FavoritesLoaded(favorites) => {
            model.favorites = favorites;
            Ok(Cmd::None)
        }
        LibraryEvent::Loaded { tracks, .. } => Ok(whole_library(model, tracks)),
        LibraryEvent::Listed { tracks, revision } => {
            Ok(listed_library(model, tracks, revision))
        }
        LibraryEvent::Tagged { tracks, .. } => Ok(tagged_tracks(model, tracks)),
        LibraryEvent::HistoryLoaded(entries) => {
            model.history = entries;
            Ok(Cmd::None)
        }
        LibraryEvent::Error(failure) => Ok(model.workspace.show(
            Toast::error("Library error").with_text(failure.to_string()),
            &mut model.revisions,
        )),
    }
}

fn whole_library(model: &mut Model, tracks: Vec<Arc<Track>>) -> Cmd {
    model.scan_status = ScanStatus::Idle;
    let opening = match &model.library {
        None => Cmd::from(Cue::LibraryOpened),
        Some(_) => Cmd::None,
    };
    library_loaded(model, tracks);
    opening
}

fn listed_library(
    model: &mut Model,
    listed: Vec<Arc<Track>>,
    revision: Revision,
) -> Cmd {
    model.scan_status = ScanStatus::Tagging {
        done: 0,
        total: listed.len(),
    };
    let tagging = tag_request(&model.music_dir, &listed, revision);
    library_loaded(model, listed);
    tagging_progress(&mut model.scan_status, 0).then(tagging)
}

fn tag_request(
    music_dir: &std::path::Path,
    listed: &[Arc<Track>],
    revision: Revision,
) -> Cmd {
    if listed.is_empty() {
        return Cmd::None;
    }
    Effect::Library(LibraryCmd::TagTracks {
        music_dir: music_dir.to_path_buf(),
        paths: listed
            .iter()
            .map(|track| track.path().to_path_buf())
            .collect(),
        revision,
    })
    .into()
}

fn tagged_tracks(model: &mut Model, tagged: Vec<Arc<Track>>) -> Cmd {
    let read = tagged.len();
    let tagged: Tagged = tagged
        .into_iter()
        .map(|track| (track.path().to_path_buf(), track))
        .collect();
    if let Some(ready) = &mut model.library {
        retag_tracks(&mut ready.tracks, &tagged);
    }
    retag_tracks(&mut model.playlist.tracks, &tagged);
    retag_player(&mut model.player, &tagged);
    tagging_progress(&mut model.scan_status, read)
}

fn library_loaded(model: &mut Model, tracks: Vec<Arc<Track>>) {
    install_library(&mut model.library, tracks);
    match model.playlist_source {
        PlaylistSource::Named => {}
        PlaylistSource::Library => {
            if let Some(ready) = &mut model.library {
                crate::update::browse::resync_playlist(
                    crate::update::browse::ResyncParts {
                        library: ready,
                        player: &model.player,
                        playlist: &mut model.playlist,
                        queue: &mut model.queue,
                    },
                );
            }
            let browse = &mut model.workspace.browse;
            browse.cursor = browse.cursor.resize(model.playlist.tracks.len());
        }
    }
}

type Tagged = HashMap<std::path::PathBuf, Arc<Track>>;

fn retag_tracks(tracks: &mut [Arc<Track>], tagged: &Tagged) {
    for track in tracks {
        if let Some(read) = tagged.get(track.path()) {
            *track = Arc::clone(read);
        }
    }
}

fn retag_player(player: &mut Player, tagged: &Tagged) {
    if let Player::Loading { track, .. }
    | Player::Playing { track, .. }
    | Player::Paused { track, .. } = player
    {
        retag_tracks(std::slice::from_mut(track), tagged);
    }
}

fn tagging_progress(scan_status: &mut ScanStatus, read: usize) -> Cmd {
    let ScanStatus::Tagging { done, total } = *scan_status else {
        return Cmd::None;
    };
    let done = done.saturating_add(read).min(total);
    if done >= total {
        *scan_status = ScanStatus::Idle;
        return Cmd::from(Cue::LibraryOpened);
    }
    *scan_status = ScanStatus::Tagging { done, total };
    Cmd::None
}

fn install_library(library: &mut Option<Library>, tracks: Vec<Arc<Track>>) {
    let view = (0..tracks.len()).map(TrackIndex::new).collect();
    *library = Some(Library { tracks, view });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        cmd::{Cmd, Cue, Effect},
        domain::{
            HistoryEntry,
            Model,
            Moment,
            Revision,
            TOAST_LIFETIME,
            ToastKind,
            Track,
            TrackIndex,
            library::Library,
            playlist::PlaylistSource,
        },
        message::{LibraryError, LibraryEvent, Timer},
        update::library::library,
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

        let cmd = library(
            &mut model,
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
        assert!(matches!(cmd, Cmd::None));
    }

    #[test]
    fn a_named_playlist_survives_a_full_scan() {
        let named = vec![track("/elsewhere/one.flac")];
        let mut model = Model {
            playlist_source: PlaylistSource::Named,
            playlist: crate::domain::playlist::Playlist::from_tracks(named.clone()),
            ..Model::default()
        };

        let cmd = library(
            &mut model,
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
        assert!(matches!(cmd, Cmd::One(Effect::Animate(Cue::LibraryOpened))));
    }

    #[test]
    fn a_library_error_raises_an_error_toast() {
        let mut model = Model::default();

        let cmd =
            library(&mut model, LibraryEvent::Error(LibraryError::NoUserDirs)).unwrap();

        let toast = model.workspace.toasts.first().unwrap();
        assert_eq!(toast.kind, ToastKind::Error);
        assert_eq!(
            toast.text.as_deref(),
            Some(LibraryError::NoUserDirs.to_string().as_str())
        );
        assert_eq!(
            cmd,
            Cmd::Batch(vec![
                Effect::Animate(Cue::ToastRaised),
                Effect::After {
                    delay: TOAST_LIFETIME,
                    timer: Timer::Toast(Revision::default().next()),
                },
            ])
        );
    }

    #[test]
    fn a_loaded_history_replaces_the_history_and_emits_nothing() {
        let mut model = Model {
            history: vec![HistoryEntry {
                path: "/music/stale.flac".into(),
                title: "Stale".into(),
                artist: None,
                at: Moment::new(std::time::Duration::from_secs(1)),
            }],
            ..Default::default()
        };

        let fresh = vec![
            HistoryEntry {
                path: "/music/b.flac".into(),
                title: "B".into(),
                artist: Some("Artist".into()),
                at: Moment::new(std::time::Duration::from_secs(200)),
            },
            HistoryEntry {
                path: "/music/a.flac".into(),
                title: "A".into(),
                artist: None,
                at: Moment::new(std::time::Duration::from_secs(100)),
            },
        ];
        let cmd =
            library(&mut model, LibraryEvent::HistoryLoaded(fresh.clone())).unwrap();

        assert_eq!(model.history, fresh);
        assert!(matches!(cmd, Cmd::None));
    }
}
