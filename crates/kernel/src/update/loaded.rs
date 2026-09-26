use std::{collections::HashMap, sync::Arc};

use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd},
    domain::{
        Loaded,
        Model,
        Player,
        PlaylistIndex,
        Reply,
        Revision,
        ScanStatus,
        Toast,
        Track,
        TrackIndex,
        Transport,
        Workspace,
        library::Library,
        playlist::{self, Playlist, PlaylistSource},
    },
    message::{LibraryFact, LibraryFailure, LoadedRequest, WorkspaceRequest},
    update::{
        audio,
        machine::Machine,
        playlist::PlaylistMessage,
        rejection::Rejection,
    },
};

struct PlaylistJump<'a> {
    playlist: &'a mut Playlist,
    transport: &'a mut Transport,
    player: &'a mut Player,
}

pub(super) fn loaded(
    model: &mut Model,
    message: LoadedRequest,
) -> Result<Cmd, Rejection> {
    match message {
        LoadedRequest::Jump(index) => jump(
            PlaylistJump {
                playlist: &mut model.playlist,
                transport: &mut model.transport,
                player: &mut model.player,
            },
            index,
        ),
        LoadedRequest::ShuffleRolled(order) => Ok(model
            .playlist
            .update(PlaylistMessage::ShuffleRolled(order))?),
    }
}

pub(crate) fn library(model: &mut Model, fact: LibraryFact) -> Result<Cmd, Rejection> {
    match fact {
        LibraryFact::FavoritesLoaded(favorites) => {
            model.favorites = favorites.into();
            Ok(Cmd::None)
        }
        LibraryFact::Loaded { tracks, revision } => {
            Ok(whole_library(model, tracks, revision))
        }
        LibraryFact::Listed { tracks, revision } => {
            Ok(listed_library(model, tracks, revision))
        }
        LibraryFact::Tagged { tracks, revision } => {
            Ok(tagged_tracks(model, tracks, revision))
        }
        LibraryFact::HistoryLoaded(entries) => {
            model.history.view = entries;
            Ok(Cmd::None)
        }
        LibraryFact::Failed(failure) => failed(model, &failure),
    }
}

fn failed(model: &mut Model, failure: &LibraryFailure) -> Result<Cmd, Rejection> {
    Ok(model
        .workspace
        .update(WorkspaceRequest::ShowToast(Toast::error(
            failure.to_string(),
        )))?)
}

fn whole_library(
    model: &mut Model,
    tracks: Vec<Arc<Track>>,
    revision: Revision,
) -> Cmd {
    if let Reply::Stale = revision.reply(model.scan_generation) {
        return Cmd::None;
    }
    model.scan_status = ScanStatus::Idle;
    let opening = match &model.library {
        Loaded::Loading => Cmd::from(Cue::LibraryOpened),
        Loaded::Ready(_) => Cmd::None,
    };
    let source = model.playlist_source;
    let cmd = library_loaded(scan_landing(model), source, tracks);
    cmd.then(opening)
}

fn listed_library(
    model: &mut Model,
    listed: Vec<Arc<Track>>,
    revision: Revision,
) -> Cmd {
    if let Reply::Stale = revision.reply(model.scan_generation) {
        return Cmd::None;
    }
    model.scan_status = ScanStatus::Tagging {
        done: 0,
        total: listed.len(),
    };
    let tagging = tag_request(&model.music_dir, &listed, revision);
    let source = model.playlist_source;
    let cmd = library_loaded(scan_landing(model), source, listed);
    cmd.then(tagging_progress(&mut model.scan_status, 0))
        .then(tagging)
}

fn tag_request(
    root: &std::path::Path,
    listed: &[Arc<Track>],
    revision: Revision,
) -> Cmd {
    if listed.is_empty() {
        return Cmd::None;
    }
    Effect::Library(LibraryCmd::TagTracks {
        root: root.to_path_buf(),
        paths: listed
            .iter()
            .map(|track| track.path().to_path_buf())
            .collect(),
        revision,
    })
    .into()
}

fn tagged_tracks(
    model: &mut Model,
    tagged: Vec<Arc<Track>>,
    revision: Revision,
) -> Cmd {
    if let Reply::Stale = revision.reply(model.scan_generation) {
        return Cmd::None;
    }
    let read = tagged.len();
    retag(
        TaggedLanding {
            library: &mut model.library,
            playlist: &mut model.playlist,
            player: &mut model.player,
        },
        tagged,
    );
    tagging_progress(&mut model.scan_status, read)
}

fn scan_landing(model: &mut Model) -> ScanLanding<'_> {
    ScanLanding {
        library: &mut model.library,
        playlist: &mut model.playlist,
        queue: &mut model.queue,
        player: &model.player,
        workspace: &mut model.workspace,
    }
}

struct ScanLanding<'a> {
    library: &'a mut Loaded<Library>,
    playlist: &'a mut Playlist,
    queue: &'a mut Vec<PlaylistIndex>,
    player: &'a Player,
    workspace: &'a mut Workspace,
}

fn library_loaded(
    landing: ScanLanding<'_>,
    source: PlaylistSource,
    tracks: Vec<Arc<Track>>,
) -> Cmd {
    let ScanLanding {
        library,
        playlist,
        queue,
        player,
        workspace,
    } = landing;
    install_library(library, tracks);
    view_all(library);
    match source {
        PlaylistSource::Named => {}
        PlaylistSource::Library => {
            if let Loaded::Ready(ready) = library {
                crate::update::browse::resync_playlist(
                    crate::update::browse::TrashRequest {
                        library: ready,
                        player,
                        playlist,
                        queue,
                    },
                );
            }
            workspace.browse.cursor =
                workspace.browse.cursor.resize(playlist.tracks.len());
        }
    }
    Cmd::None
}

