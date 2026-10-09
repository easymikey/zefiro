use std::{path::PathBuf, sync::Arc, time::Duration};

use kernel::{
    cmd::{AudioCmd, Cmd, Effect, GrowingMedia, Media, Playback, RemoteCmd, TrackLoad},
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

pub(crate) fn album_model(server_status: ServerStatus, format: &str) -> Model {
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

pub(crate) fn album_sources() -> Vec<TrackSource> {
    (0..3)
        .map(|track_number| server_track(track_number, "flac").source().clone())
        .collect()
}

pub(crate) fn playlist_sources(model: &Model) -> Vec<TrackSource> {
    model
        .playlist
        .tracks
        .iter()
        .map(|track| track.source().clone())
        .collect()
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

#[test]
fn a_completed_m4a_download_loads_after_the_play_order_it_keeps() {
    let mut model = album_model(online(), "m4a");
    let played = kernel::update::update(
        &mut model,
        Message::Browse(BrowseRequest::PlaySelected),
        Moment::default(),
    );
    let revision = model
        .downloads
        .first()
        .map_or_else(Revision::default, |download| download.media_fetch.revision);

    let fetched = kernel::update::update(
        &mut model,
        Message::Remote(RemoteEvent::Fetched {
            revision,
            result: Ok(Fetched {
                media_path: media_path(),
                downloaded: 9 * MIB,
                byte_len: 9 * MIB,
            }),
        }),
        Moment::default(),
    );

    let audio_cmds: Vec<AudioCmd> = played
        .into_iter()
        .flatten()
        .chain(fetched.into_iter().flatten())
        .filter_map(|effect| {
            if let Effect::Audio(
                audio_cmd @ (AudioCmd::SetPlayback(_) | AudioCmd::Load(_)),
            ) = effect
            {
                Some(audio_cmd)
            } else {
                None
            }
        })
        .collect();
    assert!(
        matches!(
            audio_cmds.as_slice(),
            [
                AudioCmd::SetPlayback(Playback::Playing),
                AudioCmd::Load(TrackLoad { media: Media::Local(path), .. }),
            ] if *path == media_path()
        ),
        "a played m4a must order Playing and then load the completed file, got {audio_cmds:?}"
    );
}

fn entered_album_model() -> Model {
    let mut model = album_model(online(), "flac");
    drop(browse(&mut model, BrowseRequest::PlaySelected));
    model
}

fn playing_album_model() -> Model {
    let mut model = entered_album_model();
    drop(update(
        &mut model,
        Message::Audio(AudioEvent::Loaded(None)),
        Moment::default(),
    ));
    model
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

fn local_catalog_model() -> Model {
    let mut model = entered_album_model();
    model.catalog_name = CatalogName::Local;
    model.workspace.browse.cursor = Cursor::at(3, 1);
    model
}

fn local_playlist_model() -> Model {
    let mut model = server_model(online(), 1);
    model.playlist = Playlist::from_tracks(vec![Arc::new(Track::from(
        TrackSource::Local(PathBuf::from("/music/a.flac")),
    ))]);
    model.workspace.browse.cursor = Cursor::at(1, 0);
    model
}

#[rstest]
#[case::toggle(entered_album_model(), QueueRequest::Toggle)]
#[case::play_next(entered_album_model(), QueueRequest::PlayNext)]
#[case::toggle_at_in_the_playlist(
    entered_album_model(),
    QueueRequest::ToggleAt(ViewIndex::new(1))
)]
#[case::toggle_from_the_local_catalog(local_catalog_model(), QueueRequest::Toggle)]
#[case::toggle_in_a_server_tab_over_a_local_playlist(
    local_playlist_model(),
    QueueRequest::Toggle
)]
#[case::play_next_in_a_server_tab_over_a_local_playlist(
    local_playlist_model(),
    QueueRequest::PlayNext
)]
fn queueing_a_server_track_is_refused(
    #[case] mut model: Model,
    #[case] queue_request: QueueRequest,
) {
    let answer = update(&mut model, Message::Queue(queue_request), Moment::default());

    assert_eq!(answer, Err(Unhandled));
    assert_eq!(model.queue, Vec::new());
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

#[rstest]
#[case::enter(
    album_model(online(), "flac"),
    Message::Browse(BrowseRequest::PlaySelected),
    1
)]
#[case::next(entered_album_model(), Message::Playback(PlaybackRequest::Next), 2)]
#[case::ended(playing_album_model(), Message::Audio(AudioEvent::Ended), 2)]
#[case::toggle_from_stopped(
    stopped_album_model(),
    Message::Playback(PlaybackRequest::Toggle),
    1
)]
fn playing_a_server_track_orders_a_fetch_from_byte_zero_with_a_new_revision(
    #[case] mut model: Model,
    #[case] message: Message,
    #[case] track_number: usize,
) {
    let revisions: Vec<Revision> = model
        .downloads
        .iter()
        .map(|download| download.media_fetch.revision)
        .collect();

    let media_fetches = played_fetches(&mut model, message);

    let media_fetch = fetch_from_zero(track_number, &media_fetches);
    assert_eq!(media_fetches, vec![media_fetch.clone()]);
    assert!(!revisions.contains(&media_fetch.revision));
    assert_eq!(
        model.downloads,
        vec![Download {
            media_fetch,
            fetched: None,
        }]
    );
    assert_eq!(model.playlist.cursor.index(), track_number);
    assert_eq!(
        model.player.current().map(|track| track.source().clone()),
        Some(server_track(track_number, "flac").source().clone())
    );
}

