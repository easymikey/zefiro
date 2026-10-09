use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use kernel::{
    domain::{
        appearance::{Appearance, CoverMode},
        catalog::{Catalog, CatalogName},
        device::DeviceName,
        favorites::Favorites,
        history::HistoryEntry,
        index::ViewIndex,
        model::{Model, ScanStatus},
        overlay::Overlay,
        player::Player,
        playlist::{Playlist, PlaylistRows, PlaylistSource},
        revision::Revisions,
        server::{Artwork, Server},
        settings::Settings,
        startup::Shuffle,
        theme::{ThemeChoice, Themes},
        time::Moment,
        toast::Toast,
        track::Track,
        transport::Transport,
    },
    update::keymap::chord::KeyBinding,
};

use crate::{
    card::CardView,
    geometry::{CoverSizing, cover_sizing},
    key_hints::{KeyHintChords, KeyHintsView},
    overlay::{layer::OverlayView, settings::view::SettingsView},
    playlist::view::{CatalogView, LibraryStatus, PlaylistView},
    primitive::spinner::Spinner,
    spectrum::Spectrum,
    status_line::StatusLineView,
    theme::{Theme, active_theme::ActiveTheme, rgb::ColorDepth},
    toast::ToastWidget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelPath {
    Protocol,
    Halfblocks,
}

#[derive(Debug, Clone, Copy)]
pub struct ScenePresentation<'a> {
    pub appearance: &'a Appearance,
    pub theme: &'a Theme,
    pub color_depth: ColorDepth,
    pub spectrum: &'a Spectrum,
    pub pixel_path: PixelPath,
    pub cell_aspect: f32,
    pub since_first_paint: Duration,
    pub now: Moment,
    pub home_dir: Option<&'a Path>,
    pub key_hint_chords: &'a KeyHintChords,
}

#[derive(Debug, Clone, Copy)]
pub struct Scene<'a> {
    pub player: &'a Player,
    pub transport: &'a Transport,
    pub playlist: &'a Playlist,
    pub playlist_source: &'a PlaylistSource,
    pub rows: PlaylistRows<'a>,
    pub queue: &'a [Arc<Track>],
    pub favorites: &'a Favorites,
    pub themes: &'a Themes,
    pub settings: &'a Settings,
    pub revisions: &'a Revisions,
    pub overlay: Option<&'a Overlay>,
    pub history: &'a [HistoryEntry],
    pub toasts: &'a [Toast],
    pub selected: ViewIndex,
    pub playing_index: Option<ViewIndex>,
    pub displayed_track: Option<&'a Arc<Track>>,
    pub library_status: LibraryStatus,
    pub scan_status: ScanStatus,
    pub bindings: &'a [KeyBinding],
    pub music_dir: &'a Path,
    pub servers: &'a [Server],
    pub catalog_name: &'a CatalogName,
    pub catalogs: &'a [Catalog],
    pub covers: &'a HashMap<Artwork, PathBuf>,
    pub presentation: ScenePresentation<'a>,
}

impl<'a> Scene<'a> {
    #[must_use]
    pub fn from_model(model: &'a Model, presentation: ScenePresentation<'a>) -> Self {
        Self {
            player: &model.player,
            transport: &model.transport,
            playlist: &model.playlist,
            playlist_source: &model.playlist_source,
            rows: PlaylistRows::new(
                &model.playlist_source,
                model.library.as_ref(),
                &model.playlist,
            ),
            queue: &model.queue,
            favorites: &model.favorites,
            themes: &model.themes,
            settings: &model.settings,
            revisions: &model.revisions,
            overlay: model.workspace.overlay.as_ref(),
            history: &model.history,
            toasts: &model.workspace.toasts,
            selected: ViewIndex::new(model.workspace.browse.selected().get()),
            playing_index: model.playing_index(),
            displayed_track: model.displayed_track(),
            library_status: if model.library.is_none() {
                LibraryStatus::Loading
            } else {
                LibraryStatus::Ready
            },
            scan_status: model.scan_status,
            bindings: model.workspace.keymap.bindings(),
            music_dir: &model.music_dir,
            servers: &model.servers,
            catalog_name: &model.catalog_name,
            catalogs: &model.catalogs,
            covers: &model.covers,
            presentation,
        }
    }

