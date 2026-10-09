use std::{path::PathBuf, sync::Arc};

use kernel::{
    domain::{
        catalog::CatalogName,
        cursor::Cursor,
        history::HistoryEntry,
        index::{TrackIndex, ViewIndex},
        library::Library,
        model::Model,
        playlist::{Playlist, PlaylistRows, PlaylistSource},
        time::Moment,
        track::{Track, TrackSource},
    },
    message::{AudioEvent, BrowseRequest, Message, PlaybackRequest, QueueRequest},
};

use crate::{
    support::update::update,
    table::{
        server_play::{album_model, album_sources, playlist_sources},
        server_tab::{browse, online},
    },
};

fn library_sources() -> Vec<TrackSource> {
    ["/music/a.flac", "/music/b.flac"]
        .map(|path| TrackSource::Local(PathBuf::from(path)))
        .to_vec()
}

fn library_tab_after_an_album_play() -> Model {
    let mut model = album_model(online(), "flac");
    let tracks: Vec<Arc<Track>> = library_sources()
        .into_iter()
        .map(|source| Arc::new(Track::from(source)))
        .collect();
    model.library = Some(Library {
        tracks: tracks.clone(),
        track_indexes: vec![TrackIndex::new(0), TrackIndex::new(1)],
    });
    model.playlist = Playlist::from_tracks(tracks);
    model.workspace.browse.cursor = Cursor::at(2, 1);
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    drop(update(
        &mut model,
        Message::Audio(AudioEvent::Loaded(None)),
        Moment::default(),
    ));
    model.catalog_name = CatalogName::Local;
    model
}

fn playing_source(model: &Model) -> Option<TrackSource> {
    model.player.current().map(|track| track.source().clone())
}

#[test]
fn an_album_play_leaves_the_local_tab_over_the_library_with_no_playing_mark() {
    let model = library_tab_after_an_album_play();

    assert_eq!(playlist_sources(&model), album_sources());
    assert_eq!(model.workspace.browse.cursor, Cursor::at(2, 1));
    assert_eq!(model.playing_index(), None);
}

#[test]
fn the_local_tab_rows_are_the_library_tracks_while_an_album_plays() {
    let model = library_tab_after_an_album_play();

    let track_sources: Vec<TrackSource> = PlaylistRows::new(
        &model.playlist_source,
        model.library.as_ref(),
        &model.playlist,
    )
    .iter()
    .map(|track| track.source().clone())
    .collect();

    assert_eq!(track_sources, library_sources());
}

#[test]
fn a_local_search_pick_after_an_album_play_plays_the_library() {
    let mut model = library_tab_after_an_album_play();

    let answer = browse(&mut model, BrowseRequest::JumpTo(ViewIndex::new(0)));

    assert!(answer.is_ok());
    assert_eq!(model.playlist_source, PlaylistSource::Library);
    assert_eq!(playing_source(&model), Some(library_sources()[0].clone()));
}

#[test]
fn enter_in_the_local_tab_after_an_album_play_plays_the_library_in_library_order() {
    let mut model = library_tab_after_an_album_play();

    let answer = browse(&mut model, BrowseRequest::PlaySelected);

    assert!(answer.is_ok());
    assert_eq!(model.playlist_source, PlaylistSource::Library);
    assert_eq!(playlist_sources(&model), library_sources());
    assert_eq!(playing_source(&model), Some(library_sources()[1].clone()));
    assert_eq!(model.playing_index(), Some(ViewIndex::new(1)));
}

#[test]
fn an_album_play_over_a_shorter_named_playlist_spans_the_local_cursor_over_the_library()
{
    let mut model = album_model(online(), "flac");
    let tracks: Vec<Arc<Track>> = library_sources()
        .into_iter()
        .map(|source| Arc::new(Track::from(source)))
        .collect();
    model.library = Some(Library {
        tracks: tracks.clone(),
        track_indexes: vec![TrackIndex::new(0), TrackIndex::new(1)],
    });
    model.playlist = Playlist::from_tracks(tracks[..1].to_vec());
    model.playlist_source = PlaylistSource::Named;
    model.workspace.browse.cursor = Cursor::at(1, 0);

    let answer = browse(&mut model, BrowseRequest::PlaySelected);

    assert!(answer.is_ok());
    assert_eq!(model.workspace.browse.cursor.len(), library_sources().len());
}

#[test]
fn next_after_an_album_play_walks_the_album_from_the_local_tab() {
    let mut model = library_tab_after_an_album_play();

    drop(update(
        &mut model,
        Message::Playback(PlaybackRequest::Next),
        Moment::default(),
    ));

    assert_eq!(playing_source(&model), Some(album_sources()[2].clone()));
    assert_eq!(model.workspace.browse.cursor, Cursor::at(2, 1));
}

#[test]
fn queueing_a_local_row_after_an_album_play_queues_the_library_track() {
    let mut model = library_tab_after_an_album_play();

    drop(update(
        &mut model,
        Message::Queue(QueueRequest::Toggle),
        Moment::default(),
    ));

    assert_eq!(queued_sources(&model), vec![library_sources()[1].clone()]);
}

#[test]
fn a_local_history_entry_after_an_album_play_enqueues_its_library_track() {
    let mut model = library_tab_after_an_album_play();
    model.history = vec![HistoryEntry {
        track_source: library_sources()[0].clone(),
        title: "a".to_string(),
        artist: None,
        played_at: Moment::default(),
    }];

    drop(update(
        &mut model,
        Message::Queue(QueueRequest::ToggleHistoryEntry(0)),
        Moment::default(),
    ));

    assert_eq!(queued_sources(&model), vec![library_sources()[0].clone()]);
}

fn queued_sources(model: &Model) -> Vec<TrackSource> {
    model
        .queue
        .iter()
        .map(|track| track.source().clone())
        .collect()
}

#[test]
fn a_server_play_before_the_scan_lands_leaves_the_local_tab_empty() {
    let mut model = album_model(online(), "flac");

    let answer = browse(&mut model, BrowseRequest::PlaySelected);
    drop(update(
        &mut model,
        Message::Audio(AudioEvent::Loaded(None)),
        Moment::default(),
    ));

    assert!(answer.is_ok());
    assert_eq!(playlist_sources(&model), album_sources());
    let rows = PlaylistRows::new(
        &model.playlist_source,
        model.library.as_ref(),
        &model.playlist,
    );
    assert!(rows.is_empty());
    assert_eq!(model.workspace.browse.cursor.len(), 0);
}
