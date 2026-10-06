use kernel::{
    cmd::{Cmd, CoverJob},
    domain::revision::Revision,
    message::LibraryEvent,
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum CoverDecoding {
    #[default]
    Idle,
    Busy {
        job: CoverJob,
        revision: Revision,
    },
}

#[derive(Debug)]
pub(crate) enum CoverDecodingMessage {
    Request { job: CoverJob, revision: Revision },
    Decoded(Revision),
}

impl Machine for CoverDecoding {
    type Message = CoverDecodingMessage;
    type Effect = Cmd<(CoverJob, Revision), LibraryEvent>;

    fn transition(
        &mut self,
        message: CoverDecodingMessage,
    ) -> Result<Cmd<(CoverJob, Revision), LibraryEvent>, Unhandled> {
        match (&*self, message) {
            (CoverDecoding::Idle, CoverDecodingMessage::Request { job, revision }) => {
                Ok(self.start(job, revision))
            }
            (
                CoverDecoding::Busy { job: busy, .. },
                CoverDecodingMessage::Request { job, revision },
            ) if *busy != job => Ok(self.start(job, revision)),
            (
                CoverDecoding::Busy { revision, .. },
                CoverDecodingMessage::Decoded(answered),
            ) if *revision == answered => {
                *self = CoverDecoding::Idle;
                Ok(Cmd::none())
            }
            (CoverDecoding::Idle, CoverDecodingMessage::Decoded(_))
            | (
                CoverDecoding::Busy { .. },
                CoverDecodingMessage::Request { .. } | CoverDecodingMessage::Decoded(_),
            ) => Err(Unhandled),
        }
    }
}

impl CoverDecoding {
    pub(crate) fn busy(&self) -> Option<&CoverJob> {
        match self {
            CoverDecoding::Idle => None,
            CoverDecoding::Busy { job, .. } => Some(job),
        }
    }

    fn start(
        &mut self,
        job: CoverJob,
        revision: Revision,
    ) -> Cmd<(CoverJob, Revision), LibraryEvent> {
        *self = CoverDecoding::Busy {
            job: job.clone(),
            revision,
        };
        Cmd::effect((job, revision))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use kernel::{
        cmd::{Cmd, CoverJob},
        domain::{geometry::Pixels, revision::Revision},
        message::LibraryEvent,
        update::machine::{Machine, Unhandled},
    };
    use rstest::rstest;

    use crate::cover::decoding::{CoverDecoding, CoverDecodingMessage};

    fn job(path: &str, side: u32) -> CoverJob {
        CoverJob {
            path: PathBuf::from(path),
            side: Pixels(side),
        }
    }

    fn request(path: &str) -> CoverDecodingMessage {
        CoverDecodingMessage::Request {
            job: job(path, 64),
            revision: Revision::default(),
        }
    }

    fn idle() -> CoverDecoding {
        CoverDecoding::Idle
    }

    fn busy(path: &str) -> CoverDecoding {
        CoverDecoding::Busy {
            job: job(path, 64),
            revision: Revision::default(),
        }
    }

    fn describe(cmd: &Cmd<(CoverJob, Revision), LibraryEvent>) -> String {
        cmd.effects().next().map_or_else(
            || "nothing".to_string(),
            |(job, _)| format!("decode {} @ {}", job.path.display(), job.side.0),
        )
    }

    struct CoverDecodingRow {
        start: CoverDecoding,
        message: CoverDecodingMessage,
        next: CoverDecoding,
        effect: &'static str,
    }

    #[rstest]
    #[case::idle_starts_a_decode(CoverDecodingRow {
        start: idle(),
        message: request("/music/cover.jpg"),
        next: busy("/music/cover.jpg"),
        effect: "decode /music/cover.jpg @ 64",
    })]
    #[case::busy_switches_to_another_path(CoverDecodingRow {
        start: busy("/music/one.jpg"),
        message: request("/music/two.jpg"),
        next: busy("/music/two.jpg"),
        effect: "decode /music/two.jpg @ 64",
    })]
    #[case::busy_settles_on_its_own_answer(CoverDecodingRow {
        start: busy("/music/cover.jpg"),
        message: CoverDecodingMessage::Decoded(Revision::default()),
        next: idle(),
        effect: "nothing",
    })]
    fn a_row_moves_the_decode_and_names_its_effect(#[case] row: CoverDecodingRow) {
        let mut state = row.start;
        let effect = state.transition(row.message).unwrap();
        assert_eq!(state, row.next);
        assert_eq!(describe(&effect), row.effect);
    }

    #[rstest]
    #[case::idle_refuses_an_answer(
        idle(),
        CoverDecodingMessage::Decoded(Revision::default())
    )]
    #[case::busy_refuses_the_same_request_again(
        busy("/music/cover.jpg"),
        request("/music/cover.jpg")
    )]
    #[case::busy_refuses_a_stale_answer(
        busy("/music/cover.jpg"),
        CoverDecodingMessage::Decoded(Revision::default().next())
    )]
    fn a_refused_row_hands_the_state_back(
        #[case] start: CoverDecoding,
        #[case] message: CoverDecodingMessage,
    ) {
        let expected = start.clone();
        let mut state = start;
        let refused = state.transition(message).err().unwrap();
        assert_eq!(state, expected);
        assert_eq!(refused, Unhandled);
    }
}