    #[must_use]
    pub fn current_track_path(&self) -> Option<&'a Path> {
        self.player.current().and_then(|track| {
            track.local_path().or_else(|| {
                track
                    .artwork()
                    .and_then(|artwork| self.covers.get(artwork))
                    .map(PathBuf::as_path)
            })
        })
    }

    #[must_use]
    pub(crate) fn active_theme(&self) -> ActiveTheme<'a> {
        ActiveTheme {
            spinner: Spinner::new(self.presentation.since_first_paint),
            ..ActiveTheme::new(self.presentation.theme, self.presentation.color_depth)
                .with_progress_bar(self.presentation.appearance.progress_bar)
        }
    }

    #[must_use]
    pub fn cover_mode(&self) -> CoverMode {
        painted_cover_mode(
            self.settings.appearance_settings.cover_mode,
            self.presentation.pixel_path,
        )
    }

    #[must_use]
    pub(crate) fn cover_sizing(&self) -> CoverSizing {
        cover_sizing(self.cover_mode(), self.presentation.appearance.cover_cells)
    }
}

impl<'a> KeyHintsView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        let chords = scene.presentation.key_hint_chords;
        let listing = CatalogView::from_scene(scene)
            .map(|catalog_view| &catalog_view.level().listing);
        match scene.overlay {
            Some(Overlay::Settings(..)) => Self {
                full_chips: &chords.settings_chips,
                compact_chips: &chords.settings_chips,
                listing,
                servers: scene.servers,
            },
            None
            | Some(
                Overlay::Help
                | Overlay::Search(_)
                | Overlay::ServerSearch
                | Overlay::SavePlaylist(_)
                | Overlay::History(_)
                | Overlay::ConfirmTrash(_)
                | Overlay::JumpToTime(_)
                | Overlay::TrackDetails(_)
                | Overlay::MusicDir { .. }
                | Overlay::AddServer(_)
                | Overlay::Servers(_)
                | Overlay::ConfirmRemove(_),
            ) => Self {
                full_chips: &chords.chips,
                compact_chips: &chords.compact_chips,
                listing,
                servers: scene.servers,
            },
        }
    }
}

impl<'a> CardView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        Self {
            player: scene.player,
            speed: scene.transport.speed,
            volume: scene.transport.volume,
            spectrum: scene.presentation.spectrum,
            repeat_mode: scene.playlist.repeat_mode,
            play_order: &scene.playlist.play_order,
            displayed_track: scene.displayed_track,
            output_status: &scene.transport.output_status,
            buffering_revision: scene.transport.buffering_revision,
            now: scene.presentation.now,
        }
    }
}

impl<'a> StatusLineView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        let shuffle = if scene.playlist.play_order.is_shuffle() {
            Shuffle::On
        } else {
            Shuffle::Off
        };
        Self {
            shuffle,
            repeat_mode: scene.playlist.repeat_mode,
            queue_len: scene.queue.len(),
            selected: scene.selected,
            playlist_len: scene.rows.len(),
            playlist_source: scene.playlist_source,
            scan_status: scene.scan_status,
            scanning_label: scene.presentation.theme.scanning_label.as_str(),
            spinner: scene.active_theme().spinner,
            theme_name: scene.presentation.theme.name.as_str(),
            remaining: scene
                .transport
                .sleep_timer
                .map(|timer| timer.deadline_at.elapsed_since(scene.presentation.now)),
            servers: scene.servers,
            catalog_name: scene.catalog_name,
        }
    }
}

impl<'a> PlaylistView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        Self {
            rows: scene.rows,
            queue: scene.queue,
            favorites: scene.favorites,
            selected: scene.selected,
            playing_index: scene.playing_index,
            library_status: scene.library_status,
            status_line_view: StatusLineView::from_scene(scene),
            catalog_view: CatalogView::from_scene(scene),
        }
    }
}

impl<'a> CatalogView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Option<Self> {
        match scene.catalog_name {
            CatalogName::Local => None,
            CatalogName::Server(server_name) => Some(Self {
                catalog: scene
                    .catalogs
                    .iter()
                    .find(|catalog| catalog.server_name == *server_name)?,
                server: scene
                    .servers
                    .iter()
                    .find(|server| server.account.server_name == *server_name)?,
                queue: scene.queue,
                favorites: scene.favorites,
                playing_track_source: scene
                    .player
                    .current()
                    .map(|track| track.source()),
            }),
        }
    }
}

