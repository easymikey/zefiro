use std::{
    collections::VecDeque,
    mem,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use kernel::{
    cmd::{Cmd, Cmds, RemoteCmd},
    domain::{
        favorites::{Favorite, Favorites},
        io_error::IoError,
        revision::Revision,
        server::{
            Account,
            CacheKey,
            Connection,
            MediaFetch,
            PlayReport,
            RemoteError,
            Scrobble,
            ServerName,
            ServerTrackId,
            Session,
        },
        track::CatalogRow,
    },
    message::RemoteEvent,
    update::machine::{Driver, LoopCmd, LoopEffect, Machine, Unhandled},
};

use crate::{
    job::{RemoteJob, SignedReport, flush},
    message::{RemoteMessage, RemoteTimer},
};

pub const SEARCH_WAIT: Duration = Duration::from_millis(250);
pub const REPORT_RETRY: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteEffect {
    Restore,
    Flush(Vec<PlayReport>),
}

#[derive(Debug)]
pub struct RemoteDriver {
    connections: VecDeque<Connection>,
    account: Option<Account>,
    job: Option<RemoteJob>,
    remote_job: Option<RemoteJob>,
    remote_jobs: VecDeque<RemoteJob>,
    revision: Option<Revision>,
    media_dir: Arc<Path>,
    fetch_cache_key: Option<CacheKey>,
    prefetch_cache_key: Option<CacheKey>,
    signed_reports: Vec<SignedReport>,
    play_reports: Vec<PlayReport>,
    reporting_server_name: Option<ServerName>,
    remote_errors: Vec<RemoteError>,
    reports_path: PathBuf,
}

impl RemoteDriver {
    #[must_use]
    pub fn new(media_dir: PathBuf, reports_path: PathBuf) -> Self {
        Self {
            connections: VecDeque::new(),
            account: None,
            job: None,
            remote_job: None,
            remote_jobs: VecDeque::new(),
            revision: None,
            media_dir: Arc::from(media_dir),
            fetch_cache_key: None,
            prefetch_cache_key: None,
            signed_reports: Vec::new(),
            play_reports: Vec::new(),
            reporting_server_name: None,
            remote_errors: Vec::new(),
            reports_path,
        }
    }

    fn take_cmds(
        &mut self,
        cmds: Cmds<RemoteCmd>,
    ) -> Result<<Self as Machine>::Effect, Unhandled> {
        if cmds.cmds.is_empty() {
            return Err(Unhandled);
        }
        let answer = cmds
            .cmds
            .into_iter()
            .fold(Cmd::none(), |answer, remote_cmd| {
                answer.then(self.enqueue(remote_cmd))
            });
        Ok(answer.then(self.advance()))
    }

    fn fetch_chunk(&mut self, media_fetch: MediaFetch) -> <Self as Machine>::Effect {
        self.fetch_cache_key = Some(media_fetch.cache_key.clone());
        Cmd::effect(LoopEffect::Run(RemoteJob::Fetch {
            media_fetch,
            media_dir: Arc::clone(&self.media_dir),
            kept_cache_keys: self.kept_cache_keys(),
        }))
    }

    fn prefetch_chunk(&mut self, media_fetch: MediaFetch) -> <Self as Machine>::Effect {
        self.prefetch_cache_key = Some(media_fetch.cache_key.clone());
        Cmd::effect(LoopEffect::Run(RemoteJob::Prefetch {
            media_fetch,
            media_dir: Arc::clone(&self.media_dir),
            kept_cache_keys: self.kept_cache_keys(),
        }))
    }

    fn kept_cache_keys(&self) -> Vec<CacheKey> {
        self.fetch_cache_key
            .iter()
            .chain(&self.prefetch_cache_key)
            .cloned()
            .collect()
    }

    fn connect(&mut self, connection: Connection) -> <Self as Machine>::Effect {
        let server_name = &connection.account.server_name;
        match self
            .connections
            .iter_mut()
            .find(|waiting| waiting.account.server_name == *server_name)
        {
            Some(waiting) => *waiting = connection,
            None => self.connections.push_back(connection),
        }
        Cmd::none()
    }

    fn search(
        &mut self,
        job: RemoteJob,
        revision: Revision,
    ) -> <Self as Machine>::Effect {
        self.remote_job = Some(job);
        Cmd::effect(LoopEffect::After {
            delay: SEARCH_WAIT,
            message: RemoteMessage::Elapsed(RemoteTimer::Search(revision)),
        })
    }

    fn list(&mut self, job: RemoteJob) -> <Self as Machine>::Effect {
        self.job = Some(job);
        Cmd::none()
    }

    fn enqueue(&mut self, remote_cmd: RemoteCmd) -> <Self as Machine>::Effect {
        match remote_cmd {
            RemoteCmd::Connect(connection) => self.connect(connection),
            RemoteCmd::Forget(account) => self.forget(account),
            RemoteCmd::List {
                server_name,
                session,
                listing,
                page,
                revision,
            } => self.list(RemoteJob::List {
                server_name,
                session,
                listing,
                page,
                revision,
            }),
            RemoteCmd::Fetch(media_fetch) => self.fetch_chunk(media_fetch),
            RemoteCmd::Prefetch(media_fetch) => self.prefetch_chunk(media_fetch),
            RemoteCmd::Search {
                server_name,
                session,
                input,
                revision,
            } => {
                if input.trim().is_empty() {
                    self.remote_job = None;
                    return found(
                        server_name,
                        Ok((Vec::new(), Favorites::default())),
                        revision,
                    );
                }
                let job = RemoteJob::Search {
                    server_name,
                    session,
                    input,
                    revision,
                };
                self.search(job, revision)
            }
            RemoteCmd::Star {
                server_name,
                session,
                server_track_id,
                favorite,
            } => self.queue(RemoteJob::Star {
                server_name,
                session,
                server_track_id,
                favorite,
            }),
            RemoteCmd::Report {
                session,
                play_report,
            } => self.push(SignedReport {
                session,
                play_report,
            }),
            RemoteCmd::Flush(play_reports) => self.flush(play_reports),
        }
    }

    fn forget(&mut self, account: Account) -> <Self as Machine>::Effect {
        let server_name = Some(&account.server_name);
        self.connections
            .retain(|waiting| waiting.account.server_name != account.server_name);
        self.job = self
            .job
            .take()
            .filter(|job| job.server_name() != server_name);
        self.remote_job = self
            .remote_job
            .take()
            .filter(|job| job.server_name() != server_name);
        let mut remote_jobs = mem::take(&mut self.remote_jobs).into_iter();
        self.remote_jobs = remote_jobs
            .next()
            .into_iter()
            .chain(remote_jobs.filter(|job| job.server_name() != server_name))
            .collect();
        self.signed_reports.retain(|signed_report| {
            signed_report.play_report.server_name != account.server_name
        });
        self.play_reports
            .retain(|play_report| play_report.server_name != account.server_name);
        self.remote_errors
            .retain(|remote_error| *remote_error.server_name() != account.server_name);
        self.queue(RemoteJob::Forget(account)).then(self.save())
    }

    fn push(&mut self, signed_report: SignedReport) -> <Self as Machine>::Effect {
        if let Some(index) = self
            .play_reports
            .iter()
            .position(|play_report| *play_report == signed_report.play_report)
        {
            self.play_reports.remove(index);
        }
        let scrobble = signed_report.play_report.scrobble;
        self.signed_reports.push(signed_report);
        match scrobble {
            Scrobble::Played(_moment) => self.save(),
            Scrobble::NowPlaying => Cmd::none(),
        }
    }

    fn flush(&mut self, play_reports: Vec<PlayReport>) -> <Self as Machine>::Effect {
        self.play_reports = play_reports;
        self.save()
    }

    fn save(&self) -> <Self as Machine>::Effect {
        let signed_reports = self
            .signed_reports
            .iter()
            .map(|signed_report| &signed_report.play_report);
        Cmd::effect(LoopEffect::Execute(RemoteEffect::Flush(
            self.play_reports
                .iter()
                .chain(signed_reports)
                .cloned()
                .collect(),
        )))
    }

    fn report(&mut self) -> Option<RemoteJob> {
        if self.reporting_server_name.is_some() {
            return None;
        }
        let server_name = self.signed_reports.first()?.play_report.server_name.clone();
        let signed_reports = self
            .signed_reports
            .iter()
            .filter(|signed_report| {
                signed_report.play_report.server_name == server_name
            })
            .cloned()
            .collect();
        self.reporting_server_name = Some(server_name);
        Some(RemoteJob::Report(signed_reports))
    }

    fn reported(
        &mut self,
        play_reports: Vec<PlayReport>,
        result: Result<(), RemoteError>,
    ) -> Result<<Self as Machine>::Effect, Unhandled> {
        let server_name = self.reporting_server_name.clone().ok_or(Unhandled)?;
        for play_report in play_reports {
            if let Some(index) = self
                .signed_reports
                .iter()
                .position(|signed_report| signed_report.play_report == play_report)
            {
                self.signed_reports.remove(index);
            }
        }
        let answer = match result {
            Ok(()) => {
                self.reporting_server_name = None;
                self.remote_errors
                    .retain(|remote_error| *remote_error.server_name() != server_name);
                self.advance()
            }
            Err(error) if error.is_refusal() => {
                if let Some(index) =
                    self.signed_reports.iter().position(|signed_report| {
                        signed_report.play_report.server_name == server_name
                    })
                {
                    self.signed_reports.remove(index);
                }
                self.reporting_server_name = None;
                self.remote_errors
                    .retain(|remote_error| *remote_error.server_name() != server_name);
                Cmd::message(RemoteEvent::Error(error)).then(self.advance())
            }
            Err(error) => {
                self.signed_reports.retain(|signed_report| {
                    signed_report.play_report.server_name != server_name
                        || played(&signed_report.play_report)
                });
                let (failed, others): (Vec<_>, Vec<_>) =
                    mem::take(&mut self.signed_reports).into_iter().partition(
                        |signed_report| {
                            signed_report.play_report.server_name == server_name
                        },
                    );
                self.signed_reports = others.into_iter().chain(failed).collect();
                let event = if self.remote_errors.contains(&error) {
                    Cmd::none()
                } else {
                    self.remote_errors.retain(|remote_error| {
                        remote_error.server_name() != error.server_name()
                    });
                    self.remote_errors.push(error.clone());
                    Cmd::message(RemoteEvent::Error(error))
                };
                event.then(Cmd::effect(LoopEffect::After {
                    delay: REPORT_RETRY,
                    message: RemoteMessage::Elapsed(RemoteTimer::Retry),
                }))
            }
        };
        Ok(answer.then(self.save()))
    }

    fn restored(
        &mut self,
        result: Result<Vec<PlayReport>, IoError>,
    ) -> <Self as Machine>::Effect {
        if let Ok(play_reports) = &result {
            self.play_reports.clone_from(play_reports);
        }
        if result.as_ref().is_ok_and(Vec::is_empty) {
            return Cmd::none();
        }
        Cmd::message(RemoteEvent::Restored(result))
    }

    fn retry(&mut self) -> Result<<Self as Machine>::Effect, Unhandled> {
        self.reporting_server_name.take().ok_or(Unhandled)?;
        Ok(self.advance())
    }

    fn advance(&mut self) -> <Self as Machine>::Effect {
        let connect = if self.account.is_some() {
            None
        } else {
            self.connections
                .iter()
                .position(|connection| {
                    !self
                        .remote_jobs
                        .iter()
                        .any(|job| forgotten(job, &connection.account.server_name))
                })
                .and_then(|index| self.connections.remove(index))
                .map(|connection| {
                    self.account = Some(connection.account.clone());
                    RemoteJob::Connect(connection)
                })
        };
        let list = if self.revision.is_some() {
            None
        } else {
            self.job
                .take()
                .inspect(|job| self.revision = job.revision())
        };
        let report = self.report();
        connect
            .into_iter()
            .chain(list)
            .chain(report)
            .fold(Cmd::none(), |cmd, job| {
                cmd.then(Cmd::effect(LoopEffect::Run(job)))
            })
    }

    fn connected(
        &mut self,
        server_name: ServerName,
        result: Result<Session, RemoteError>,
    ) -> Result<<Self as Machine>::Effect, Unhandled> {
        let held = self.remote_jobs.front().is_some_and(|job| self.held(job));
        self.account
            .take_if(|account| account.server_name == server_name)
            .ok_or(Unhandled)?;
        let answer = match result {
            Ok(session) => {
                self.remote_errors
                    .retain(|remote_error| *remote_error.server_name() != server_name);
                RemoteEvent::Connected {
                    server_name,
                    session,
                }
            }
            Err(error) => RemoteEvent::Error(error),
        };
        let forget = if held { self.start() } else { Cmd::none() };
        Ok(Cmd::message(answer).then(forget))
    }

    fn listed(
        &mut self,
        revision: Revision,
        answer: Result<RemoteEvent, RemoteError>,
    ) -> Result<<Self as Machine>::Effect, Unhandled> {
        self.revision
            .take_if(|sent| *sent == revision)
            .map(drop)
            .ok_or(Unhandled)?;
        Ok(
            Cmd::message(answer.unwrap_or_else(RemoteEvent::Error))
                .then(self.advance()),
        )
    }

    fn held(&self, job: &RemoteJob) -> bool {
        self.account
            .as_ref()
            .is_some_and(|account| forgotten(job, &account.server_name))
    }

    fn start(&self) -> <Self as Machine>::Effect {
        self.remote_jobs
            .front()
            .filter(|job| !self.held(job))
            .cloned()
            .map_or_else(Cmd::none, |job| Cmd::effect(LoopEffect::Run(job)))
    }

    fn queue(&mut self, job: RemoteJob) -> <Self as Machine>::Effect {
        self.remote_jobs.push_back(job);
        if self.remote_jobs.len() == 1 {
            self.start()
        } else {
            Cmd::none()
        }
    }

    fn dequeue(
        &mut self,
        result: Result<(), RemoteError>,
    ) -> Result<<Self as Machine>::Effect, Unhandled> {
        self.remote_jobs.pop_front().map(drop).ok_or(Unhandled)?;
        Ok(reported(result.err())
            .then(self.start())
            .then(self.advance()))
    }

    fn elapsed(
        &mut self,
        remote_timer: RemoteTimer,
    ) -> Result<<Self as Machine>::Effect, Unhandled> {
        match remote_timer {
            RemoteTimer::Search(revision) => self
                .remote_job
                .take_if(|job| job.revision() == Some(revision))
                .map(|job| Cmd::effect(LoopEffect::Run(job)))
                .ok_or(Unhandled),
            RemoteTimer::Retry => self.retry(),
        }
    }
}

fn reported(error: Option<RemoteError>) -> <RemoteDriver as Machine>::Effect {
    error.map_or_else(Cmd::none, |error| Cmd::message(RemoteEvent::Error(error)))
}

fn forgotten(job: &RemoteJob, server_name: &ServerName) -> bool {
    matches!(job, RemoteJob::Forget(account) if account.server_name == *server_name)
}

fn played(play_report: &PlayReport) -> bool {
    matches!(play_report.scrobble, Scrobble::Played(_))
}

fn saved(result: Result<(), IoError>) -> <RemoteDriver as Machine>::Effect {
    result.map_or_else(
        |io_error| Cmd::message(RemoteEvent::Unsaved(io_error)),
        |()| Cmd::none(),
    )
}

fn starred(
    server_name: ServerName,
    server_track_id: ServerTrackId,
    favorite: Favorite,
) -> RemoteEvent {
    RemoteEvent::Starred {
        server_name,
        server_track_id,
        favorite,
    }
}

fn found(
    server_name: ServerName,
    result: Result<(Vec<CatalogRow>, Favorites), RemoteError>,
    revision: Revision,
) -> <RemoteDriver as Machine>::Effect {
    Cmd::message(RemoteEvent::Found {
        server_name,
        result,
        revision,
    })
}

impl Machine for RemoteDriver {
    type Message = RemoteMessage;
    type Effect = LoopCmd<RemoteEffect, RemoteJob, RemoteMessage, RemoteEvent>;

    fn transition(
        &mut self,
        message: RemoteMessage,
    ) -> Result<Self::Effect, Unhandled> {
        match message {
            RemoteMessage::Started => {
                Ok(Cmd::effect(LoopEffect::Execute(RemoteEffect::Restore)))
            }
            RemoteMessage::Restored(result) => Ok(self.restored(result)),
            RemoteMessage::Cmds(cmds) => self.take_cmds(cmds),
            RemoteMessage::Connected {
                server_name,
                result,
                stored,
            } => self
                .connected(server_name, result)
                .map(|answer| answer.then(reported(stored.err())).then(self.advance())),
            RemoteMessage::Listed {
                server_name,
                listing,
                page,
                result,
                revision,
            } => self.listed(
                revision,
                result.map(|(catalog_rows, favorites)| RemoteEvent::Listed {
                    server_name,
                    listing,
                    page,
                    catalog_rows,
                    favorites,
                    revision,
                }),
            ),
            RemoteMessage::Fetched { revision, result } => {
                Ok(Cmd::message(RemoteEvent::Fetched { revision, result }))
            }
            RemoteMessage::Elapsed(remote_timer) => self.elapsed(remote_timer),
            RemoteMessage::Found {
                server_name,
                result,
                revision,
            } => Ok(found(server_name, result, revision)),
            RemoteMessage::Starred {
                server_name,
                server_track_id,
                favorite,
                result,
            } => {
                let favorite = result.as_ref().map_or(!favorite, |()| favorite);
                let remote_event = starred(server_name, server_track_id, favorite);
                Ok(Cmd::message(remote_event).then(self.dequeue(result)?))
            }
            RemoteMessage::Forgotten(result) => self.dequeue(result),
            RemoteMessage::Reported {
                play_reports,
                result,
            } => self.reported(play_reports, result),
            RemoteMessage::Saved(result) => Ok(saved(result)),
        }
    }
}

impl Driver for RemoteDriver {
    type Effect = RemoteEffect;

    fn execute(&mut self, remote_effect: RemoteEffect) -> Option<RemoteMessage> {
        match remote_effect {
            RemoteEffect::Restore => Some(RemoteMessage::Restored(
                crate::job::restored(&self.reports_path),
            )),
            RemoteEffect::Flush(play_reports) => Some(RemoteMessage::Saved(flush(
                &self.reports_path,
                &play_reports,
            ))),
        }
    }
}
