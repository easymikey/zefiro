use std::sync::Arc;

use crate::{
    cmd::{Cmd, Effect, RemoteCmd},
    domain::{
        catalog::{BrowseLevel, Catalog, PAGE_LEAD, Paging},
        cursor::Cursor,
        favorites::Favorites,
        playlist::PlaylistSource,
        revision::{Freshness, Revision, Revisions},
        server::{Listing, PAGE_ROWS, Page, RemoteError, Server, ServerStatus},
        toast::Toast,
        track::{CatalogRow, TrackSource},
    },
    message::{CatalogPage, Message, ServerFavorite},
    update::{machine::Unhandled, overlay::search::narrowed, server::ServerParts},
};

pub(crate) fn listed(
    server_parts: &mut ServerParts<'_>,
    catalog_page: CatalogPage,
) -> Result<Cmd, Unhandled> {
    let CatalogPage {
        server_name,
        listing,
        page,
        catalog_rows,
        favorites,
        revision,
    } = catalog_page;
    match revision.freshness(server_parts.revisions.list) {
        Freshness::Awaited => {}
        Freshness::Stale => return Err(Unhandled),
    }
    let level = server_parts
        .catalogs
        .iter_mut()
        .find(|catalog| catalog.server_name == server_name)
        .ok_or(Unhandled)?
        .levels()
        .find(|level| level.listing == listing && level.paging == Paging::Loading(page))
        .ok_or(Unhandled)?;
    stars(server_parts.favorites, &catalog_rows, &favorites);
    level.paging = match level.listing {
        Listing::Songs | Listing::Albums(_) if catalog_rows.len() >= PAGE_ROWS => {
            Paging::Next(Page(page.0 + 1))
        }
        Listing::Songs
        | Listing::Albums(_)
        | Listing::Album(_)
        | Listing::Playlists
        | Listing::Playlist(_) => Paging::Complete,
    };
    if page == Page::default() {
        level.catalog_rows = catalog_rows;
        narrowed(level);
        level.cursor = Cursor::new(level.rows().len());
    } else {
        if level.listing == Listing::Songs
            && *server_parts.playlist_source == PlaylistSource::Songs(server_name)
        {
            let playlist = &mut *server_parts.playlist;
            playlist
                .tracks
                .extend(catalog_rows.iter().filter_map(
                    |catalog_row| match catalog_row {
                        CatalogRow::Track(track) => Some(Arc::clone(track)),
                        CatalogRow::Album(_) | CatalogRow::Playlist(_) => None,
                    },
                ));
            playlist.cursor = playlist.cursor.resize(playlist.tracks.len());
        }
        level.catalog_rows.extend(catalog_rows);
        level.cursor = level.cursor.resize(level.rows().len());
    }
    Ok(list(
        server_parts.catalogs,
        server_parts.servers,
        server_parts.revisions,
    ))
}

pub(crate) fn upcoming(server_parts: ServerParts<'_>) -> Cmd {
    let ServerParts {
        servers,
        downloads: _,
        player: _,
        catalog_name: _,
        catalogs,
        revisions,
        favorites: _,
        workspace: _,
        playlist,
        playlist_source,
        play_reports: _,
        queue: _,
    } = server_parts;
    let PlaylistSource::Songs(server_name) = playlist_source else {
        return Cmd::none();
    };
    let Some(position) = playlist.position() else {
        return Cmd::none();
    };
    if position + PAGE_LEAD < playlist.tracks.len() {
        return Cmd::none();
    }
    let level = catalogs
        .iter_mut()
        .filter(|catalog| catalog.server_name == *server_name)
        .flat_map(Catalog::levels)
        .find(|level| level.listing == Listing::Songs);
    let Some(level) = level else {
        return Cmd::none();
    };
    level.queue();
    list(catalogs, servers, revisions)
}