struct TaggedLanding<'a> {
    library: &'a mut Loaded<Library>,
    playlist: &'a mut Playlist,
    player: &'a mut Player,
}

type Tagged = HashMap<std::path::PathBuf, Arc<Track>>;

fn retag(landing: TaggedLanding<'_>, tracks: Vec<Arc<Track>>) {
    let TaggedLanding {
        library,
        playlist,
        player,
    } = landing;
    let tagged: Tagged = tracks
        .into_iter()
        .map(|track| (track.path().to_path_buf(), track))
        .collect();
    if let Loaded::Ready(ready) = library {
        retag_tracks(&mut ready.all, &tagged);
    }
    retag_tracks(&mut playlist.tracks, &tagged);
    retag_player(player, &tagged);
}

fn retag_tracks(tracks: &mut [Arc<Track>], tagged: &Tagged) {
    for track in tracks {
        if let Some(read) = tagged.get(track.path()) {
            *track = Arc::clone(read);
        }
    }
}

fn retag_player(player: &mut Player, tagged: &Tagged) {
    match player {
        Player::Stopped => {}
        Player::Loading { track, .. }
        | Player::Playing { track, .. }
        | Player::Paused { track, .. } => {
            if let Some(read) = tagged.get(track.path()) {
                *track = Arc::clone(read);
            }
        }
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

fn install_library(library: &mut Loaded<Library>, tracks: Vec<Arc<Track>>) {
    match library {
        Loaded::Ready(existing) => existing.all = tracks,
        Loaded::Loading => {
            *library = Loaded::Ready(Library {
                all: tracks,
                view: Vec::new(),
            });
        }
    }
}

fn view_all(library: &mut Loaded<Library>) {
    if let Loaded::Ready(library) = library {
        library.view = (0..library.all.len()).map(TrackIndex::new).collect();
    }
}

fn jump(slices: PlaylistJump<'_>, index: PlaylistIndex) -> Result<Cmd, Rejection> {
    let PlaylistJump {
        playlist,
        transport,
        player,
    } = slices;
    playlist::jump(playlist, index)
        .cloned()
        .map_or(Ok(Cmd::None), |track| {
            audio::start(transport, player, track)
        })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        cmd::{Cmd, Cue, Effect},
        domain::{
            History,
            HistoryEntry,
            Loaded,
            Model,
            Revision,
            TOAST_LIFETIME,
            ToastLevel,
            Track,
            TrackIndex,
            library::Library,
            playlist::PlaylistSource,
        },
        message::{LibraryFact, LibraryFailure, Timer},
        update::loaded::library,
    };

    fn track(path: &str) -> Arc<Track> {
        Arc::new(Track::listed(std::path::Path::new(path)))
    }

    #[test]
    fn library_loaded_replaces_the_library_and_mirrors_it_into_the_playlist() {
        let mut model = Model {
            library: Loaded::Ready(Library {
                all: Vec::new(),
                view: vec![TrackIndex::new(7)],
            }),
            ..Model::default()
        };

        let cmd = library(
            &mut model,
            LibraryFact::Loaded {
                tracks: vec![track("/music/a.flac"), track("/music/b.flac")],
                revision: Revision::default(),
            },
        )
        .unwrap();

        let library = model.library.ready();
        assert_eq!(
            library.map(|ready| ready.all.iter().map(|t| t.path()).collect::<Vec<_>>()),
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
    fn a_named_playlist_survives_a_rescan() {
        let named = vec![track("/elsewhere/one.flac")];
        let mut model = Model {
            playlist_source: PlaylistSource::Named,
            playlist: crate::domain::playlist::Playlist::from_tracks(named.clone()),
            ..Model::default()
        };

        let cmd = library(
            &mut model,
            LibraryFact::Loaded {
                tracks: vec![track("/music/a.flac"), track("/music/b.flac")],
                revision: Revision::default(),
            },
        )
        .unwrap();

        assert_eq!(model.playlist.tracks, named);
        assert_eq!(model.library.ready().map(|ready| ready.all.len()), Some(2));
        assert!(matches!(cmd, Cmd::One(Effect::Animate(Cue::LibraryOpened))));
    }

    #[test]
    fn a_library_failure_raises_an_error_toast() {
        let mut model = Model::default();

        let cmd = library(&mut model, LibraryFact::Failed(LibraryFailure::NoDirectory))
            .unwrap();

        let toast = model.workspace.toast.unwrap();
        assert_eq!(toast.level, ToastLevel::Error);
        assert_eq!(toast.text, LibraryFailure::NoDirectory.to_string());
        assert_eq!(
            cmd,
            Cmd::Batch(vec![
                Effect::Animate(Cue::ToastRaised),
                Effect::After {
                    delay: TOAST_LIFETIME,
                    message: Timer::Toast(Revision::UNSTAMPED),
                },
            ])
        );
    }

    #[test]
    fn set_history_replaces_history_view_and_emits_nothing() {
        let mut model = Model {
            history: History {
                view: vec![HistoryEntry {
                    path: "/music/stale.flac".into(),
                    title: "Stale".into(),
                    artist: None,
                    at: 1,
                }],
                ..Default::default()
            },
            ..Default::default()
        };

        let fresh = vec![
            HistoryEntry {
                path: "/music/b.flac".into(),
                title: "B".into(),
                artist: Some("Artist".into()),
                at: 200,
            },
            HistoryEntry {
                path: "/music/a.flac".into(),
                title: "A".into(),
                artist: None,
                at: 100,
            },
        ];
        let cmd =
            library(&mut model, LibraryFact::HistoryLoaded(fresh.clone())).unwrap();

        assert_eq!(model.history.view, fresh);
        assert!(matches!(cmd, Cmd::None));
    }
}
