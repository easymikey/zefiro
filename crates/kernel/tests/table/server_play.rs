use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    cmd::{AudioCmd, Cmd, Effect, GrowingMedia, Media, RemoteCmd},
    domain::{
        catalog::{BrowseLevel, CatalogName, Paging},
        cursor::Cursor,
        index::ViewIndex,
        io_error::IoError,
        model::Model,
        player::Player,
        playhead::Playhead,
        playlist::{Playlist, PlaylistSource},
        revision::Revision,
        server::{
            AlbumId,
            CacheKey,
            Download,
            Fetched,
            Listing,
            MediaFetch,
            RemoteError,
            ServerName,
            ServerStatus,
            ServerTrackId,
        },
        speed::Speed,
        time::Moment,
        track::{AudioFormat, CatalogRow, Tags, Track, TrackParts, TrackSource},
    },
    message::{
        AudioEvent,
        BrowseRequest,
        Message,
        PlaybackRequest,
        QueueRequest,
        RemoteEvent,
        ServerRequest,
        Timer,
    },
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::{
    support::update::update,
    table::server_tab::{browse, home, online, server_model, session},
};

fn server_track(track_number: usize, format: &str) -> Track {
    Track::from(TrackSource::Server {
        server_name: home(),
        server_track_id: ServerTrackId::new(&format!("tr-{track_number}")),
    })
    .with_audio_format(AudioFormat {
        format: Some(format.to_string()),
        ..AudioFormat::default()
    })
}

fn album_model(server_status: ServerStatus, format: &str) -> Model {
    let mut model = server_model(server_status, 1);
    let mut album_level = BrowseLevel::new(Listing::Album(AlbumId::new("al-0")));
    album_level.catalog_rows = (0..3)
        .map(|track_number| {
            CatalogRow::Track(Arc::new(server_track(track_number, format)))
        })
        .collect();
    album_level.cursor = Cursor::at(3, 1);
    album_level.paging = Paging::Complete;
    model.catalogs[0].album_level = Some(album_level);
    model
}

fn album_sources() -> Vec<TrackSource> {
    (0..3)
        .map(|track_number| server_track(track_number, "flac").source().clone())
        .collect()
}

fn playlist_sources(model: &Model) -> Vec<TrackSource> {
    model
        .playlist
        .tracks
        .iter()
        .map(|track| track.source().clone())
        .collect()
}

#[test]
fn enter_on_the_second_track_makes_the_album_the_playlist_with_the_cursor_on_it() {
    let mut model = album_model(online(), "flac");

    let answer = browse(&mut model, BrowseRequest::PlaySelected);

    assert!(answer.is_ok());
    assert_eq!(playlist_sources(&model), album_sources());
    assert_eq!(model.playlist.cursor.index(), 1);
    assert_eq!(model.playlist_source, PlaylistSource::Server(home()));
}

#[rstest]
#[case::toggle(QueueRequest::Toggle)]
#[case::play_next(QueueRequest::PlayNext)]
fn queueing_a_server_track_is_refused(#[case] queue_request: QueueRequest) {
    let mut model = album_model(online(), "flac");
    drop(browse(&mut model, BrowseRequest::PlaySelected));

    let answer = update(&mut model, Message::Queue(queue_request), Moment::default());

    assert_eq!(answer, Err(Unhandled));
    assert_eq!(model.queue, Vec::new());
}

#[test]
fn enter_on_a_server_track_orders_a_fetch_from_byte_zero_with_the_session() {
    let mut model = album_model(online(), "flac");

    let effects = kernel::update::update(
        &mut model,
        Message::Browse(BrowseRequest::PlaySelected),
        Moment::default(),
    )
    .unwrap();

    let fetches: Vec<&MediaFetch> = effects
        .iter()
        .filter_map(|effect| {
            if let Effect::Remote(RemoteCmd::Fetch(media_fetch)) = effect {
                Some(media_fetch)
            } else {
                None
            }
        })
        .collect();
    let server_track_id = ServerTrackId::new("tr-1");
    let media_fetch = MediaFetch {
        server_name: home(),
        cache_key: CacheKey::new(&home(), &server_track_id, "flac"),
        server_track_id,
        session: session(),
        first_byte: 0,
        revision: fetches
            .first()
            .map_or_else(Revision::default, |fetch| fetch.revision),
    };
    assert_eq!(fetches, vec![&media_fetch]);
    assert_eq!(
        model.downloads,
        vec![Download {
            media_fetch,
            fetched: None,
        }]
    );
    assert_eq!(
        model.player.current().map(|track| track.source().clone()),
        Some(server_track(1, "flac").source().clone())
    );
}

