use crate::{
    cmd::{Cmd, ConfigCmd, ConfigPatch, Cue, Effect},
    domain::{
        CursorOver,
        JumpDigits,
        MusicDirError,
        Overlay,
        PlaylistIndex,
        SearchQuery,
        TextEntry,
        parse_timecode,
        playlist::PlaylistFileName,
    },
    message::{BrowseRequest, PlaybackRequest, PlaylistRequest},
    update::{
        machine::{Machine, Rejected},
        overlay::{
            FollowUp,
            InnerMessage,
            OverlayEffect,
            OverlayError,
            OverlayMessage,
            text,
        },
    },
};

struct Lift<Inner: Machine> {
    wrap: fn(Inner) -> Overlay,
    reject: fn(Inner::Error) -> OverlayError,
}

fn lift<Inner: Machine>(
    mut inner: Inner,
    message: Inner::Message,
    lens: &Lift<Inner>,
) -> Transition
where
    Inner::Effect: Into<OverlayEffect>,
{
    let outcome = inner.update(message);
    let state = Some((lens.wrap)(inner));
    match outcome {
        Ok(effect) => Ok((state, effect.into())),
        Err(reason) => refuse(state, (lens.reject)(reason)),
    }
}

type Transition = Result<(Option<Overlay>, OverlayEffect), Rejected<Option<Overlay>>>;

impl Machine for Option<Overlay> {
    type Message = OverlayMessage;
    type Error = OverlayError;
    type Effect = OverlayEffect;

    fn transition(self, message: OverlayMessage) -> Transition {
        match (self, message) {
            (previous, OverlayMessage::Open(opened)) => open(previous.as_ref(), opened),
            (
                None,
                OverlayMessage::Close
                | OverlayMessage::Confirm
                | OverlayMessage::Inner(_),
            ) => refuse(None, OverlayError::WhileClosed),
            (Some(open), OverlayMessage::Close) => {
                cued_close(Ok((None, closed_playback(&open))))
            }
            (Some(open), OverlayMessage::Confirm) => cued_close(confirm(open)),
            (Some(open), OverlayMessage::Inner(inner)) => inner_transition(open, inner),
        }
    }
}

fn refuse(state: Option<Overlay>, reason: OverlayError) -> Transition {
    Err(Rejected { state, reason })
}

fn cued_close(transition: Transition) -> Transition {
    match transition {
        Ok((None, effect)) => Ok((
            None,
            OverlayEffect {
                cmd: effect.cmd.then(Cue::OverlayClosed.into()),
                follow_up: effect.follow_up,
            },
        )),
        Ok((still_open @ Some(_), effect)) => Ok((still_open, effect)),
        Err(rejected) => Err(rejected),
    }
}

fn open(previous: Option<&Overlay>, opened: Overlay) -> Transition {
    let follow_up = opened_playback(previous, &opened);
    Ok((
        Some(opened),
        OverlayEffect {
            cmd: Cue::OverlayOpened.into(),
            follow_up,
        },
    ))
}

fn hold() -> FollowUp {
    FollowUp::Playback(PlaybackRequest::Hold)
}

fn release() -> FollowUp {
    FollowUp::Playback(PlaybackRequest::Release)
}

fn opened_playback(previous: Option<&Overlay>, opened: &Overlay) -> Option<FollowUp> {
    match (previous, opened) {
        (_, Overlay::Settings { .. }) => Some(hold()),
        (Some(Overlay::Settings { .. }), _) => Some(release()),
        (_, _) => None,
    }
}

fn closed_playback(open: &Overlay) -> OverlayEffect {
    match open {
        Overlay::Settings { .. } => OverlayEffect::from(release()),
        Overlay::Help
        | Overlay::Search(_)
        | Overlay::SavePlaylist { .. }
        | Overlay::History(_)
        | Overlay::ConfirmDelete(_)
        | Overlay::TrackDetails(_)
        | Overlay::JumpToTime(_)
        | Overlay::MusicDir { .. } => OverlayEffect::default(),
    }
}

