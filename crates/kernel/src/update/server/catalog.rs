use crate::{
    cmd::{Cmd, Effect, RemoteCmd},
    domain::{
        catalog::{Catalog, PAGE_LEAD, Paging},
        cursor::Cursor,
        favorites::Favorites,
        overlay::Overlay,
        revision::{Freshness, Revision, Revisions},
        server::{PAGE_ROWS, Page, RemoteError, ServerStatus},
        track::{CatalogRow, TrackSource},
    },
    message::{CatalogPage, ServerFavorite},
    update::{machine::Unhandled, overlay::search, server::ServerParts},
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
        .map(Catalog::level)
        .ok_or(Unhandled)?;
    if level.listing != listing || level.paging != Paging::Loading(page) {
        return Err(Unhandled);
    }
    stars(server_parts.favorites, &catalog_rows, &favorites);
    level.paging = if catalog_rows.len() < PAGE_ROWS {
        Paging::Complete
    } else {
        Paging::Next(Page(page.0 + 1))
    };
    if page == Page::default() {
        level.cursor = Cursor::new(catalog_rows.len());
        level.catalog_rows = catalog_rows;
    } else {
        level.catalog_rows.extend(catalog_rows);
        level.cursor = level.cursor.resize(level.catalog_rows.len());
    }
    Ok(Cmd::none())
}

pub(crate) fn found(
    server_parts: &mut ServerParts<'_>,
    result: Result<(Vec<CatalogRow>, Favorites), RemoteError>,
    revision: Revision,
) -> Result<Cmd, Unhandled> {
    let (catalog_rows, found_favorites) = match result {
        Ok(answer) => answer,
        Err(error) => return search::found(server_parts.overlay, Err(error), revision),
    };
    let cmd = search::found(server_parts.overlay, Ok(catalog_rows), revision)?;
    if let Some(Overlay::ServerSearch(server_query)) = server_parts.overlay.as_ref() {
        stars(
            server_parts.favorites,
            &server_query.content.catalog_rows,
            &found_favorites,
        );
    }
    Ok(cmd)
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
    catalog: &mut Catalog,
    server_status: &ServerStatus,
    revisions: &mut Revisions,
) -> Cmd {
    let server_name = catalog.server_name.clone();
    let level = catalog.level();
    let (session, page) = match (server_status, level.paging) {
        (ServerStatus::Online(session), Paging::Next(page) | Paging::Loading(page)) => {
            (session.clone(), page)
        }
        (ServerStatus::Online(_), Paging::Complete)
        | (ServerStatus::Connecting | ServerStatus::Offline(_), _) => {
            return Cmd::none();
        }
    };
    level.paging = Paging::Loading(page);
    revisions.list = revisions.issue_effect();
    Effect::Remote(RemoteCmd::List {
        server_name,
        session,
        listing: level.listing.clone(),
        page,
        revision: revisions.list,
    })
    .into()
}

pub(crate) fn show(
    catalog: &mut Catalog,
    server_status: &ServerStatus,
    revisions: &mut Revisions,
) -> Cmd {
    let level = catalog.level();
    match level.paging {
        Paging::Loading(_) => list(catalog, server_status, revisions),
        Paging::Next(_) if level.catalog_rows.is_empty() => {
            list(catalog, server_status, revisions)
        }
        Paging::Next(_) | Paging::Complete => Cmd::none(),
    }
}

pub(crate) fn moved(
    catalog: &mut Catalog,
    server_status: &ServerStatus,
    revisions: &mut Revisions,
) -> Cmd {
    let level = catalog.level();
    match level.paging {
        Paging::Next(_)
            if level.cursor.index() + PAGE_LEAD >= level.catalog_rows.len() =>
        {
            list(catalog, server_status, revisions)
        }
        Paging::Next(_) | Paging::Loading(_) | Paging::Complete => Cmd::none(),
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
            CatalogRow::Album(_server_album) => {}
        }
    }
}
