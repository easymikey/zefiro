use std::{path::PathBuf, sync::Arc};

use crate::domain::{
    CustomSetting,
    Drivers,
    Favorites,
    History,
    Loaded,
    Player,
    PlaylistIndex,
    Revision,
    Settings,
    Track,
    Transport,
    Workspace,
    library::Library,
    playlist::{Playlist, PlaylistSource},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanStatus {
    #[default]
    Idle,
    Scanning,
    Tagging {
        done: usize,
        total: usize,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub workspace: Workspace,
    pub library: Loaded<Library>,
    pub music_dir: PathBuf,
    pub scan_status: ScanStatus,
    pub playlist: Playlist,
    pub playlist_source: PlaylistSource,
    pub queue: Vec<PlaylistIndex>,
    pub player: Player,
    pub transport: Transport,
    pub history: History,
    pub favorites: Favorites,
    pub settings: Settings,
    pub custom_rows: Vec<CustomSetting>,
    pub config_generation: Revision,
    pub theme_generation: Revision,
    pub scan_generation: Revision,
    pub toast_generation: Revision,
    pub sleep_generation: Revision,
    pub effects: Revision,
    pub themes: Vec<String>,
    pub drivers: Drivers,
}

impl Model {
    #[must_use]
    pub fn displayed_track(&self) -> Option<&Arc<Track>> {
        self.player
            .current()
            .or_else(|| self.library.view_track(self.workspace.browse.selected()))
    }

    #[must_use]
    pub fn playing_index(&self) -> Option<PlaylistIndex> {
        let current = self.player.current()?;
        let index = self.playlist.anchor()?;
        let at_index = self.playlist.current()?;
        (current.path() == at_index.path()).then_some(index)
    }
}

#[cfg(test)]
fn titled_track(title: &str) -> Arc<Track> {
    Arc::new(
        Track::builder()
            .path(format!("{title}.mp3"))
            .duration(std::time::Duration::from_secs(1))
            .tags(crate::domain::Tags {
                title: Some(title.to_string()),
                ..crate::domain::Tags::default()
            })
            .audio_format(crate::domain::AudioFormat::default())
            .build(),
    )
}

#[cfg(test)]
mod displayed_track_tests {
    use std::time::Duration;

    use crate::domain::{
        Browse,
        Cursor,
        Loaded,
        Player,
        Preload,
        TrackIndex,
        Workspace,
        library::Library,
        model::{Model, titled_track},
    };

    #[test]
    fn playing_track_wins_over_the_playlist_selection() {
        let mut model = Model {
            library: Loaded::Ready(Library {
                all: vec![titled_track("selected")],
                view: vec![TrackIndex::new(0)],
            }),
            ..Model::default()
        };
        model.player = Player::Playing {
            track: titled_track("playing"),
            at: Duration::ZERO,
            preload: Preload::None,
        };
        model.workspace.browse.cursor = Cursor::with_len(1).at(0);
        assert_eq!(
            model.displayed_track().map(|t| t.tags().title.clone()),
            Some(Some("playing".to_string()))
        );
    }

    #[test]
    fn stopped_falls_back_to_the_selected_playlist_track() {
        let model = Model {
            library: Loaded::Ready(Library {
                all: vec![titled_track("a"), titled_track("b")],
                view: vec![TrackIndex::new(0), TrackIndex::new(1)],
            }),
            workspace: Workspace {
                browse: Browse {
                    cursor: Cursor::with_len(2).at(1),
                    ..Browse::default()
                },
                ..Workspace::default()
            },
            ..Model::default()
        };
        assert_eq!(
            model.displayed_track().map(|t| t.tags().title.clone()),
            Some(Some("b".to_string()))
        );
    }

    #[test]
    fn stopped_with_an_empty_playlist_shows_nothing() {
        let model = Model::default();
        assert_eq!(model.displayed_track(), None);
    }
}

#[cfg(test)]
mod playing_index_tests {
    use std::{sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::domain::{
        Cursor,
        Pause,
        Player,
        PlaylistIndex,
        Preload,
        Track,
        model::{Model, titled_track},
        playlist::Playlist,
    };

    fn model_with(tracks: Vec<Arc<Track>>, index: Option<PlaylistIndex>) -> Model {
        Model {
            playlist: Playlist {
                at: Cursor::with_len(tracks.len())
                    .at(index.map_or(0, PlaylistIndex::get)),
                tracks,
                ..Playlist::default()
            },
            ..Model::default()
        }
    }

    #[rstest]
    #[case::playing_reports_the_playlist_index(Player::Playing {
        track: titled_track("a"),
        at: Duration::ZERO,
        preload: Preload::None,
    }, Some(PlaylistIndex::new(0)))]
    #[case::paused_reports_the_playlist_index(Player::Paused {
        track: titled_track("a"),
        at: Duration::ZERO,
        pause: Pause::ByListener,
    }, Some(PlaylistIndex::new(0)))]
    #[case::stopped_reports_none(Player::Stopped, None)]
    fn playing_index_reflects_the_player_state(
        #[case] player: Player,
        #[case] expected: Option<PlaylistIndex>,
    ) {
        let mut model =
            model_with(vec![titled_track("a")], Some(PlaylistIndex::new(0)));
        model.player = player;
        assert_eq!(model.playing_index(), expected);
    }

    #[test]
    fn a_rescan_that_reallocates_the_tracks_keeps_the_marker() {
        let mut model =
            model_with(vec![titled_track("a")], Some(PlaylistIndex::new(0)));
        model.player = Player::Playing {
            track: titled_track("a"),
            at: Duration::ZERO,
            preload: Preload::None,
        };
        let rescanned = titled_track("a");
        assert!(!Arc::ptr_eq(&rescanned, &titled_track("a")));
        model.playlist.tracks = vec![rescanned];
        assert_eq!(model.playing_index(), Some(PlaylistIndex::new(0)));
    }

    #[test]
    fn player_track_differing_from_the_playlist_index_reports_none() {
        let mut model = model_with(
            vec![titled_track("a"), titled_track("b")],
            Some(PlaylistIndex::new(1)),
        );
        model.player = Player::Playing {
            track: titled_track("a"),
            at: Duration::ZERO,
            preload: Preload::None,
        };
        assert_eq!(model.playing_index(), None);
    }
}
