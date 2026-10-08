use std::{
    collections::VecDeque,
    convert::Infallible,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use kernel::{
    cmd::{Cmd, Cmds, RemoteCmd},
    domain::{
        revision::Revision,
        server::{
            Account,
            CacheKey,
            Connection,
            MediaFetch,
            RemoteError,
            ServerName,
            Session,
        },
    },
    message::RemoteEvent,
    update::machine::{Driver, LoopCmd, LoopEffect, Machine, Unhandled},
};

use crate::{
    job::RemoteJob,
    message::{RemoteMessage, RemoteTimer},
};

pub const SEARCH_WAIT: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub struct RemoteDriver {
    connections: VecDeque<Connection>,
    account: Option<Account>,
    job: Option<RemoteJob>,
    remote_job: Option<RemoteJob>,
    revision: Option<Revision>,
    media_dir: Arc<Path>,
    fetch_cache_key: Option<CacheKey>,
    prefetch_cache_key: Option<CacheKey>,
}

impl RemoteDriver {
    #[must_use]
    pub fn new(media_dir: PathBuf) -> Self {
        Self {
            connections: VecDeque::new(),
            account: None,
            job: None,
            remote_job: None,
            revision: None,
            media_dir: Arc::from(media_dir),
            fetch_cache_key: None,
            prefetch_cache_key: None,
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

    fn enqueue(&mut self, remote_cmd: RemoteCmd) -> <Self as Machine>::Effect {
        match remote_cmd {
            RemoteCmd::Connect(connection) => self.connect(connection),
            RemoteCmd::List {
                server_name,
                session,
                listing,
                page,
                revision,
            } => {
                self.job = Some(RemoteJob::List {
                    server_name,
                    session,
                    listing,
                    page,
                    revision,
                });
                Cmd::none()
            }
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
                    return Cmd::message(RemoteEvent::Found {
                        server_name,
                        catalog_rows: Vec::new(),
                        revision,
                    });
                }
                let job = RemoteJob::Search {
                    server_name,
                    session,
                    input,
                    revision,
                };
                self.search(job, revision)
            }
        }
    }

    fn advance(&mut self) -> <Self as Machine>::Effect {
        let connect = if self.account.is_some() {
            None
        } else {
            self.connections.pop_front().map(|connection| {
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
        connect
            .into_iter()
            .chain(list)
            .fold(Cmd::none(), |cmd, job| {
                cmd.then(Cmd::effect(LoopEffect::Run(job)))
            })
    }

    fn connected(
        &mut self,
        server_name: ServerName,
        result: Result<Session, RemoteError>,
    ) -> Result<RemoteEvent, Unhandled> {
        self.account
            .take_if(|account| account.server_name == server_name)
            .ok_or(Unhandled)?;
        Ok(match result {
            Ok(session) => RemoteEvent::Connected {
                server_name,
                session,
            },
            Err(error) => RemoteEvent::Error(error),
        })
    }
}

impl Machine for RemoteDriver {
    type Message = RemoteMessage;
    type Effect = LoopCmd<Infallible, RemoteJob, RemoteMessage, RemoteEvent>;

    fn transition(
        &mut self,
        message: RemoteMessage,
    ) -> Result<Self::Effect, Unhandled> {
        match message {
            RemoteMessage::Started => Ok(Cmd::none()),
            RemoteMessage::Cmds(cmds) => self.take_cmds(cmds),
            RemoteMessage::Connected {
                server_name,
                result,
                stored,
            } => {
                let answer = self.connected(server_name, result)?;
                Ok(Cmd::message(answer)
                    .then(stored.err().map_or_else(Cmd::none, |error| {
                        Cmd::message(RemoteEvent::Error(error))
                    }))
                    .then(self.advance()))
            }
            RemoteMessage::Listed {
                server_name,
                listing,
                page,
                result,
                revision,
            } => {
                if self.revision != Some(revision) {
                    return Err(Unhandled);
                }
                self.revision = None;
                let answer = match result {
                    Ok(catalog_rows) => RemoteEvent::Listed {
                        server_name,
                        listing,
                        page,
                        catalog_rows,
                        revision,
                    },
                    Err(error) => RemoteEvent::Error(error),
                };
                Ok(Cmd::message(answer).then(self.advance()))
            }
            RemoteMessage::Fetched { revision, result } => {
                Ok(Cmd::message(RemoteEvent::Fetched { revision, result }))
            }
            RemoteMessage::Elapsed(RemoteTimer::Search(revision)) => self
                .remote_job
                .take_if(|job| job.revision() == Some(revision))
                .map(|job| Cmd::effect(LoopEffect::Run(job)))
                .ok_or(Unhandled),
            RemoteMessage::Found {
                server_name,
                result,
                revision,
            } => Ok(Cmd::message(match result {
                Ok(catalog_rows) => RemoteEvent::Found {
                    server_name,
                    catalog_rows,
                    revision,
                },
                Err(error) => RemoteEvent::Error(error),
            })),
        }
    }
}

impl Driver for RemoteDriver {
    type Effect = Infallible;

    fn execute(&mut self, effect: Infallible) -> Option<RemoteMessage> {
        match effect {}
    }
}