#[test]
fn enter_on_a_track_of_an_offline_server_toasts_and_plays_nothing() {
    let mut model = album_model(
        ServerStatus::Offline(RemoteError::Unreachable {
            server_name: home(),
            source: IoError::Other,
        }),
        "flac",
    );

    let answer = browse(&mut model, BrowseRequest::PlaySelected);

    assert!(answer.is_ok());
    assert_eq!(
        model
            .workspace
            .toasts
            .last()
            .map(|toast| toast.title.as_str()),
        Some("home is offline")
    );
    assert_eq!(model.player.current(), None);
    assert_eq!(model.playlist.tracks, Vec::new());
    assert_eq!(model.downloads, Vec::new());
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

fn media_path() -> PathBuf {
    PathBuf::from("/cache/home/tr-1")
}

fn no_load(_revision: Revision) -> Option<Media> {
    None
}

fn growing_from_the_margin(revision: Revision) -> Option<Media> {
    Some(Media::Growing(GrowingMedia {
        media_path: media_path(),
        downloaded: 512 * KIB,
        byte_len: 9 * MIB,
        revision,
    }))
}

fn local(_revision: Revision) -> Option<Media> {
    Some(Media::Local(media_path()))
}

#[rstest]
#[case::below_the_margin("flac", 512 * KIB - 1, no_load)]
#[case::at_the_margin("flac", 512 * KIB, growing_from_the_margin)]
#[case::cached("flac", 9 * MIB, local)]
#[case::m4a_growing("m4a", 4 * MIB, no_load)]
#[case::m4b_growing("M4B", 4 * MIB, no_load)]
#[case::m4a_complete("m4a", 9 * MIB, local)]
fn a_fetched_track_loads_at_the_start_margin_or_when_complete(
    #[case] format: &str,
    #[case] downloaded: u64,
    #[case] media: fn(Revision) -> Option<Media>,
) {
    let mut model = album_model(online(), format);
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    let revision = model
        .downloads
        .first()
        .map_or_else(Revision::default, |download| download.media_fetch.revision);

    let effects = kernel::update::update(
        &mut model,
        Message::Remote(RemoteEvent::Fetched {
            revision,
            result: Ok(Fetched {
                media_path: media_path(),
                downloaded,
                byte_len: 9 * MIB,
            }),
        }),
        Moment::default(),
    );

    let medias: Vec<Media> = effects
        .into_iter()
        .flatten()
        .filter_map(|effect| {
            if let Effect::Audio(AudioCmd::Load(track_load)) = effect {
                Some(track_load.media)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(medias, media(revision).into_iter().collect::<Vec<_>>());
}

#[rstest]
#[case::toggle(QueueRequest::Toggle)]
#[case::play_next(QueueRequest::PlayNext)]
fn queueing_in_a_server_tab_over_a_local_playlist_is_refused(
    #[case] queue_request: QueueRequest,
) {
    let mut model = server_model(online(), 1);
    model.playlist = Playlist::from_tracks(vec![Arc::new(Track::from(
        TrackSource::Local(PathBuf::from("/music/a.flac")),
    ))]);
    model.workspace.browse.cursor = Cursor::at(1, 0);

    let answer = update(&mut model, Message::Queue(queue_request), Moment::default());

    assert_eq!(answer, Err(Unhandled));
    assert_eq!(model.queue, Vec::new());
}

#[test]
fn enter_on_a_server_track_keeps_the_browse_cursor_inside_the_album() {
    let mut model = album_model(online(), "flac");
    model.workspace.browse.cursor = Cursor::at(60, 50);

    drop(browse(&mut model, BrowseRequest::PlaySelected));

    assert_eq!(model.workspace.browse.cursor.len(), 3);
    assert!(model.workspace.browse.cursor.index() < 3);
}

fn played_fetches(model: &mut Model, message: Message) -> Vec<MediaFetch> {
    kernel::update::update(model, message, Moment::default())
        .unwrap()
        .into_iter()
        .filter_map(|effect| {
            if let Effect::Remote(RemoteCmd::Fetch(media_fetch)) = effect {
                Some(media_fetch)
            } else {
                None
            }
        })
        .collect()
}

fn fetch_from_zero(track_number: usize, media_fetches: &[MediaFetch]) -> MediaFetch {
    let server_track_id = ServerTrackId::new(&format!("tr-{track_number}"));
    MediaFetch {
        server_name: home(),
        cache_key: CacheKey::new(&home(), &server_track_id, "flac"),
        server_track_id,
        session: session(),
        first_byte: 0,
        revision: media_fetches
            .first()
            .map_or_else(Revision::default, |media_fetch| media_fetch.revision),
    }
}

#[test]
fn next_onto_a_server_track_orders_a_fetch_from_byte_zero_with_a_new_revision() {
    let mut model = album_model(online(), "flac");
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    let first_revision = model
        .downloads
        .first()
        .map(|download| download.media_fetch.revision);

    let media_fetches =
        played_fetches(&mut model, Message::Playback(PlaybackRequest::Next));

    assert_eq!(media_fetches, vec![fetch_from_zero(2, &media_fetches)]);
    assert_ne!(
        media_fetches
            .first()
            .map(|media_fetch| media_fetch.revision),
        first_revision
    );
    assert_eq!(model.playlist.cursor.index(), 2);
}

#[test]
fn next_onto_a_track_of_an_offline_server_toasts_and_changes_nothing() {
    let mut model = album_model(online(), "flac");
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    model.servers[0].server_status = ServerStatus::Offline(RemoteError::Unreachable {
        server_name: home(),
        source: IoError::Other,
    });

    let answer = update(
        &mut model,
        Message::Playback(PlaybackRequest::Next),
        Moment::default(),
    );

    assert!(answer.is_ok());
    assert_eq!(
        model
            .workspace
            .toasts
            .last()
            .map(|toast| toast.title.as_str()),
        Some("home is offline")
    );
    assert_eq!(
        model.player.current().map(|track| track.source().clone()),
        Some(server_track(1, "flac").source().clone())
    );
    assert_eq!(model.playlist.cursor.index(), 1);
}

#[test]
fn the_successor_of_an_ending_server_track_orders_a_fetch() {
    let mut model = album_model(online(), "flac");
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    drop(update(
        &mut model,
        Message::Audio(AudioEvent::Loaded(None)),
        Moment::default(),
    ));

    let media_fetches = played_fetches(&mut model, Message::Audio(AudioEvent::Ended));

    assert_eq!(media_fetches, vec![fetch_from_zero(2, &media_fetches)]);
}

fn go_offline(model: &mut Model) {
    model.servers[0].server_status = ServerStatus::Offline(RemoteError::Unreachable {
        server_name: home(),
        source: IoError::Other,
    });
}

fn last_toast(model: &Model) -> Option<&str> {
    model
        .workspace
        .toasts
        .last()
        .map(|toast| toast.title.as_str())
}

fn playing_album_model() -> Model {
    let mut model = album_model(online(), "flac");
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    drop(update(
        &mut model,
        Message::Audio(AudioEvent::Loaded(None)),
        Moment::default(),
    ));
    model
}

#[test]
fn an_ending_track_before_a_track_of_an_offline_server_toasts_and_stops() {
    let mut model = playing_album_model();
    go_offline(&mut model);

    let media_fetches = played_fetches(&mut model, Message::Audio(AudioEvent::Ended));

    assert_eq!(media_fetches, Vec::new());
    assert_eq!(last_toast(&model), Some("home is offline"));
    assert_eq!(model.player, Player::Stopped);
    assert_eq!(model.playlist.cursor.index(), 1);
}

#[rstest]
#[case::previous(PlaybackRequest::Previous)]
#[case::jump(PlaybackRequest::JumpTo(ViewIndex::new(2)))]
fn playing_onto_a_track_of_an_offline_server_toasts_and_changes_nothing(
    #[case] playback_request: PlaybackRequest,
) {
    let mut model = playing_album_model();
    go_offline(&mut model);
    let player = model.player.clone();

    let answer = update(
        &mut model,
        Message::Playback(playback_request),
        Moment::default(),
    );

    assert!(answer.is_ok());
    assert_eq!(last_toast(&model), Some("home is offline"));
    assert_eq!(model.player, player);
    assert_eq!(model.playlist.cursor.index(), 1);
}

fn stopped_album_model() -> Model {
    let mut model = playing_album_model();
    drop(update(
        &mut model,
        Message::Playback(PlaybackRequest::Stop),
        Moment::default(),
    ));
    model
}

#[test]
fn toggle_from_stopped_on_a_server_track_orders_a_fetch() {
    let mut model = stopped_album_model();

    let media_fetches =
        played_fetches(&mut model, Message::Playback(PlaybackRequest::Toggle));

    assert_eq!(media_fetches, vec![fetch_from_zero(1, &media_fetches)]);
}

#[test]
fn toggle_from_stopped_on_a_track_of_an_offline_server_toasts_and_changes_nothing() {
    let mut model = stopped_album_model();
    go_offline(&mut model);

    let answer = update(
        &mut model,
        Message::Playback(PlaybackRequest::Toggle),
        Moment::default(),
    );

    assert!(answer.is_ok());
    assert_eq!(last_toast(&model), Some("home is offline"));
    assert_eq!(model.player, Player::Stopped);
    assert_eq!(model.playlist.cursor.index(), 1);
}

#[test]
fn toggling_a_server_track_of_the_playlist_by_index_is_refused() {
    let mut model = album_model(online(), "flac");
    drop(browse(&mut model, BrowseRequest::PlaySelected));

    let answer = update(
        &mut model,
        Message::Queue(QueueRequest::ToggleAt(ViewIndex::new(1))),
        Moment::default(),
    );

    assert_eq!(answer, Err(Unhandled));
    assert_eq!(model.queue, Vec::new());
}

#[test]
fn queueing_a_server_track_from_the_local_catalog_is_refused() {
    let mut model = album_model(online(), "flac");
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    model.catalog_name = CatalogName::Local;
    model.workspace.browse.cursor = Cursor::at(3, 1);

    let answer = update(
        &mut model,
        Message::Queue(QueueRequest::Toggle),
        Moment::default(),
    );

    assert_eq!(answer, Err(Unhandled));
    assert_eq!(model.queue, Vec::new());
}

fn remove(model: &mut Model, server_name: ServerName) -> Result<Cmd, Unhandled> {
    update(
        model,
        Message::Server(ServerRequest::Remove(server_name)),
        Moment::default(),
    )
}

#[test]
fn remove_of_the_server_of_the_playing_track_stops_it_and_orders_no_more_chunks() {
    let mut model = playing_album_model();
    let revision = model.downloads[0].media_fetch.revision;

    let answer = remove(&mut model, home());

    assert!(answer.is_ok());
    assert_eq!(model.player, Player::Stopped);
    assert_eq!(model.downloads, Vec::new());
    assert_eq!(
        update(
            &mut model,
            Message::Elapsed(Timer::Fetch(revision)),
            Moment::default()
        ),
        Err(Unhandled)
    );
    assert_eq!(
        update(
            &mut model,
            Message::Remote(RemoteEvent::Fetched {
                revision,
                result: Ok(Fetched {
                    media_path: media_path(),
                    downloaded: MIB,
                    byte_len: 9 * MIB,
                }),
            }),
            Moment::default()
        ),
        Err(Unhandled)
    );
}

#[test]
fn remove_of_another_server_keeps_the_playing_track_and_its_download() {
    let mut model = playing_album_model();
    let mut work = model.servers[0].clone();
    work.account.server_name = ServerName::new("work");
    model.servers.push(work);
    let player = model.player.clone();
    let downloads = model.downloads.clone();

    let answer = remove(&mut model, ServerName::new("work"));

    assert!(answer.is_ok());
    assert_eq!(model.player, player);
    assert_eq!(model.downloads, downloads);
}

fn local_before_a_preloaded_server_track() -> Model {
    let mut model = playing_album_model();
    let local = Arc::new(Track::new(TrackParts {
        path: "/tmp/local.flac".into(),
        duration: Duration::from_secs(100),
        tags: Tags::default(),
        audio_format: AudioFormat::default(),
    }));
    model.playlist.tracks[1] = Arc::clone(&local);
    model.downloads.clear();
    model.player = Player::Playing {
        track: local,
        playhead: Playhead::anchored(
            Duration::ZERO,
            Moment::default(),
            Speed::default(),
        ),
        preloaded: None,
    };
    drop(update(
        &mut model,
        Message::Audio(AudioEvent::PositionReported(Duration::from_secs(95))),
        Moment::default(),
    ));
    let mark = model.revisions.lookahead;
    drop(update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(mark)),
        Moment::default(),
    ));
    model
}

#[test]
fn remove_of_the_server_of_the_preloaded_track_drops_it_and_its_download() {
    let mut model = local_before_a_preloaded_server_track();
    let successor = server_track(2, "flac");
    assert!(matches!(
        &model.player,
        Player::Playing { preloaded: Some(track), .. } if track.source() == successor.source()
    ));
    assert_eq!(model.downloads.len(), 1);

    let answer = remove(&mut model, home());

    assert!(answer.is_ok());
    assert!(matches!(
        model.player,
        Player::Playing {
            preloaded: None,
            ..
        }
    ));
    assert_eq!(model.downloads, Vec::new());
    let media_fetches = played_fetches(&mut model, Message::Audio(AudioEvent::Ended));
    assert_eq!(media_fetches, Vec::new());
    assert_eq!(model.player, Player::Stopped);
}

#[test]
fn remove_of_the_server_of_the_preloaded_track_cancels_its_preload() {
    let mut model = local_before_a_preloaded_server_track();

    let answer = remove(&mut model, home());

    assert!(answer.is_ok_and(|cmd| {
        cmd.effects()
            .any(|effect| matches!(effect, Effect::Audio(AudioCmd::CancelPreload)))
    }));
}