impl<'a> SettingsView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        let audio = &scene.settings.audio_settings;
        Self {
            crossfade: audio.crossfade,
            replay_gain: audio.replay_gain,
            theme: theme_label(&scene.themes.theme_choice),
            theme_names: &scene.themes.names,
            sleep_presets: audio.sleep_presets.as_slice(),
            music_dir: scene.music_dir,
            home_dir: scene.presentation.home_dir,
            output_device_name: audio.device.named().map(DeviceName::as_str),
            output_devices: &scene.settings.output_devices,
            appearance_settings: scene.settings.appearance_settings,
        }
    }
}

impl<'a> OverlayView<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Self {
        Self {
            overlay: scene.overlay,
            rows: scene.rows,
            history: scene.history,
            servers: scene.servers,
            active_theme: scene.active_theme(),
            settings_view: SettingsView::from_scene(scene),
            bindings: scene.bindings,
            now: scene.presentation.now,
        }
    }
}

impl<'a> ToastWidget<'a> {
    #[must_use]
    pub(crate) fn from_scene(scene: &Scene<'a>) -> Option<Self> {
        (!scene.toasts.is_empty())
            .then(|| Self::new(scene.toasts, scene.active_theme()))
    }
}

fn theme_label(choice: &ThemeChoice) -> &str {
    match choice {
        ThemeChoice::Auto => "auto",
        ThemeChoice::Named(name) => name.as_str(),
    }
}

