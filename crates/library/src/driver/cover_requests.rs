use std::path::PathBuf;

use kernel::{
    cmd::{Cmd, CoverJob},
    domain::{config::Diagnostic, revision::Revision},
    message::{LibraryError, LibraryEvent},
    update::machine::{LoopEffect, Machine, Unhandled},
};

use crate::{
    cover::{CoverDecoded, CoverError, CoverLookup, decoding::CoverDecodingMessage},
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
        if self.decoding.busy().is_some()
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
        let settled = match (self.decoding.is_current(revision), &decoded) {
            (true, _) => lift_decoding(
                self.decoding
                    .transition(CoverDecodingMessage::Decoded(revision))?,
            ),
            (false, Err(_)) => return Err(Unhandled),
            (false, Ok(_)) => Cmd::none(),
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{
        cmd::{DiskCmd, LibraryCmd},
        domain::{geometry::Pixels, revision::Revision},
        update::machine::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::{
        cover::{CoverDecoded, CoverLookup},
        driver::tests::{
            LibraryRow,
            cmds,
            cover,
            cover_sized,
            cover_state,
            decoded,
            describe,
            driver,
            failed,
            prefetch,
        },
        message::LibraryMessage,
    };

    #[rstest]
    #[case::a_cover_while_idle_decodes(LibraryRow {
        library_messages: Vec::new(),
        message: cover("/music/one.flac"),
        cmd: "decode /music/one.flac @ 64",
    })]
    #[case::a_decoded_cover_is_published(LibraryRow {
        library_messages: vec![cover("/music/one.flac")],
        message: decoded("/music/one.flac", 1),
        cmd: "publish /music/one.flac @ 64 missing",
    })]
    #[case::a_failed_cover_is_published_missing_and_told(LibraryRow {
        library_messages: vec![cover("/music/one.flac")],
        message: failed("/music/one.flac", 1),
        cmd: "publish /music/one.flac @ 64 missing; tell error",
    })]
    #[case::a_failed_cover_is_decoded_again_when_wanted_again(LibraryRow {
        library_messages: vec![
            cover("/music/one.flac"),
            failed("/music/one.flac", 1),
            cover("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        message: cover("/music/one.flac"),
        cmd: "decode /music/one.flac @ 64",
    })]
    #[case::a_remembered_cover_is_published_without_a_decode(LibraryRow {
        library_messages: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            cover("/music/two.flac"),
        ],
        message: cover("/music/one.flac"),
        cmd: "publish /music/one.flac @ 64 missing",
    })]
    #[case::a_batch_keeps_its_handled_commands_when_a_prefetch_is_refused(LibraryRow {
        library_messages: Vec::new(),
        message: cmds(vec![
            LibraryCmd::PrefetchCover(kernel::cmd::CoverJob {
                path: PathBuf::from("/music/two.flac"),
                side: Pixels(64),
            }),
            LibraryCmd::Disk(DiskCmd::LoadFavorites),
        ]),
        cmd: "execute load_favorites",
    })]
    #[case::a_cover_wanted_while_its_prefetch_decodes_waits_for_it(LibraryRow {
        library_messages: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
        ],
        message: cover("/music/two.flac"),
        cmd: "nothing",
    })]
    #[case::a_prefetch_uses_the_remembered_side(LibraryRow {
        library_messages: vec![
            cover_sized("/music/one.flac", 96),
            LibraryMessage::CoverDecoded {
                revision: Revision::default().next(),
                decoded: Ok(CoverDecoded {
                    path: PathBuf::from("/music/one.flac"),
                    side: Pixels(96),
                    cover_lookup: CoverLookup::Missing,
                }),
            },
        ],
        message: prefetch("/music/two.flac"),
        cmd: "decode /music/two.flac @ 96",
    })]
    #[case::a_cover_at_a_new_side_while_it_decodes_restarts(LibraryRow {
        library_messages: vec![cover_sized("/music/one.flac", 64)],
        message: cover_sized("/music/one.flac", 96),
        cmd: "decode /music/one.flac @ 96",
    })]
    #[case::a_decoded_prefetch_is_remembered_not_published(LibraryRow {
        library_messages: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
        ],
        message: decoded("/music/two.flac", 2),
        cmd: "nothing",
    })]
    #[case::a_cover_wanted_while_its_prefetch_decodes_is_published(LibraryRow {
        library_messages: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            cover("/music/two.flac"),
        ],
        message: decoded("/music/two.flac", 2),
        cmd: "publish /music/two.flac @ 64 missing",
    })]
    #[case::a_prefetched_cover_is_published_from_memory(LibraryRow {
        library_messages: vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        message: cover("/music/two.flac"),
        cmd: "publish /music/two.flac @ 64 missing",
    })]
    #[case::a_stale_decoded_cover_is_remembered_not_published(LibraryRow {
        library_messages: vec![cover("/music/one.flac"), cover("/music/two.flac")],
        message: decoded("/music/one.flac", 1),
        cmd: "nothing",
    })]
    #[case::a_stale_decoded_cover_is_published_from_memory(LibraryRow {
        library_messages: vec![
            cover("/music/one.flac"),
            cover("/music/two.flac"),
            decoded("/music/one.flac", 1),
        ],
        message: cover("/music/one.flac"),
        cmd: "publish /music/one.flac @ 64 missing",
    })]
    fn a_row_steps_the_driver_and_names_its_cmd(#[case] row: LibraryRow) {
        let mut driver = driver();
        for message in row.library_messages {
            assert!(driver.transition(message).is_ok());
        }

        let cmd = driver.transition(row.message).unwrap();

        assert_eq!(describe(cmd), row.cmd);
    }

    #[rstest]
    #[case::a_prefetch_before_any_cover_is_refused(
        Vec::new(),
        prefetch("/music/two.flac")
    )]
    #[case::a_prefetch_while_a_cover_decodes_is_refused(
        vec![cover("/music/one.flac")],
        prefetch("/music/two.flac")
    )]
    #[case::a_prefetch_of_a_remembered_cover_is_refused(
        vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        prefetch("/music/two.flac")
    )]
    #[case::a_prefetch_of_the_oldest_remembered_cover_is_refused(
        vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        prefetch("/music/one.flac")
    )]
    #[case::a_stale_failed_cover(
        vec![cover("/music/one.flac"), cover("/music/two.flac")],
        failed("/music/one.flac", 1)
    )]
    #[case::the_same_cover_while_it_decodes(
        vec![cover("/music/one.flac")],
        cover("/music/one.flac")
    )]
    #[case::a_duplicate_cover_after_its_decode(
        vec![cover("/music/one.flac"), decoded("/music/one.flac", 1)],
        cover("/music/one.flac")
    )]
    #[case::a_duplicate_cover_after_a_prefetch(
        vec![
            cover("/music/one.flac"),
            decoded("/music/one.flac", 1),
            prefetch("/music/two.flac"),
            decoded("/music/two.flac", 2),
        ],
        cover("/music/one.flac")
    )]
    #[case::a_batch_whose_commands_are_all_rejected(
        vec![cover("/music/one.flac")],
        cmds(vec![
            LibraryCmd::DecodeCover(kernel::cmd::CoverJob {
                path: PathBuf::from("/music/one.flac"),
                side: Pixels(64),
            }),
            LibraryCmd::DecodeCover(kernel::cmd::CoverJob {
                path: PathBuf::from("/music/one.flac"),
                side: Pixels(64),
            }),
        ])
    )]
    fn a_refused_row_is_unhandled(
        #[case] library_messages: Vec<LibraryMessage>,
        #[case] message: LibraryMessage,
    ) {
        let mut driver = driver();
        for step in library_messages {
            assert!(driver.transition(step).is_ok());
        }

        let before = cover_state(&driver);

        assert!(matches!(driver.transition(message), Err(Unhandled)));
        assert_eq!(cover_state(&driver), before);
    }
}
