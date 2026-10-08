use std::{sync::Arc, time::Duration};

use crate::{
    cmd::{AudioCmd, Cmd, Effect, TrackLoad},
    domain::{
        player::Player,
        revision::Revision,
        server::{Download, Fetched, RemoteError, ServerName, ServerStatus},
        track::{Track, TrackSource},
    },
    message::{Message, PlaybackRequest, RemoteEvent, Timer},
    update::{
        machine::Unhandled,
        player::{chunk, preload},
        server::ServerParts,
    },
};

const FETCH_RETRY: Duration = Duration::from_secs(5);

pub(crate) fn fetched(
    server_parts: ServerParts<'_>,
    revision: Revision,
    result: Result<Fetched, RemoteError>,
) -> Result<Cmd, Unhandled> {
    let ServerParts {
        servers,
        downloads,
        player,
        catalog_name: _,
        catalogs: _,
        revisions: _,
        favorites: _,
        overlay: _,
        play_reports: _,
    } = server_parts;
    let download = downloads
        .iter_mut()
        .find(|download| download.media_fetch.revision == revision)
        .ok_or(Unhandled)?;
    let first_byte = download
        .fetched
        .as_ref()
        .map_or(download.media_fetch.first_byte, |fetched| {
            fetched.downloaded
        });
    let answer = match result {
        Ok(fetched) => {
            let online = servers
                .iter()
                .find(|server| {
                    server.account.server_name == download.media_fetch.server_name
                        && matches!(server.server_status, ServerStatus::Offline(_))
                })
                .map_or_else(Cmd::none, |_server| {
                    Cmd::message(Message::Remote(RemoteEvent::Connected {
                        server_name: download.media_fetch.server_name.clone(),
                        session: download.media_fetch.session.clone(),
                    }))
                });
            if fetched.downloaded != first_byte || fetched.is_complete() {
                return Ok(progress(download, player, fetched).then(online));
            }
            online
        }
        Err(error) => Cmd::message(Message::Remote(RemoteEvent::Error(error))),
    };
    Ok(Cmd::from(Effect::After {
        delay: FETCH_RETRY,
        timer: Timer::Fetch(revision),
    })
    .then(answer))
}

fn progress(download: &mut Download, player: &Player, fetched: Fetched) -> Cmd {
    let ready = download.ready();
    let held = player
        .current()
        .into_iter()
        .chain(player.preloaded())
        .any(|track| track.holds(&download.media_fetch));
    let grow = (held && ready).then_some(AudioCmd::Grow {
        revision: download.media_fetch.revision,
        downloaded: fetched.downloaded,
    });
    download.fetched = Some(fetched);
    let track_load = player
        .current()
        .filter(|_track| held && !ready && matches!(player, Player::Loading(_)))
        .and_then(|track| TrackLoad::fetched(track, download));
    grow.into_iter()
        .chain(track_load.map(AudioCmd::Load))
        .chain(preload(download, player).filter(|_audio_cmd| !ready))
        .map(Effect::Audio)
        .chain(chunk(download, player).map(Effect::Remote))
        .collect()
}

