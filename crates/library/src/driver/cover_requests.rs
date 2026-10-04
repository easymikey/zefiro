use std::path::PathBuf;

use kernel::{
    cmd::{Cmd, CoverJob},
    domain::{config::Diagnostic, revision::Revision},
    message::{LibraryError, LibraryEvent},
    update::machine::{Machine, Unhandled},
};

use crate::{
    cover::{
        CoverArt,
        CoverDecoded,
        CoverError,
        decoding::{CoverDecoding, CoverDecodingMessage},
    },
    driver::{LibraryDriver, LibraryEffect},
    job::LibraryJob,
    message::LibraryMessage,
};

impl<P> LibraryDriver<P> {
    pub(crate) fn ask(
        &mut self,
        job: CoverJob,
    ) -> Result<Cmd<LibraryEffect, LibraryEvent>, Unhandled> {
        if self.asked.as_ref() == Some(&job) {
            return Err(Unhandled);
        }
        let cmd = self.covers.answer(&job).map_or_else(
            || {
                self.decode(job.clone())
                    .unwrap_or_else(|Unhandled| Cmd::none())
            },
            |decoded| Cmd::effect(LibraryEffect::PublishCover(decoded)),
        );
        self.asked = Some(job);
        Ok(cmd)
    }

    pub(crate) fn prefetch(
        &mut self,
        path: PathBuf,
    ) -> Cmd<LibraryEffect, LibraryEvent> {
        let Some(side) = self.asked.as_ref().map(|asked| asked.side) else {
            return Cmd::none();
        };
        let job = CoverJob { path, side };
        if self.decoding != CoverDecoding::Idle || self.covers.answer(&job).is_some() {
            return Cmd::none();
        }
        self.decode(job).unwrap_or_else(|Unhandled| Cmd::none())
    }

    fn decode(
        &mut self,
        job: CoverJob,
    ) -> Result<Cmd<LibraryEffect, LibraryEvent>, Unhandled> {
        let revision = self.cover_revision.next();
        let started = self
            .decoding
            .transition(CoverDecodingMessage::Request { job, revision })?;
        self.cover_revision = revision;
        Ok(self.lift_decoding(started))
    }

    pub(crate) fn decoded(
        &mut self,
        revision: Revision,
        decoded: Result<CoverDecoded, CoverError>,
    ) -> Result<Cmd<LibraryEffect, LibraryEvent>, Unhandled> {
        let settled = self
            .decoding
            .transition(CoverDecodingMessage::Decoded(revision))?;
        if let Ok(decoded) = &decoded {
            self.covers.remember(decoded);
        }
        let settled = self.lift_decoding(settled);
        let answer = match decoded {
            Ok(decoded) => self.published(decoded),
            Err(error) => self.failed(&error),
        };
        Ok(settled.then(answer))
    }

    fn published(&self, decoded: CoverDecoded) -> Cmd<LibraryEffect, LibraryEvent> {
        let asked = self
            .asked
            .as_ref()
            .is_some_and(|job| job.path == decoded.path && job.side == decoded.side);
        if asked {
            Cmd::effect(LibraryEffect::PublishCover(decoded))
        } else {
            Cmd::none()
        }
    }

    fn failed(&self, error: &CoverError) -> Cmd<LibraryEffect, LibraryEvent> {
        let Some(job) = self.asked.as_ref().filter(|job| job.path == error.path) else {
            return Cmd::none();
        };
        Cmd::effect(LibraryEffect::PublishCover(CoverDecoded {
            path: job.path.clone(),
            side: job.side,
            art: CoverArt::Missing,
        }))
        .then(Cmd::message(LibraryEvent::Error(LibraryError::Cover {
            path: error.path.clone(),
            diagnostic: Diagnostic::from_error(error),
        })))
    }

    fn lift_decoding(
        &mut self,
        decoding: Cmd<(CoverJob, Revision), LibraryMessage>,
    ) -> Cmd<LibraryEffect, LibraryEvent> {
        let (jobs, messages) = decoding.into_parts();
        let started: Cmd<LibraryEffect, LibraryEvent> = jobs
            .into_iter()
            .map(|(job, revision)| {
                LibraryEffect::Run(LibraryJob::Cover { job, revision })
            })
            .collect();
        messages.into_iter().fold(started, |cmd, told| {
            cmd.then(
                self.transition(told)
                    .unwrap_or_else(|Unhandled| Cmd::none()),
            )
        })
    }
}
