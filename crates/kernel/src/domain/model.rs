use std::{collections::HashMap, path::PathBuf, sync::Arc};

use crate::domain::{
    catalog::{Catalog, CatalogName},
    driver::Drivers,
    favorites::Favorites,
    history::HistoryEntry,
    index::ViewIndex,
    library::Library,
    player::Player,
    playlist::{Playlist, PlaylistSource},
    revision::Revisions,
    server::{Artwork, Download, PlayReport, Server},
    settings::Settings,
    theme::Themes,
    track::Track,
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
    pub queue: Vec<Arc<Track>>,
    pub player: Player,
    pub transport: Transport,
    pub history: Vec<HistoryEntry>,
    pub favorites: Favorites,
    pub settings: Settings,
    pub revisions: Revisions,
    pub themes: Themes,
    pub drivers: Drivers,
    pub servers: Vec<Server>,
    pub downloads: Vec<Download>,
    pub catalog_name: CatalogName,
    pub catalogs: Vec<Catalog>,
    pub play_reports: Vec<PlayReport>,
    pub covers: HashMap<Artwork, PathBuf>,
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
        if self.playlist_source.server_name().is_some() {
            return None;
        }
        let current = self.player.current()?;
        let index = self.playlist.playing_index()?;
        let at_index = self.playlist.current()?;
        (current.source() == at_index.source()).then_some(index)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use crate::domain::{
        cursor::Cursor,
        index::{TrackIndex, ViewIndex},
        library::Library,
        model::Model,
        player::{PausedBy, Player},
        playlist::Playlist,
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
    fn a_paused_track_keeps_its_playing_index() {
        let model = Model {
            playlist: Playlist {
                cursor: Cursor::at(1, 0),
                tracks: vec![titled_track("a")],
                ..Playlist::default()
            },
            player: Player::Paused {
                track: titled_track("a"),
                position: Duration::ZERO,
                by: PausedBy::Listener,
            },
            ..Model::default()
        };
        assert_eq!(model.playing_index(), Some(ViewIndex::new(0)));
    }
}