pub(crate) fn forget(
    downloads: &mut Vec<Download>,
    player: &Player,
    server_name: &ServerName,
) -> Cmd {
    let served = |track: &Arc<Track>| {
        matches!(
            track.source(),
            TrackSource::Server {
                server_name: playing,
                ..
            } if playing == server_name
        )
    };
    let stops = player.current().is_some_and(served);
    let cancelled = player
        .preloaded()
        .filter(|track| !stops && served(track))
        .and_then(|track| {
            downloads
                .iter()
                .find(|download| track.holds(&download.media_fetch))
        })
        .map(|download| download.media_fetch.revision);
    downloads.retain(|download| {
        download.media_fetch.server_name != *server_name
            || cancelled == Some(download.media_fetch.revision)
    });
    if stops {
        return Cmd::message(Message::Playback(PlaybackRequest::Stop));
    }
    cancelled.map_or_else(Cmd::none, |revision| {
        Cmd::effect(Effect::Audio(AudioCmd::CancelPreload(revision)))
    })
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use rstest::rstest;

    use crate::{
        cmd::{AudioCmd, Cmd, Effect, GrowingMedia, Media, RemoteCmd, TrackLoad},
        domain::{
            driver::DriverName,
            model::Model,
            player::Player,
            revision::Revision,
            server::{
                CacheKey,
                Download,
                Endpoint,
                Fetched,
                MediaFetch,
                RemoteError,
                ServerName,
                ServerStatus,
                ServerTrackId,
                Session,
            },
            track::{Track, TrackSource},
        },
        message::{Message, RemoteEvent, Timer},
        update::{
            machine::Unhandled,
            player::retry,
            server::{
                download::FETCH_RETRY,
                tests::{model_with, server, session, unreachable},
                update,
            },
            server_parts,
            startup::startup_cmd,
        },
    };

    const MIB: u64 = 1024 * 1024;
    fn media_fetch(id: &str, first_byte: u64, steps: u64) -> MediaFetch {
        let server_name = ServerName::new("home");
        let server_track_id = ServerTrackId::new(id);
        MediaFetch {
            cache_key: CacheKey::new(&server_name, &server_track_id, "flac"),
            server_name,
            server_track_id,
            session: session(),
            first_byte,
            revision: (0..steps).fold(Revision::default(), |issued, _| issued.next()),
        }
    }

    fn fetched(downloaded: u64) -> Fetched {
        Fetched {
            media_path: PathBuf::from("/cache/home/tr-1.flac.part"),
            downloaded,
            byte_len: 9 * MIB,
        }
    }

    fn download(id: &str, steps: u64, fetched: Option<Fetched>) -> Download {
        Download {
            media_fetch: media_fetch(id, 0, steps),
            fetched,
        }
    }

    fn playing(id: &str) -> Player {
        Player::Loading(Arc::new(Track::from(TrackSource::Server {
            server_name: ServerName::new("home"),
            server_track_id: ServerTrackId::new(id),
        })))
    }

    fn answered(steps: u64, result: Result<Fetched, RemoteError>) -> RemoteEvent {
        RemoteEvent::Fetched {
            revision: media_fetch("", 0, steps).revision,
            result,
        }
    }

    fn chunk_model(
        player: Player,
        fetched: Option<Fetched>,
        server_status: ServerStatus,
    ) -> Model {
        let downloads = vec![download("tr-1", 1, fetched)];
        Model {
            player,
            ..model_with(vec![server(server_status)], downloads)
        }
    }

    #[rstest]
    #[case::incoming(chunk_model(Player::Stopped, None, ServerStatus::Online(session())), Cmd::from(Effect::Remote(RemoteCmd::Prefetch(media_fetch("tr-1", 4 * MIB, 1)))))]
    #[case::current(chunk_model(playing("tr-1"), None, ServerStatus::Online(session())), Cmd::from_iter([Effect::Audio(AudioCmd::Load(growing(4 * MIB))), Effect::Remote(RemoteCmd::Fetch(media_fetch("tr-1", 4 * MIB, 1)))]))]
    #[case::grown(chunk_model(playing("tr-1"), Some(fetched(MIB)), ServerStatus::Online(session())), Cmd::from_iter([Effect::Audio(AudioCmd::Grow { revision: media_fetch("tr-1", 0, 1).revision, downloaded: 4 * MIB }), Effect::Remote(RemoteCmd::Fetch(media_fetch("tr-1", 4 * MIB, 1)))]))]
    #[case::reconnected(chunk_model(Player::Stopped, None, ServerStatus::Online(Session::new(Endpoint::parse("https://music.example.com").unwrap(), "u=ann&t=fresh&s=pepper"))), Cmd::from(Effect::Remote(RemoteCmd::Prefetch(media_fetch("tr-1", 4 * MIB, 1)))))]
    #[case::offline(chunk_model(Player::Stopped, None, ServerStatus::Offline(unreachable("home"))), Cmd::from(Effect::Remote(RemoteCmd::Prefetch(media_fetch("tr-1", 4 * MIB, 1)))).then(Cmd::message(Message::Remote(RemoteEvent::Connected { server_name: ServerName::new("home"), session: session() }))))]
    fn a_chunk_answer_orders_the_next_chunk_and_keeps_a_newer_session(
        #[case] mut model: Model,
        #[case] cmd: Cmd,
    ) {
        let servers = model.servers.clone();
        let answer =
            update(server_parts(&mut model), answered(1, Ok(fetched(4 * MIB))));
        assert_eq!(answer, Ok(cmd));
        assert_eq!(
            model.downloads,
            vec![download("tr-1", 1, Some(fetched(4 * MIB)))]
        );
        assert_eq!(model.servers, servers);
    }

    #[rstest]
    #[case::last(fetched(8 * MIB), 9 * MIB, None)]
    #[case::lower(fetched(4 * MIB), MIB, Some(RemoteCmd::Prefetch(media_fetch("tr-1", MIB, 1))))]
    fn a_chunk_answer_records_its_progress_and_orders_the_next_chunk_from_it(
        #[case] known_fetched: Fetched,
        #[case] downloaded: u64,
        #[case] remote_cmd: Option<RemoteCmd>,
    ) {
        let mut model =
            model_with(Vec::new(), vec![download("tr-1", 1, Some(known_fetched))]);

        let answer = update(
            server_parts(&mut model),
            answered(1, Ok(fetched(downloaded))),
        );

        assert_eq!(
            answer,
            Ok(remote_cmd.map_or_else(Cmd::none, |remote_cmd| {
                Cmd::from(Effect::Remote(remote_cmd))
            }))
        );
        assert_eq!(
            model.downloads,
            vec![download("tr-1", 1, Some(fetched(downloaded)))]
        );
    }

    #[rstest]
    #[case::unknown_revision(answered(2, Ok(fetched(4 * MIB))))]
    #[case::unknown_revision_error(answered(2, Err(unreachable("home"))))]
    fn a_stale_answer_is_refused(#[case] event: RemoteEvent) {
        let downloads = vec![download("tr-1", 1, Some(fetched(4 * MIB)))];
        let mut model = model_with(Vec::new(), downloads.clone());

        let answer = update(server_parts(&mut model), event);

        assert_eq!(answer, Err(Unhandled));
        assert_eq!(model.downloads, downloads);
    }

    #[rstest]
    #[case::error(
        Err(unreachable("home")),
        Cmd::message(Message::Remote(RemoteEvent::Error(unreachable("home"))))
    )]
    #[case::no_progress(Ok(fetched(4 * MIB)), Cmd::none())]
    fn an_error_or_no_progress_retries_the_same_chunk_after_fetch_retry(
        #[case] result: Result<Fetched, RemoteError>,
        #[case] cmd: Cmd,
    ) {
        let downloads = vec![download("tr-1", 1, Some(fetched(4 * MIB)))];
        let mut model = model_with(Vec::new(), downloads.clone());
        let revision = media_fetch("tr-1", 0, 1).revision;

        let answer = update(server_parts(&mut model), answered(1, result));

        assert_eq!(
            answer,
            Ok(Cmd::from(Effect::After {
                delay: FETCH_RETRY,
                timer: Timer::Fetch(revision),
            })
            .then(cmd))
        );
        assert_eq!(model.downloads, downloads);
        assert_eq!(
            retry(&model.downloads, &model.player, revision),
            Ok(Cmd::from(Effect::Remote(RemoteCmd::Prefetch(media_fetch(
                "tr-1",
                4 * MIB,
                1
            )))))
        );
        assert_eq!(
            retry(
                &model.downloads,
                &model.player,
                media_fetch("", 0, 2).revision
            ),
            Err(Unhandled)
        );
    }

    fn growing(downloaded: u64) -> TrackLoad {
        let revision = media_fetch("tr-1", 0, 1).revision;
        TrackLoad {
            media: Media::Growing(GrowingMedia {
                media_path: PathBuf::from("/cache/home/tr-1.flac.part"),
                downloaded,
                byte_len: 9 * MIB,
                revision,
            }),
            decibels: None,
            revision,
        }
    }

    #[test]
    fn a_restarted_remote_driver_gets_each_unfinished_download_again() {
        let mut model = model_with(
            Vec::new(),
            vec![
                download("tr-1", 1, Some(fetched(4 * MIB))),
                download("tr-2", 2, Some(fetched(9 * MIB))),
                download("tr-3", 3, None),
            ],
        );
        model.player = playing("tr-1");

        let cmd = startup_cmd(&mut model, DriverName::Remote);

        assert_eq!(
            cmd,
            Cmd::from_iter([
                Effect::Remote(RemoteCmd::Fetch(media_fetch("tr-1", 4 * MIB, 1))),
                Effect::Remote(RemoteCmd::Prefetch(media_fetch("tr-3", 0, 3))),
            ])
        );
    }
}