#[test]
fn enter_on_the_second_track_makes_the_album_the_playlist_and_empties_the_local_cursor_with_no_library()
 {
    let mut model = album_model(online(), "flac");
    model.workspace.browse.cursor = Cursor::at(60, 50);

    let answer = browse(&mut model, BrowseRequest::PlaySelected);

    assert!(answer.is_ok());
    assert_eq!(playlist_sources(&model), album_sources());
    assert_eq!(model.playlist_source, PlaylistSource::Server(home()));
    assert_eq!(model.workspace.browse.cursor, Cursor::at(0, 0));
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

#[rstest]
#[case::enter(
    album_model(online(), "flac"),
    Message::Browse(BrowseRequest::PlaySelected)
)]
#[case::next(entered_album_model(), Message::Playback(PlaybackRequest::Next))]
#[case::previous(playing_album_model(), Message::Playback(PlaybackRequest::Previous))]
#[case::toggle_from_stopped(
    stopped_album_model(),
    Message::Playback(PlaybackRequest::Toggle)
)]
fn playing_onto_a_track_of_an_offline_server_toasts_and_changes_nothing(
    #[case] mut model: Model,
    #[case] message: Message,
) {
    go_offline(&mut model);
    let player = model.player.clone();
    let playlist = model.playlist.clone();
    let downloads = model.downloads.clone();

    let answer = update(&mut model, message, Moment::default());

    assert!(answer.is_ok());
    assert_eq!(last_toast(&model), Some("home is offline"));
    assert_eq!(model.player, player);
    assert_eq!(model.playlist, playlist);
    assert_eq!(model.downloads, downloads);
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
        Message::Audio(AudioEvent::PositionReported {
            position: Duration::from_secs(95),
            revision: Revision::default(),
        }),
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

fn preloads_the_successor(model: &Model) -> bool {
    let successor = server_track(2, "flac");
    matches!(
        &model.player,
        Player::Playing { preloaded: Some(track), .. } if track.source() == successor.source()
    )
}

#[test]
fn remove_of_the_server_of_the_preloaded_track_keeps_it_and_its_download_until_the_engine_answers()
 {
    let mut model = local_before_a_preloaded_server_track();
    assert!(preloads_the_successor(&model));
    let downloads = model.downloads.clone();
    assert_eq!(downloads.len(), 1);

    let revision = downloads[0].media_fetch.revision;

    let answer = remove(&mut model, home());

    assert!(answer.is_ok_and(|cmd| {
        cmd.effects()
            .any(|effect| *effect == Effect::Audio(AudioCmd::CancelPreload(revision)))
    }));
    assert!(preloads_the_successor(&model));
    assert_eq!(model.downloads, downloads);
}

#[test]
fn remove_of_the_server_of_the_preloaded_track_drops_it_and_its_download_once_the_preload_is_cancelled()
 {
    let mut model = local_before_a_preloaded_server_track();
    let revision = model.downloads[0].media_fetch.revision;
    drop(remove(&mut model, home()));

    let answer = update(
        &mut model,
        Message::Audio(AudioEvent::PreloadCancelled(revision)),
        Moment::default(),
    );

    assert_eq!(answer, Ok(Cmd::none()));
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
fn a_stale_preload_cancelled_leaves_the_new_preload_and_its_download() {
    let mut model = local_before_a_preloaded_server_track();
    let server = model.servers[0].clone();
    let revision = model.downloads[0].media_fetch.revision;
    drop(remove(&mut model, home()));
    model.servers.push(server);
    for message in [
        Message::Playback(PlaybackRequest::Stop),
        Message::Playback(PlaybackRequest::Play),
        Message::Audio(AudioEvent::Loaded(None)),
        Message::Audio(AudioEvent::PositionReported {
            position: Duration::from_secs(95),
            revision: Revision::default(),
        }),
    ] {
        drop(update(&mut model, message, Moment::default()));
    }
    let mark = model.revisions.lookahead;
    drop(update(
        &mut model,
        Message::Elapsed(Timer::Lookahead(mark)),
        Moment::default(),
    ));
    let downloads = model.downloads.clone();
    assert!(preloads_the_successor(&model));

    let answer = update(
        &mut model,
        Message::Audio(AudioEvent::PreloadCancelled(revision)),
        Moment::default(),
    );

    assert_eq!(answer, Err(Unhandled));
    assert!(preloads_the_successor(&model));
    assert_eq!(model.downloads, downloads);
}

#[test]
fn remove_of_the_server_of_the_preloaded_track_plays_it_when_the_engine_keeps_the_preload()
 {
    let mut model = local_before_a_preloaded_server_track();
    let successor = server_track(2, "flac");
    let downloads = model.downloads.clone();
    let revision = downloads[0].media_fetch.revision;
    drop(remove(&mut model, home()));

    let kept = update(
        &mut model,
        Message::Audio(AudioEvent::PreloadKept(revision)),
        Moment::default(),
    );
    let changed = update(
        &mut model,
        Message::Audio(AudioEvent::TrackChanged),
        Moment::default(),
    );

    assert_eq!(kept, Ok(Cmd::none()));
    assert!(changed.is_ok());
    assert!(matches!(
        &model.player,
        Player::Playing { track, .. } if track.source() == successor.source()
    ));
    assert_eq!(model.downloads, downloads);
}

fn artwork() -> kernel::domain::server::Artwork {
    kernel::domain::server::Artwork {
        server_name: home(),
        id: Arc::from("al-0"),
    }
}

fn cover_album_model() -> Model {
    let mut model = album_model(online(), "flac");
    if let Some(album_level) = model.catalogs[0].album_level.as_mut() {
        album_level.catalog_rows = (0..3)
            .map(|track_number| {
                CatalogRow::Track(Arc::new(
                    server_track(track_number, "flac").with_cover(Some(artwork())),
                ))
            })
            .collect();
    }
    model.workspace.cover_side = Some(kernel::domain::geometry::Pixels(300));
    model
}

fn cover_effects(model: &mut Model, message: Message) -> Vec<Effect> {
    kernel::update::update(model, message, Moment::default())
        .unwrap()
        .into_iter()
        .filter(|effect| {
            matches!(
                effect,
                Effect::Remote(RemoteCmd::Cover { .. })
                    | Effect::Library(kernel::cmd::LibraryCmd::DecodeCover(_))
            )
        })
        .collect()
}

#[test]
fn a_server_track_asks_for_its_cover_once_and_the_next_track_of_the_album_asks_nothing()
{
    let mut model = cover_album_model();

    let started =
        cover_effects(&mut model, Message::Browse(BrowseRequest::PlaySelected));
    let answered = cover_effects(
        &mut model,
        Message::Remote(RemoteEvent::Cover {
            artwork: artwork(),
            result: Ok(PathBuf::from("/cache/covers/home/al-0/cover.jpg")),
        }),
    );
    let next = cover_effects(&mut model, Message::Playback(PlaybackRequest::Next));

    assert_eq!(
        started,
        vec![Effect::Remote(RemoteCmd::Cover {
            session: session(),
            artwork: artwork(),
        })]
    );
    assert_eq!(
        answered,
        vec![Effect::Library(kernel::cmd::LibraryCmd::DecodeCover(
            kernel::cmd::CoverJob {
                path: PathBuf::from("/cache/covers/home/al-0/cover.jpg"),
                side: kernel::domain::geometry::Pixels(300),
            }
        ))]
    );
    assert_eq!(next, Vec::new());
}

#[test]
fn a_failed_cover_fetch_leaves_the_empty_cover_and_no_toast() {
    let mut model = cover_album_model();
    drop(browse(&mut model, BrowseRequest::PlaySelected));

    let effects = cover_effects(
        &mut model,
        Message::Remote(RemoteEvent::Cover {
            artwork: artwork(),
            result: Err(RemoteError::Moved {
                server_name: home(),
            }),
        }),
    );

    assert_eq!(effects, Vec::new());
    assert!(model.covers.is_empty());
    assert!(model.workspace.toasts.is_empty());
}

#[test]
fn a_stored_artwork_of_another_server_with_the_same_id_asks_for_the_cover() {
    let mut model = cover_album_model();
    model.covers.insert(
        kernel::domain::server::Artwork {
            server_name: ServerName::new("work"),
            id: Arc::from("al-0"),
        },
        PathBuf::from("/cache/covers/work/al-0/cover.jpg"),
    );

    let started =
        cover_effects(&mut model, Message::Browse(BrowseRequest::PlaySelected));

    assert_eq!(
        started,
        vec![Effect::Remote(RemoteCmd::Cover {
            session: session(),
            artwork: artwork(),
        })]
    );
}
