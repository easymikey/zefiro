use std::path::PathBuf;

use kernel::{
    cmd::{Cmd, CoverJob},
    domain::{config::Diagnostic, revision::Revision},
    message::{LibraryError, LibraryEvent},
    update::machine::{LoopEffect, Machine, Unhandled},
};

use crate::{
    cover::{
        CoverArt,
        CoverDecoded,
        CoverError,
        decoding::{CoverDecoding, CoverDecodingMessage},
    },
    driver::{LibraryDriver, LibraryEffect, LibraryLoopCmd, drained},
    job::LibraryJob,
};

impl<P> LibraryDriver<P> {
    pub(crate) fn ask(&mut self, job: CoverJob) -> Result<LibraryLoopCmd, Unhandled> {
        if self.asked.as_ref() == Some(&job) {
            return Err(Unhandled);
        }
        let cmd = match self.covers.answer(&job).cloned() {
            Some(decoded) => {
                self.covers.remember(&decoded);
                Cmd::effect(LoopEffect::Execute(LibraryEffect::PublishCover(decoded)))
            }
            None if self.decoding.busy() == Some(&job) => Cmd::none(),
            None => self.decode(job.clone())?,
        };
        self.asked = Some(job);
        Ok(cmd)
    }

    pub(crate) fn prefetch(
        &mut self,
        path: PathBuf,
    ) -> Result<LibraryLoopCmd, Unhandled> {
        let side = self
            .asked
            .as_ref()
            .map(|asked| asked.side)
            .ok_or(Unhandled)?;
        let job = CoverJob { path, side };
        if self.decoding != CoverDecoding::Idle || self.covers.answer(&job).is_some() {
            return Err(Unhandled);
        }
        self.decode(job)
    }

    fn decode(&mut self, job: CoverJob) -> Result<LibraryLoopCmd, Unhandled> {
        let revision = self.cover_revision.next();
        let started = self
            .decoding
            .transition(CoverDecodingMessage::Request { job, revision })?;
        self.cover_revision = revision;
        Ok(lift_decoding(started))
    }

    pub(crate) fn decoded(
        &mut self,
        revision: Revision,
        decoded: Result<CoverDecoded, CoverError>,
    ) -> Result<LibraryLoopCmd, Unhandled> {
        let settled = self
            .decoding
            .transition(CoverDecodingMessage::Decoded(revision))?;
        if let Ok(decoded) = &decoded {
            self.covers.remember(decoded);
        }
        let settled = lift_decoding(settled);
        let answer = match decoded {
            Ok(decoded) => self.published(decoded),
            Err(error) => self.failed(&error),
        };
        Ok(settled.then(answer))
    }

    fn published(&self, decoded: CoverDecoded) -> LibraryLoopCmd {
        let asked = self
            .asked
            .as_ref()
            .is_some_and(|job| job.path == decoded.path && job.side == decoded.side);
        if asked {
            Cmd::effect(LoopEffect::Execute(LibraryEffect::PublishCover(decoded)))
        } else {
            Cmd::none()
        }
    }

    fn failed(&self, error: &CoverError) -> LibraryLoopCmd {
        let Some(job) = self.asked.as_ref().filter(|job| job.path == error.path) else {
            return Cmd::none();
        };
        Cmd::effect(LoopEffect::Execute(LibraryEffect::PublishCover(
            CoverDecoded {
                path: job.path.clone(),
                side: job.side,
                art: CoverArt::Missing,
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

fn lift_decoding(decoding: Cmd<(CoverJob, Revision), LibraryEvent>) -> LibraryLoopCmd {
    let (jobs, events) = decoding.into_parts();
    let started: LibraryLoopCmd = jobs
        .into_iter()
        .map(|(job, revision)| LoopEffect::Run(LibraryJob::Cover { job, revision }))
        .collect();
    drained(started, events)
}
