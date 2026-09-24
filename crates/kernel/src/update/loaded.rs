use std::{collections::HashMap, sync::Arc};

use crate::{
    cmd::{Cmd, Cue, Effect, LibraryCmd},
    domain::{
        CustomSetting,
        Loaded,
        Model,
        Overlay,
        Player,
        PlaylistIndex,
        Reply,
        Revision,
        ScanStatus,
        SettingRow,
        Toast,
        Track,
        TrackIndex,
        Transport,
        Workspace,
        library::Library,
        playlist::{self, Playlist, PlaylistSource},
    },
    message::{LibraryFailure, LoadedRequest, WorkspaceRequest},
    update::{
        audio,
        machine::Machine,
        overlay::selected_setting_row,
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
        LoadedRequest::FavoritesLoaded(favorites) => {
            model.favorites = favorites.into();
            Ok(Cmd::None)
        }
        LoadedRequest::LibraryLoaded { tracks, revision } => {
            Ok(whole_library(model, tracks, revision))
        }
        LoadedRequest::LibraryListed { tracks, revision } => {
            Ok(listed_library(model, tracks, revision))
        }
        LoadedRequest::TracksTagged { tracks, revision } => {
            Ok(tagged_tracks(model, tracks, revision))
        }
        LoadedRequest::HistoryLoaded(entries) => {
            model.history.view = entries;
            Ok(Cmd::None)
        }
        LoadedRequest::ThemesLoaded(themes) => {
            model.themes = themes;
            Ok(Cmd::None)
        }
        LoadedRequest::MusicDirReloaded(reloaded) => {
            Ok(music_dir_reloaded(&mut model.music_dir, reloaded))
        }
        LoadedRequest::CustomRowsReloaded(rows) => {
            reload_custom_rows(model, rows);
            Ok(Cmd::None)
        }
        LoadedRequest::Failed(failure) => failed(model, &failure),
    }
}

fn reload_custom_rows(model: &mut Model, rows: Vec<CustomSetting>) {
    let previous = selected_setting_row(model);
    model.custom_rows = rows;
    let Model {
        workspace,
        custom_rows,
        ..
    } = model;
    let Some(Overlay::Settings(cursor)) = &mut workspace.overlay else {
        return;
    };
    let all = SettingRow::all(custom_rows);
    cursor.resize(all.len());
    if let Some(row) = previous
        && let Some(index) = all.iter().position(|candidate| *candidate == row)
    {
        cursor.select(index);
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

fn music_dir_reloaded(
    music_dir: &mut std::path::PathBuf,
    reloaded: std::path::PathBuf,
) -> Cmd {
    if reloaded == *music_dir {
        Cmd::None
    } else {
        *music_dir = reloaded.clone();
        Effect::Library(LibraryCmd::Rescan {
            root: reloaded,
            revision: Revision::UNSTAMPED,
        })
        .into()
    }
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
    use std::{path::PathBuf, sync::Arc};

    use crate::{
        cmd::{Cmd, Cue, Effect, LibraryCmd},
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
        message::{LibraryFailure, LoadedRequest, Timer},
        update::loaded::loaded,
    };

    fn track(path: &str) -> Arc<Track> {
        Arc::new(Track::listed(std::path::Path::new(path)))
    }

    #[test]
    fn setting_themes_installs_the_list_the_shell_found() {
        let mut model = Model {
            themes: vec!["noir".to_string()],
            ..Model::default()
        };

        let cmd = loaded(
            &mut model,
            LoadedRequest::ThemesLoaded(vec!["wafer".to_string()]),
        )
        .unwrap();

        assert_eq!(model.themes, ["wafer".to_string()]);
        assert!(matches!(cmd, Cmd::None));
    }

    fn rescanned_root(cmd: &Cmd) -> Option<PathBuf> {
        match cmd {
            Cmd::One(Effect::Library(LibraryCmd::Rescan { root, .. })) => {
                Some(root.clone())
            }
            Cmd::None | Cmd::One(_) | Cmd::Batch(_) => None,
        }
    }

    #[rstest::rstest]
    #[case::the_root_it_already_plays(PathBuf::from("/music"), None)]
    #[case::another_root(PathBuf::from("/other"), Some(PathBuf::from("/other")))]
    fn a_music_dir_reload_rescans_only_a_root_that_moved(
        #[case] reloaded: PathBuf,
        #[case] expected: Option<PathBuf>,
    ) {
        let mut model = Model {
            music_dir: PathBuf::from("/music"),
            ..Model::default()
        };

        let cmd = loaded(
            &mut model,
            LoadedRequest::MusicDirReloaded(reloaded.clone()),
        )
        .unwrap();

        assert_eq!(rescanned_root(&cmd), expected);
        assert_eq!(model.music_dir, reloaded);
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

        let cmd = loaded(
            &mut model,
            LoadedRequest::LibraryLoaded {
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

        let cmd = loaded(
            &mut model,
            LoadedRequest::LibraryLoaded {
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

        let cmd = loaded(
            &mut model,
            LoadedRequest::Failed(LibraryFailure::NoDirectory),
        )
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
            loaded(&mut model, LoadedRequest::HistoryLoaded(fresh.clone())).unwrap();

        assert_eq!(model.history.view, fresh);
        assert!(matches!(cmd, Cmd::None));
    }
}
