use std::path::PathBuf;

use kernel::{
    cmd::{Cmd, CoverJob},
    domain::{config::Diagnostic, revision::Revision},
    message::{LibraryError, LibraryEvent},
    update::machine::{LoopEffect, Machine, Unhandled},
};

use crate::{
    cover::{
        CoverDecoded,
        CoverError,
        CoverLookup,
        decoding::{CoverDecoding, CoverDecodingMessage},
    },
    driver::{LibraryDriver, LibraryEffect, LibraryLoopCmd, drained},
    job::LibraryJob,
};

impl<P> LibraryDriver<P> {
    pub(crate) fn decode_cover(
        &mut self,
        cover_job: CoverJob,
    ) -> Result<LibraryLoopCmd, Unhandled> {
        if self.wanted_cover_job.as_ref() == Some(&cover_job) {
            return Err(Unhandled);
        }
        let cmd = match self.cover_cache.cached(&cover_job).cloned() {
            Some(decoded) => {
                self.cover_cache.remember(&decoded);
                Cmd::effect(LoopEffect::Execute(LibraryEffect::PublishCover(decoded)))
            }
            None if self.decoding.busy() == Some(&cover_job) => Cmd::none(),
            None => self.decode(cover_job.clone())?,
        };
        self.wanted_cover_job = Some(cover_job);
        Ok(cmd)
    }

    pub(crate) fn prefetch(
        &mut self,
        path: PathBuf,
    ) -> Result<LibraryLoopCmd, Unhandled> {
        let side = self
            .wanted_cover_job
            .as_ref()
            .map(|wanted| wanted.side)
            .ok_or(Unhandled)?;
        let cover_job = CoverJob { path, side };
        if self.decoding != CoverDecoding::Idle
            || self.cover_cache.cached(&cover_job).is_some()
        {
            return Err(Unhandled);
        }
        self.decode(cover_job)
    }

    fn decode(&mut self, cover_job: CoverJob) -> Result<LibraryLoopCmd, Unhandled> {
        let revision = self.cover_revision.next();
        let started = self.decoding.transition(CoverDecodingMessage::Decode {
            job: cover_job,
            revision,
        })?;
        self.cover_revision = revision;
        Ok(lift_decoding(started))
    }

    pub(crate) fn decoded(
        &mut self,
        revision: Revision,
        decoded: Result<CoverDecoded, CoverError>,
    ) -> Result<LibraryLoopCmd, Unhandled> {
        let is_current =
            self.decoding.busy().is_some() && revision == self.cover_revision;
        let settled = if decoded.is_err() || is_current {
            lift_decoding(
                self.decoding
                    .transition(CoverDecodingMessage::Decoded(revision))?,
            )
        } else {
            Cmd::none()
        };
        if let Ok(decoded) = &decoded {
            self.cover_cache.remember(decoded);
        }
        let answer = match decoded {
            Ok(decoded) => self.published(decoded),
            Err(error) => self.failed(&error),
        };
        Ok(settled.then(answer))
    }

    fn published(&self, decoded: CoverDecoded) -> LibraryLoopCmd {
        let is_wanted = self.wanted_cover_job.as_ref().is_some_and(|cover_job| {
            cover_job.path == decoded.path && cover_job.side == decoded.side
        });
        if is_wanted {
            Cmd::effect(LoopEffect::Execute(LibraryEffect::PublishCover(decoded)))
        } else {
            Cmd::none()
        }
    }

    fn failed(&self, error: &CoverError) -> LibraryLoopCmd {
        let Some(cover_job) = self
            .wanted_cover_job
            .as_ref()
            .filter(|cover_job| cover_job.path == error.path)
        else {
            return Cmd::none();
        };
        Cmd::effect(LoopEffect::Execute(LibraryEffect::PublishCover(
            CoverDecoded {
                path: cover_job.path.clone(),
                side: cover_job.side,
                cover_lookup: CoverLookup::Missing,
            },
        )))
        .then(Cmd::message(LibraryEvent::Error(
            LibraryError::DecodeCover {
                path: error.path.clone(),
                diagnostic: Diagnostic::from_error(error),
            },
        )))
    }
}

fn lift_decoding(cmd: Cmd<(CoverJob, Revision), LibraryEvent>) -> LibraryLoopCmd {
    let (jobs, events) = cmd.into_parts();
    let library_loop_cmd: LibraryLoopCmd = jobs
        .into_iter()
        .map(|(cover_job, revision)| {
            LoopEffect::Run(LibraryJob::DecodeCover {
                cover_job,
                revision,
            })
        })
        .collect();
    drained(library_loop_cmd, events)
}
