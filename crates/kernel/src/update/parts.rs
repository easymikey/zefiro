use crate::{
    domain::model::Model,
    update::{browse, config, library, player, server},
};

pub(crate) fn config_parts(model: &mut Model) -> config::ConfigParts<'_> {
    let Model {
        workspace,
        revisions,
        settings,
        themes,
        music_dir,
        library: _library,
        scan_status: _scan_status,
        playlist: _playlist,
        playlist_source: _playlist_source,
        queue: _queue,
        player: _player,
        transport: _transport,
        history: _history,
        favorites: _favorites,
        drivers: _drivers,
        servers: _servers,
        downloads: _downloads,
        catalog_name: _catalog_name,
        catalogs: _catalogs,
        play_reports: _play_reports,
    } = model;
    config::ConfigParts {
        workspace,
        revisions,
        settings,
        themes,
        music_dir,
    }
}

pub(crate) fn playback_parts(model: &mut Model) -> player::events::PlaybackParts<'_> {
    let Model {
        player,
        transport,
        playlist,
        queue,
        workspace,
        revisions,
        settings,
        library: _library,
        music_dir: _music_dir,
        scan_status: _scan_status,
        playlist_source: _playlist_source,
        history: _history,
        favorites: _favorites,
        themes: _themes,
        drivers: _drivers,
        servers,
        downloads,
        catalog_name: _catalog_name,
        catalogs: _catalogs,
        play_reports: _play_reports,
    } = model;
    player::events::PlaybackParts {
        player,
        transport,
        playlist,
        queue,
        workspace,
        revisions,
        settings,
        servers,
        downloads,
    }
}

pub(crate) fn browse_parts(model: &mut Model) -> browse::BrowseParts<'_> {
    let Model {
        player,
        transport,
        playlist,
        queue,
        workspace,
        revisions,
        settings,
        library,
        favorites,
        scan_status,
        music_dir,
        playlist_source,
        history: _history,
        themes: _themes,
        drivers: _drivers,
        servers,
        downloads,
        catalog_name,
        catalogs,
        play_reports: _play_reports,
    } = model;
    browse::BrowseParts {
        playback_parts: player::events::PlaybackParts {
            player,
            transport,
            playlist,
            queue,
            workspace,
            revisions,
            settings,
            servers,
            downloads,
        },
        library,
        favorites,
        scan_status,
        music_dir,
        playlist_source,
        catalog_name,
        catalogs,
    }
}

pub(crate) fn library_parts(model: &mut Model) -> library::LibraryParts<'_> {
    let Model {
        library,
        favorites,
        history,
        scan_status,
        music_dir,
        revisions,
        workspace,
        playlist,
        playlist_source,
        player,
        queue,
        transport: _transport,
        settings: _settings,
        themes: _themes,
        drivers: _drivers,
        servers: _servers,
        downloads: _downloads,
        catalog_name: _catalog_name,
        catalogs: _catalogs,
        play_reports: _play_reports,
    } = model;
    library::LibraryParts {
        library,
        favorites,
        history,
        scan_status,
        music_dir,
        revisions,
        workspace,
        playlist,
        playlist_source,
        player,
        queue,
    }
}

pub(crate) fn server_parts(model: &mut Model) -> server::ServerParts<'_> {
    let Model {
        servers,
        downloads,
        player,
        library: _library,
        favorites,
        history: _history,
        scan_status: _scan_status,
        music_dir: _music_dir,
        revisions,
        workspace,
        playlist: _playlist,
        playlist_source: _playlist_source,
        queue: _queue,
        transport: _transport,
        settings: _settings,
        themes: _themes,
        drivers: _drivers,
        catalog_name,
        catalogs,
        play_reports,
    } = model;
    server::ServerParts {
        servers,
        downloads,
        player,
        catalog_name,
        catalogs,
        revisions,
        favorites,
        overlay: &mut workspace.overlay,
        play_reports,
    }
}
