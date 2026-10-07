use std::{path::PathBuf, sync::Arc};

use crate::domain::{
    driver::Drivers,
    favorites::Favorites,
    history::HistoryEntry,
    index::ViewIndex,
    library::Library,
    player::Player,
    playlist::{Playlist, PlaylistSource},
    revision::Revisions,
    settings::Settings,
    theme::Themes,
    track::{Track, TrackSource},
    transport::Transport,
    workspace::Workspace,
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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub workspace: Workspace,
    pub library: Option<Library>,
    pub music_dir: PathBuf,
    pub scan_status: ScanStatus,
    pub playlist: Playlist,
    pub playlist_source: PlaylistSource,
    pub queue: Vec<TrackSource>,
    pub player: Player,
    pub transport: Transport,
    pub history: Vec<HistoryEntry>,
    pub favorites: Favorites,
    pub settings: Settings,
    pub revisions: Revisions,
    pub themes: Themes,
    pub drivers: Drivers,
}

impl Model {
    #[must_use]
    pub fn displayed_track(&self) -> Option<&Arc<Track>> {
        self.player.current().or_else(|| {
            self.library.as_ref().and_then(|library| {
                library.view_track(self.workspace.browse.selected())
            })
        })
    }

    #[must_use]
    pub fn playing_index(&self) -> Option<ViewIndex> {
        let current = self.player.current()?;
        let index = self.playlist.playing_index()?;
        let at_index = self.playlist.current()?;
        (current.source() == at_index.source()).then_some(index)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::domain::{
        cursor::Cursor,
        index::{TrackIndex, ViewIndex},
        library::Library,
        model::Model,
        player::{PausedBy, Player},
        playhead::Playhead,
        playlist::Playlist,
        speed::Speed,
        time::Moment,
        track::{AudioFormat, Tags, Track, TrackParts},
        workspace::{Browse, Workspace},
    };

    fn titled_track(title: &str) -> Arc<Track> {
        Arc::new(Track::new(TrackParts {
            path: format!("{title}.mp3").into(),
            duration: Duration::from_secs(1),
            tags: Tags {
                title: Some(title.to_string()),
                ..Tags::default()
            },
            audio_format: AudioFormat::default(),
        }))
    }

    #[test]
    fn playing_track_wins_over_the_playlist_selection() {
        let mut model = Model {
            library: Some(Library {
                tracks: vec![titled_track("selected")],
                track_indexes: vec![TrackIndex::new(0)],
            }),
            ..Model::default()
        };
        model.player = Player::Playing {
            track: titled_track("playing"),
            playhead: Playhead::anchored(
                Duration::ZERO,
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        };
        model.workspace.browse.cursor = Cursor::at(1, 0);
        assert_eq!(
            model.displayed_track().map(|t| t.tags().title.clone()),
            Some(Some("playing".to_string()))
        );
    }

    #[test]
    fn stopped_falls_back_to_the_selected_playlist_track() {
        let model = Model {
            library: Some(Library {
                tracks: vec![titled_track("a"), titled_track("b")],
                track_indexes: vec![TrackIndex::new(0), TrackIndex::new(1)],
            }),
            workspace: Workspace {
                browse: Browse {
                    cursor: Cursor::at(2, 1),
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

    fn anchored_at_zero() -> Playhead {
        Playhead::anchored(Duration::ZERO, Moment::default(), Speed::default())
    }

    fn model_with(tracks: Vec<Arc<Track>>, index: Option<ViewIndex>) -> Model {
        Model {
            playlist: Playlist {
                cursor: Cursor::at(tracks.len(), index.map_or(0, ViewIndex::get)),
                tracks,
                ..Playlist::default()
            },
            ..Model::default()
        }
    }

    #[rstest]
    #[case::playing_reports_the_playlist_index(Player::Playing {
        track: titled_track("a"),
        playhead: anchored_at_zero(),
        preloaded: None,
    }, Some(ViewIndex::new(0)))]
    #[case::paused_reports_the_playlist_index(Player::Paused {
        track: titled_track("a"),
        position: Duration::ZERO,
        by: PausedBy::Listener,
    }, Some(ViewIndex::new(0)))]
    #[case::stopped_reports_none(Player::Stopped, None)]
    fn playing_index_reflects_the_player_state(
        #[case] player: Player,
        #[case] expected: Option<ViewIndex>,
    ) {
        let mut model = model_with(vec![titled_track("a")], Some(ViewIndex::new(0)));
        model.player = player;
        assert_eq!(model.playing_index(), expected);
    }

    #[test]
    fn a_rescan_that_reallocates_the_tracks_keeps_the_marker() {
        let mut model = model_with(vec![titled_track("a")], Some(ViewIndex::new(0)));
        model.player = Player::Playing {
            track: titled_track("a"),
            playhead: anchored_at_zero(),
            preloaded: None,
        };
        let rescanned = titled_track("a");
        assert!(!Arc::ptr_eq(&rescanned, &titled_track("a")));
        model.playlist.tracks = vec![rescanned];
        assert_eq!(model.playing_index(), Some(ViewIndex::new(0)));
    }

    #[test]
    fn player_track_differing_from_the_playlist_index_reports_none() {
        let mut model = model_with(
            vec![titled_track("a"), titled_track("b")],
            Some(ViewIndex::new(1)),
        );
        model.player = Player::Playing {
            track: titled_track("a"),
            playhead: anchored_at_zero(),
            preloaded: None,
        };
        assert_eq!(model.playing_index(), None);
    }
}
