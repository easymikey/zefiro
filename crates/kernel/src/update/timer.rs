use crate::{
    cmd::Cmd,
    domain::{Freshness, Moment, Revision, Workspace},
    update::{
        machine::Unhandled,
        player::{self, PlaybackParts, PlayerMessage},
    },
};

pub(crate) fn toast_expired(
    workspace: &mut Workspace,
    revision: Revision,
    reply: Freshness,
) -> Cmd {
    match reply {
        Freshness::Awaited => workspace.expire(revision),
        Freshness::Stale => Cmd::none(),
    }
}

pub(crate) fn sleep_fired(
    playback: &mut PlaybackParts<'_>,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    match (
        revision.freshness(playback.revisions.sleep),
        playback.transport.sleep,
    ) {
        (Freshness::Awaited, Some(_)) => {
            let paused =
                player::update_player(playback, PlayerMessage::SleepFired(now), now)?;
            playback.transport.sleep = None;
            Ok(paused)
        }
        (Freshness::Awaited, None) | (Freshness::Stale, Some(_) | None) => {
            Ok(Cmd::none())
        }
    }
}