fn painted_cover_mode(cover_mode: CoverMode, pixel_path: PixelPath) -> CoverMode {
    match (cover_mode, pixel_path) {
        (CoverMode::Vinyl | CoverMode::Plain, PixelPath::Halfblocks)
        | (CoverMode::Off, PixelPath::Protocol | PixelPath::Halfblocks) => {
            CoverMode::Off
        }
        (CoverMode::Vinyl | CoverMode::Plain, PixelPath::Protocol)
        | (CoverMode::Milkdrop, PixelPath::Protocol | PixelPath::Halfblocks) => {
            cover_mode
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kernel::domain::{
        appearance::CoverMode,
        catalog::{BrowseLevel, Catalog, Paging},
        cursor::Cursor,
        server::{
            Account,
            AlbumId,
            AlbumOrder,
            Endpoint,
            Listing,
            Page,
            RemoteError,
            Server,
            ServerAlbum,
            ServerName,
            ServerStatus,
            ServerTrackId,
            Session,
            UserName,
        },
        track::{CatalogRow, Track},
    };
    use rstest::rstest;

    use crate::{
        playlist::{pane::PlaylistWidget, view::PlaylistView},
        scene::{PixelPath, painted_cover_mode},
        test_support::{SceneSources, model_with_tracks, rendered},
    };

    #[rstest]
    #[case::vinyl_with_graphics(
        CoverMode::Vinyl,
        PixelPath::Protocol,
        CoverMode::Vinyl
    )]
    #[case::plain_with_graphics(
        CoverMode::Plain,
        PixelPath::Protocol,
        CoverMode::Plain
    )]
    #[case::vinyl_without_graphics(
        CoverMode::Vinyl,
        PixelPath::Halfblocks,
        CoverMode::Off
    )]
    #[case::plain_without_graphics(
        CoverMode::Plain,
        PixelPath::Halfblocks,
        CoverMode::Off
    )]
    #[case::off_with_graphics(CoverMode::Off, PixelPath::Protocol, CoverMode::Off)]
    #[case::off_without_graphics(CoverMode::Off, PixelPath::Halfblocks, CoverMode::Off)]
    #[case::milkdrop_with_graphics(
        CoverMode::Milkdrop,
        PixelPath::Protocol,
        CoverMode::Milkdrop
    )]
    #[case::milkdrop_without_graphics(
        CoverMode::Milkdrop,
        PixelPath::Halfblocks,
        CoverMode::Milkdrop
    )]
    fn the_painted_cover_mode_reads_the_style_and_the_terminal(
        #[case] cover_mode: CoverMode,
        #[case] pixel_path: PixelPath,
        #[case] expected: CoverMode,
    ) {
        assert_eq!(painted_cover_mode(cover_mode, pixel_path), expected);
    }

    fn album(album_id: &str, title: &str, year: Option<u16>) -> CatalogRow {
        CatalogRow::Album(ServerAlbum {
            album_id: AlbumId::new(album_id),
            title: Arc::from(title),
            artist: Arc::from("Miles Davis"),
            year,
            track_count: if year.is_some() { 5 } else { 1 },
            duration: std::time::Duration::from_secs(2_744),
        })
    }

    fn server_track(title: &str) -> CatalogRow {
        CatalogRow::Track(tagged_server_track(title))
    }

    fn tagged_server_track(title: &str) -> Arc<Track> {
        let server_name = ServerName::new("home");
        let server_track_id = ServerTrackId::new(title);
        let source = kernel::domain::track::TrackSource::Server {
            server_name,
            server_track_id,
        };
        let tags = kernel::domain::track::Tags {
            title: Some(title.to_string()),
            ..kernel::domain::track::Tags::default()
        };
        let duration = std::time::Duration::from_secs(545);
        Arc::new(Track::tagged(source, duration, tags))
    }

    fn albums(paging: Paging) -> Catalog {
        let mut catalog = Catalog::new(ServerName::new("home"));
        let level = &mut catalog.albums_level;
        level.listing = Listing::Albums(AlbumOrder::Newest);
        level.catalog_rows = vec![
            album("a-1", "Kind of Blue", Some(1959)),
            album("a-2", "Sketches of Spain", None),
        ];
        level.cursor = Cursor::new(2);
        level.paging = paging;
        catalog
    }

    fn server_frame(
        server_status: ServerStatus,
        catalog: Catalog,
        width: u16,
    ) -> String {
        frame_of(server_model(server_status, catalog), width)
    }

    fn server_model(
        server_status: ServerStatus,
        catalog: Catalog,
    ) -> kernel::domain::model::Model {
        let mut model = model_with_tracks(0);
        let endpoint = Endpoint::parse("https://music.example.com").unwrap();
        let user_name = UserName::new("mikey").unwrap();
        let server_name = ServerName::new("home");
        let account = Account {
            server_name: server_name.clone(),
            endpoint,
            user_name,
        };
        model.servers = vec![Server {
            account,
            server_status,
        }];
        model.catalogs = vec![catalog];
        model.catalog_name = kernel::domain::catalog::CatalogName::Server(server_name);
        model
    }

    fn frame_of(model: kernel::domain::model::Model, width: u16) -> String {
        let sources = SceneSources::new(model);
        let scene = sources.scene();
        let widget =
            PlaylistWidget::new(PlaylistView::from_scene(&scene), scene.active_theme());
        rendered(width, 8, |frame| frame.render_widget(&widget, frame.area()))
            .to_string()
    }

    fn online() -> ServerStatus {
        let endpoint = Endpoint::parse("https://music.example.com").unwrap();
        ServerStatus::Online(Session::new(endpoint, "u=mikey"))
    }

    fn no_albums(paging: Paging) -> Catalog {
        let mut catalog = Catalog::new(ServerName::new("home"));
        catalog.albums_level.listing = Listing::Albums(AlbumOrder::Newest);
        catalog.albums_level.paging = paging;
        catalog
    }

    #[rstest]
    #[case::loading(
        "loading",
        server_frame(online(), albums(Paging::Loading(Page(1))), 80),
        "newest · ⣾ loading albums…"
    )]
    #[case::empty(
        "empty",
        server_frame(online(), no_albums(Paging::Complete), 80),
        "No albums on this server"
    )]
    #[case::offline(
        "offline",
        server_frame(
            ServerStatus::Offline(RemoteError::Moved { server_name: ServerName::new("home") }),
            albums(Paging::Complete),
            80,
        ),
        "○ music.example.com unreachable · c opens Servers"
    )]
    fn a_server_tab_paints_what_it_waits_for(
        #[case] name: &str,
        #[case] text: String,
        #[case] expected: &str,
    ) {
        assert!(text.contains(expected), "got {text:?}");
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(text);
        });
    }

    fn empty_view(listing: Listing) -> Catalog {
        let mut catalog = Catalog::new(ServerName::new("home"));
        catalog.albums_level.listing = listing;
        catalog.albums_level.paging = Paging::Complete;
        catalog
    }

    fn filtered(catalog_rows: Vec<CatalogRow>) -> Catalog {
        let mut catalog = albums(Paging::Complete);
        let level = &mut catalog.albums_level;
        level.cursor = Cursor::new(catalog_rows.len());
        level.server_query = Some(kernel::domain::overlay::ServerQuery {
            input: "kind".to_owned(),
            catalog_rows,
            revision: None,
        });
        catalog
    }

    #[rstest]
    #[case::filtered(
        "filtered",
        server_frame(
            online(),
            filtered(vec![album("a-1", "Kind of Blue", Some(1959))]),
            80,
        ),
        "1 / 2 · /kind"
    )]
    #[case::nothing_matches(
        "nothing_matches",
        server_frame(online(), filtered(Vec::new()), 80),
        "Nothing matches"
    )]
    #[case::no_songs(
        "no_songs",
        server_frame(online(), empty_view(Listing::Songs), 80),
        "No songs on this server"
    )]
    #[case::no_playlists(
        "no_playlists",
        server_frame(online(), empty_view(Listing::Playlists), 80),
        "No playlists on this server"
    )]
    fn a_server_view_says_what_its_filter_leaves_and_when_it_is_empty(
        #[case] name: &str,
        #[case] text: String,
        #[case] expected: &str,
    ) {
        assert!(text.contains(expected), "got {text:?}");
        insta::with_settings!({ snapshot_suffix => name }, {
            insta::assert_snapshot!(text);
        });
    }

    #[rstest]
    #[case::animations_on(kernel::domain::appearance::Animations::On)]
    #[case::animations_off(kernel::domain::appearance::Animations::Off)]
    fn a_wait_spins_whether_animations_are_on_or_off(
        #[case] animations: kernel::domain::appearance::Animations,
    ) {
        let mut model = model_with_tracks(0);
        model.settings.appearance_settings.animations = animations;
        let mut sources = SceneSources::new(model);
        sources.since_first_paint = std::time::Duration::from_millis(250);
        assert_eq!(sources.scene().active_theme().spinner.glyph(), "⣻");
    }

    #[rstest]
    #[case::not_asked(Paging::Next(Page(0)))]
    #[case::loading(Paging::Loading(Page(0)))]
    fn a_tab_before_its_first_page_paints_no_empty_label(#[case] paging: Paging) {
        let text = server_frame(online(), no_albums(paging), 80);
        assert!(!text.contains("No albums"), "got {text:?}");
    }

    #[test]
    fn a_60_column_server_tab_keeps_the_album_details_at_the_right() {
        let text = server_frame(online(), albums(Paging::Complete), 60);
        assert!(text.contains("1959 · 5 tracks · 45:44"), "got {text:?}");
        insta::assert_snapshot!(text);
    }

    #[test]
    fn an_open_album_follows_the_path_and_lists_its_tracks_like_local_rows() {
        let mut catalog = albums(Paging::Complete);
        catalog.album_level = Some(BrowseLevel {
            listing: Listing::Album(AlbumId::new("a-1")),
            catalog_rows: vec![server_track("So What"), server_track("Blue in Green")],
            cursor: Cursor::at(2, 1),
            paging: Paging::Complete,
            server_query: None,
        });
        let text = server_frame(online(), catalog, 80);
        assert!(text.contains("newest › Kind of Blue"), "got {text:?}");
        assert!(text.contains("So What"), "got {text:?}");
        insta::assert_snapshot!(text);
    }

    #[test]
    fn a_queued_server_row_wears_its_queue_number() {
        let mut catalog = albums(Paging::Complete);
        catalog.album_level = Some(BrowseLevel {
            listing: Listing::Album(AlbumId::new("a-1")),
            catalog_rows: vec![server_track("So What"), server_track("Blue in Green")],
            cursor: Cursor::at(2, 0),
            paging: Paging::Complete,
            server_query: None,
        });
        let queue = vec![
            tagged_server_track("Blue in Green"),
            tagged_server_track("So What"),
        ];
        let mut model = server_model(online(), catalog);
        model.queue = queue;
        let text = frame_of(model, 80);
        insta::assert_snapshot!(text);
    }
}
