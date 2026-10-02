use crate::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Cue, Effect},
    domain::{
        CursorOver,
        JumpDigits,
        MusicDirError,
        Overlay,
        SearchQuery,
        TextEntry,
        ViewIndex,
        parse_timecode,
        playlist::{PlaylistFileName, PlaylistNameError},
    },
    message::{BrowseRequest, PlaybackRequest, PlaylistRequest},
    update::{
        machine::Machine,
        overlay::{
            FollowUp,
            InnerMessage,
            OverlayError,
            OverlayMessage,
            OverlayOutcome,
            settings,
            text,
        },
    },
};

impl Machine for Option<Overlay> {
    type Message = OverlayMessage;
    type Error = OverlayError;
    type Effect = OverlayOutcome;

    fn transition(
        &mut self,
        message: OverlayMessage,
    ) -> Result<OverlayOutcome, OverlayError> {
        match message {
            OverlayMessage::Open(opened) => {
                let follow_up = opened_playback(self.as_ref(), &opened);
                *self = Some(opened);
                Ok(OverlayOutcome {
                    cmd: Cue::OverlayOpened.into(),
                    follow_up,
                })
            }
            OverlayMessage::Close => {
                let open = self.take().ok_or(OverlayError::WhileClosed)?;
                Ok(cued_close(closed_playback(&open)))
            }
            OverlayMessage::Confirm => confirm(self),
            OverlayMessage::Inner(inner) => inner_transition(self, inner),
        }
    }
}

fn cued_close(effect: OverlayOutcome) -> OverlayOutcome {
    OverlayOutcome {
        cmd: effect.cmd.then(Cue::OverlayClosed.into()),
        follow_up: effect.follow_up,
    }
}

fn release() -> FollowUp {
    FollowUp::Playback(PlaybackRequest::Release)
}

fn opened_playback(previous: Option<&Overlay>, opened: &Overlay) -> Option<FollowUp> {
    match (previous, opened) {
        (_, Overlay::Settings { .. }) => {
            Some(FollowUp::Playback(PlaybackRequest::HoldForOverlay))
        }
        (Some(Overlay::Settings { .. }), _) => Some(release()),
        (_, _) => None,
    }
}

fn closed_playback(open: &Overlay) -> OverlayOutcome {
    match open {
        Overlay::Settings { .. } => OverlayOutcome::from(release()),
        Overlay::Help
        | Overlay::Search(_)
        | Overlay::SavePlaylist { .. }
        | Overlay::History(_)
        | Overlay::ConfirmDelete(_)
        | Overlay::TrackDetails(_)
        | Overlay::JumpToTime(_)
        | Overlay::MusicDir { .. } => OverlayOutcome::default(),
    }
}

fn confirm(state: &mut Option<Overlay>) -> Result<OverlayOutcome, OverlayError> {
    let open = state.as_mut().ok_or(OverlayError::WhileClosed)?;
    let Some(effect) = confirmed(open)? else {
        return Ok(OverlayOutcome::default());
    };
    *state = None;
    Ok(cued_close(effect))
}

fn confirmed(open: &mut Overlay) -> Result<Option<OverlayOutcome>, OverlayError> {
    match open {
        Overlay::Search(search) => confirm_search(search).map(Some),
        Overlay::SavePlaylist { typed, error } => {
            Ok(confirm_save_playlist(typed, error))
        }
        Overlay::ConfirmDelete(candidate) => Ok(Some(OverlayOutcome::from(
            FollowUp::Browse(BrowseRequest::Trash(candidate.index)),
        ))),
        Overlay::JumpToTime(digits) => Ok(confirm_jump(digits)),
        Overlay::MusicDir { typed, error } => Ok(confirm_music_dir(typed, error)),
        Overlay::Settings { .. } => Ok(Some(OverlayOutcome::from(release()))),
        Overlay::Help | Overlay::TrackDetails(_) | Overlay::History(_) => {
            Err(OverlayError::NoConfirm)
        }
    }
}

fn confirm_search(
    search: &CursorOver<SearchQuery>,
) -> Result<OverlayOutcome, OverlayError> {
    let index = search
        .content
        .matches
        .get(search.selected())
        .copied()
        .ok_or(OverlayError::NothingSelected)?;
    Ok(OverlayOutcome::from(FollowUp::Playlist(
        PlaylistRequest::JumpTo(ViewIndex::new(index)),
    )))
}

fn confirm_jump(digits: &mut JumpDigits) -> Option<OverlayOutcome> {
    match parse_timecode(&digits.input) {
        Ok(target) => Some(OverlayOutcome::from(FollowUp::Playback(
            PlaybackRequest::SeekTo(target),
        ))),
        Err(error) => {
            digits.error = Some(error);
            None
        }
    }
}

fn confirm_save_playlist(
    typed: &TextEntry,
    error: &mut Option<PlaylistNameError>,
) -> Option<OverlayOutcome> {
    match PlaylistFileName::new(&typed.input) {
        Ok(name) => Some(OverlayOutcome::from(FollowUp::Browse(
            BrowseRequest::SavePlaylist(name),
        ))),
        Err(reason) => {
            *error = Some(reason);
            None
        }
    }
}

fn confirm_music_dir(
    typed: &mut TextEntry,
    error: &mut Option<MusicDirError>,
) -> Option<OverlayOutcome> {
    if typed.input.trim().is_empty() {
        *error = Some(MusicDirError::Empty);
        return None;
    }
    let save = Effect::Config(ConfigCmd::Save(
        ConfigPatch::builder()
            .music_dir(std::path::PathBuf::from(std::mem::take(&mut typed.input)))
            .build(),
    ));
    Some(OverlayOutcome::from(Cmd::from(save)))
}

fn inner_transition(
    state: &mut Option<Overlay>,
    inner: InnerMessage,
) -> Result<OverlayOutcome, OverlayError> {
    let open = state.as_mut().ok_or(OverlayError::WhileClosed)?;
    match (open, inner) {
        (Overlay::Search(search), InnerMessage::Search(message)) => {
            search.transition(message).map_err(OverlayError::Search)
        }
        (Overlay::Settings { selected }, InnerMessage::Settings(message)) => {
            Ok(settings::transition(selected, message))
        }
        (Overlay::SavePlaylist { typed, error }, InnerMessage::Text(message)) => {
            text::retype(typed, message);
            *error = None;
            Ok(OverlayOutcome::default())
        }
        (Overlay::MusicDir { typed, error }, InnerMessage::Text(message)) => {
            text::retype(typed, message);
            *error = None;
            Ok(OverlayOutcome::default())
        }
        (Overlay::JumpToTime(digits), InnerMessage::Jump(message)) => {
            digits.transition(message).map_err(OverlayError::Jump)
        }
        (Overlay::History(cursor), InnerMessage::History(message)) => {
            cursor.transition(message).map_err(OverlayError::History)
        }
        (
            _,
            InnerMessage::Search(_)
            | InnerMessage::Settings(_)
            | InnerMessage::Text(_)
            | InnerMessage::Jump(_)
            | InnerMessage::History(_),
        ) => Err(OverlayError::WrongOverlay),
    }
}