pub(crate) fn found(
    server_parts: &mut ServerParts<'_>,
    result: Result<(Vec<CatalogRow>, Favorites), RemoteError>,
    revision: Revision,
) -> Result<Cmd, Unhandled> {
    let (cursor, server_query) = server_parts
        .catalogs
        .iter_mut()
        .flat_map(Catalog::levels)
        .find_map(|level| {
            let BrowseLevel {
                listing: _,
                catalog_rows: _,
                cursor,
                paging: _,
                server_query,
            } = level;
            server_query
                .as_mut()
                .filter(|server_query| server_query.revision == Some(revision))
                .map(|server_query| (cursor, server_query))
        })
        .ok_or(Unhandled)?;
    server_query.revision = None;
    let (catalog_rows, found_favorites) = match result {
        Ok(answer) => answer,
        Err(error) => {
            return Ok(Cmd::message(Message::Toast(Toast::error(
                error.to_string(),
            ))));
        }
    };
    stars(server_parts.favorites, &catalog_rows, &found_favorites);
    *cursor = Cursor::new(catalog_rows.len());
    server_query.catalog_rows = catalog_rows;
    Ok(Cmd::none())
}

pub(crate) fn starred(
    server_parts: &mut ServerParts<'_>,
    server_favorite: ServerFavorite,
) -> Result<Cmd, Unhandled> {
    let ServerFavorite {
        server_name,
        server_track_id,
        favorite,
    } = server_favorite;
    let track_source = TrackSource::Server {
        server_name,
        server_track_id,
    };
    if server_parts.favorites.favorite(&track_source) == favorite {
        return Err(Unhandled);
    }
    server_parts.favorites.set(track_source, favorite);
    Ok(Cmd::none())
}

pub(crate) fn list(
    catalogs: &mut [Catalog],
    servers: &[Server],
    revisions: &mut Revisions,
) -> Cmd {
    let loading =
        catalogs
            .iter_mut()
            .flat_map(Catalog::levels)
            .any(|level| match level.paging {
                Paging::Loading(_) => true,
                Paging::Next(_) | Paging::Queued(_) | Paging::Complete => false,
            });
    if loading {
        return Cmd::none();
    }
    let next = catalogs.iter_mut().find_map(|catalog| {
        let server = servers
            .iter()
            .find(|server| server.account.server_name == catalog.server_name)?;
        let session = match &server.server_status {
            ServerStatus::Online(session) => session,
            ServerStatus::Connecting | ServerStatus::Offline(_) => return None,
        };
        catalog.levels().find_map(|level| match level.paging {
            Paging::Queued(page) => Some((server, session, level, page)),
            Paging::Next(_) | Paging::Loading(_) | Paging::Complete => None,
        })
    });
    let Some((server, session, level, page)) = next else {
        return Cmd::none();
    };
    level.paging = Paging::Loading(page);
    revisions.list = revisions.issue_effect();
    Effect::Remote(RemoteCmd::List {
        server_name: server.account.server_name.clone(),
        session: session.clone(),
        listing: level.listing.clone(),
        page,
        revision: revisions.list,
    })
    .into()
}

pub(crate) fn show(catalog: &mut Catalog) {
    let level = catalog.level();
    if level.catalog_rows.is_empty() {
        level.queue();
    }
}

pub(crate) fn moved(catalog: &mut Catalog) {
    let level = catalog.level();
    if level.query().is_none()
        && level.cursor.index() + PAGE_LEAD >= level.catalog_rows.len()
    {
        level.queue();
    }
}

fn stars(
    favorites: &mut Favorites,
    catalog_rows: &[CatalogRow],
    listed_favorites: &Favorites,
) {
    for catalog_row in catalog_rows {
        match catalog_row {
            CatalogRow::Track(track) => {
                favorites.set(
                    track.source().clone(),
                    listed_favorites.favorite(track.source()),
                );
            }
            CatalogRow::Album(_) | CatalogRow::Playlist(_) => {}
        }
    }
}