fn confirm(open: Overlay) -> Transition {
    match open {
        Overlay::Search(search) => confirm_search(search),
        Overlay::SavePlaylist { typed, .. } => confirm_save_playlist(typed),
        Overlay::ConfirmDelete(candidate) => Ok((
            None,
            OverlayEffect::from(FollowUp::Browse(BrowseRequest::Trash(
                candidate.track,
            ))),
        )),
        Overlay::JumpToTime(digits) => confirm_jump(digits),
        Overlay::MusicDir { typed, .. } => confirm_music_dir(typed),
        Overlay::Settings { .. } => Ok((None, OverlayEffect::from(release()))),
        Overlay::Help | Overlay::TrackDetails(_) | Overlay::History(_) => {
            refuse(Some(open), OverlayError::NoConfirm)
        }
    }
}

fn confirm_search(search: CursorOver<SearchQuery>) -> Transition {
    let Some(index) = search.content.matches.get(search.selected()).copied() else {
        return refuse(Some(Overlay::Search(search)), OverlayError::NothingSelected);
    };
    Ok((
        None,
        OverlayEffect::from(FollowUp::Loaded(PlaylistRequest::JumpTo(
            PlaylistIndex::new(index),
        ))),
    ))
}

fn confirm_jump(mut digits: JumpDigits) -> Transition {
    match parse_timecode(&digits.input) {
        Ok(target) => Ok((
            None,
            OverlayEffect::from(FollowUp::Playback(PlaybackRequest::SeekTo(target))),
        )),
        Err(error) => {
            digits.error = Some(error);
            Ok((Some(Overlay::JumpToTime(digits)), OverlayEffect::default()))
        }
    }
}

fn confirm_save_playlist(typed: TextEntry) -> Transition {
    match PlaylistFileName::new(&typed.input) {
        Ok(name) => Ok((
            None,
            OverlayEffect::from(FollowUp::Browse(BrowseRequest::SavePlaylist(name))),
        )),
        Err(reason) => {
            let open = Overlay::SavePlaylist {
                typed,
                error: Some(reason),
            };
            Ok((Some(open), OverlayEffect::default()))
        }
    }
}

fn confirm_music_dir(typed: TextEntry) -> Transition {
    if typed.input.trim().is_empty() {
        let open = Overlay::MusicDir {
            typed,
            error: Some(MusicDirError::Empty),
        };
        return Ok((Some(open), OverlayEffect::default()));
    }
    let save = Effect::Config(ConfigCmd::Save(
        ConfigPatch::builder().music_dir(typed.input).build(),
    ));
    Ok((None, OverlayEffect::from(Cmd::from(save))))
}

fn inner_transition(open: Overlay, inner: InnerMessage) -> Transition {
    match (open, inner) {
        (Overlay::Search(search), InnerMessage::Search(message)) => lift(
            search,
            message,
            &Lift {
                wrap: Overlay::Search,
                reject: OverlayError::Search,
            },
        ),
        (Overlay::Settings { selected }, InnerMessage::Settings(message)) => Ok(
            crate::update::overlay::settings::transition(selected, message),
        ),
        (Overlay::SavePlaylist { typed, .. }, InnerMessage::Text(message)) => Ok((
            Some(Overlay::SavePlaylist {
                typed: text::retyped(typed, message),
                error: None,
            }),
            OverlayEffect::default(),
        )),
        (Overlay::MusicDir { typed, .. }, InnerMessage::Text(message)) => Ok((
            Some(Overlay::MusicDir {
                typed: text::retyped(typed, message),
                error: None,
            }),
            OverlayEffect::default(),
        )),
        (Overlay::JumpToTime(digits), InnerMessage::Jump(message)) => lift(
            digits,
            message,
            &Lift {
                wrap: Overlay::JumpToTime,
                reject: OverlayError::Jump,
            },
        ),
        (Overlay::History(cursor), InnerMessage::History(message)) => lift(
            cursor,
            message,
            &Lift {
                wrap: Overlay::History,
                reject: OverlayError::History,
            },
        ),
        (
            open,
            InnerMessage::Search(_)
            | InnerMessage::Settings(_)
            | InnerMessage::Text(_)
            | InnerMessage::Jump(_)
            | InnerMessage::History(_),
        ) => refuse(Some(open), OverlayError::WrongOverlay),
    }
}
