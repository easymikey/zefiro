use crate::{
    domain::{
        Action,
        Chord,
        ChordPrefix,
        Direction,
        Key,
        KeyCode,
        KeyContext,
        Modifiers,
        OverlayName,
        SeekSteps,
        digit_char,
        digits,
    },
    message::{
        BrowseRequest,
        Message,
        OverlayRequest,
        PlaybackRequest,
        QueueRequest,
        SeekTenths,
    },
    update::keymap::{
        chord::{ActionRow, KeyBinding},
        overlays,
    },
};

fn key(character: char) -> Chord {
    Chord::Key(Key::plain(KeyCode::Char(character)))
}
fn bare_key_chord(code: KeyCode) -> Chord {
    Chord::Key(Key::plain(code))
}
fn shift_special(code: KeyCode) -> Chord {
    Chord::Key(Key::new(code, Modifiers::SHIFT))
}
fn ctrl(character: char) -> Chord {
    Chord::Key(Key::ctrl(KeyCode::Char(character)))
}

type RowFields = (Action, Chord, Message, KeyContext);

fn row((action, chord, message, key_context): RowFields) -> KeyBinding {
    ActionRow {
        action,
        chord,
        message,
        key_context,
    }
    .into()
}

fn digit_seek_rows() -> impl Iterator<Item = KeyBinding> {
    digits().filter_map(|digit| {
        let tenths = SeekTenths::try_from(digit).ok()?;
        Some(row((
            Action::SeekTenth(digit),
            key(digit_char(digit)?),
            Message::Playback(PlaybackRequest::SeekFraction(tenths)),
            KeyContext::Global,
        )))
    })
}

#[rustfmt::skip]
fn global_rows() -> Vec<KeyBinding> {
    use crate::domain::KeyContext::Global;
    use crate::domain::Action::{
        AbRepeat, Help, History, JumpToTime, Next, PlayPause, Previous, Quit, Repeat, Search,
        SeekBack, SeekBackLong, SeekBackShort, SeekForward, SeekForwardLong, SeekForwardShort,
        Settings, Shuffle, SleepTimer, MusicDir, SpeedDown, SpeedUp, VolumeDown, VolumeUp,
    };
    use PlaybackRequest as P;
    let steps = SeekSteps::default();
    vec![
        row((PlayPause, key(' '), Message::Playback(P::Toggle), Global)),
        row((Next, key('n'), Message::Playback(P::Next), Global)),
        row((Previous, key('p'), Message::Playback(P::Previous), Global)),
        row((SeekBack, key('h'), Message::Playback(P::SeekBy { seconds: -steps.medium }), Global)),
        row((SeekForward, key('l'), Message::Playback(P::SeekBy { seconds: steps.medium }), Global)),
        row((SeekBackShort, bare_key_chord(KeyCode::Left), Message::Playback(P::SeekBy { seconds: -steps.small }), Global)),
        row((SeekForwardShort, bare_key_chord(KeyCode::Right), Message::Playback(P::SeekBy { seconds: steps.small }), Global)),
        row((SeekBackLong, shift_special(KeyCode::Left), Message::Playback(P::SeekBy { seconds: -steps.large }), Global)),
        row((SeekForwardLong, shift_special(KeyCode::Right), Message::Playback(P::SeekBy { seconds: steps.large }), Global)),
        row((VolumeUp, key('+'), Message::Playback(P::NudgeVolume { steps: 5 }), Global)),
        row((VolumeUp, key('='), Message::Playback(P::NudgeVolume { steps: 5 }), Global)),
        row((VolumeDown, key('-'), Message::Playback(P::NudgeVolume { steps: -5 }), Global)),
        row((VolumeDown, key('_'), Message::Playback(P::NudgeVolume { steps: -5 }), Global)),
        row((Shuffle, key('s'), Message::Playback(P::ToggleShuffle), Global)),
        row((Repeat, key('r'), Message::Playback(P::CycleRepeat), Global)),
        row((SleepTimer, key('z'), Message::Playback(P::CycleSleep), Global)),
        row((AbRepeat, key('b'), Message::Playback(P::AbMark), Global)),
        row((SpeedDown, key('['), Message::Playback(P::NudgeSpeed { steps: -1 }), Global)),
        row((SpeedUp, key(']'), Message::Playback(P::NudgeSpeed { steps: 1 }), Global)),
        row((JumpToTime, ctrl('j'), Message::Overlay(OverlayRequest::Open(OverlayName::JumpToTime)), Global)),
        row((Search, key('/'), Message::Overlay(OverlayRequest::Open(OverlayName::Search)), Global)),
        row((History, key('H'), Message::Overlay(OverlayRequest::Open(OverlayName::History)), Global)),
        row((Settings, key(','), Message::Overlay(OverlayRequest::Open(OverlayName::Settings)), Global)),
        row((MusicDir, key('L'), Message::Overlay(OverlayRequest::Open(OverlayName::MusicDir)), Global)),
        row((Help, key('?'), Message::Overlay(OverlayRequest::Open(OverlayName::Help)), Global)),
        row((Help, ctrl('k'), Message::Overlay(OverlayRequest::Open(OverlayName::Help)), Global)),
        row((Quit, key('q'), Message::Quit, Global)),
    ]
}

#[rustfmt::skip]
fn playlist_rows() -> Vec<KeyBinding> {
    use crate::domain::KeyContext::Playlist;
    use crate::domain::Action::{
        Bottom, CycleSort, Delete, Dequeue, Down, Enqueue, Favorite, PageDown, PageUp, PlayNext,
        PlaySelected, QueueMoveDown, QueueMoveUp, Rescan, SavePlaylist, Top, TrackDetails, Up,
    };
    use BrowseRequest as B;
    use QueueRequest as Q;
    vec![
        row((Down, key('j'), Message::Browse(B::CursorBy { rows: 1 }), Playlist)),
        row((Down, bare_key_chord(KeyCode::Down), Message::Browse(B::CursorBy { rows: 1 }), Playlist)),
        row((Up, key('k'), Message::Browse(B::CursorBy { rows: -1 }), Playlist)),
        row((Up, bare_key_chord(KeyCode::Up), Message::Browse(B::CursorBy { rows: -1 }), Playlist)),
        row((Top, Chord::Sequence { prefix: ChordPrefix::G, key: ChordPrefix::G.key() }, Message::Browse(B::Top), Playlist)),
        row((Top, bare_key_chord(KeyCode::Home), Message::Browse(B::Top), Playlist)),
        row((Bottom, key('G'), Message::Browse(B::Bottom), Playlist)),
        row((Bottom, bare_key_chord(KeyCode::End), Message::Browse(B::Bottom), Playlist)),
        row((PageDown, bare_key_chord(KeyCode::PageDown), Message::Browse(B::PageBy(Direction::Next)), Playlist)),
        row((PageDown, ctrl('d'), Message::Browse(B::PageBy(Direction::Next)), Playlist)),
        row((PageUp, bare_key_chord(KeyCode::PageUp), Message::Browse(B::PageBy(Direction::Previous)), Playlist)),
        row((PageUp, ctrl('u'), Message::Browse(B::PageBy(Direction::Previous)), Playlist)),
        row((PlaySelected, bare_key_chord(KeyCode::Enter), Message::Browse(B::PlaySelected), Playlist)),
        row((Enqueue, key('a'), Message::Queue(Q::Enqueue), Playlist)),
        row((PlayNext, key('A'), Message::Queue(Q::PlayNext), Playlist)),
        row((Dequeue, key('x'), Message::Queue(Q::Dequeue), Playlist)),
        row((QueueMoveUp, shift_special(KeyCode::Up), Message::Queue(Q::MoveInQueue(Direction::Previous)), Playlist)),
        row((QueueMoveDown, shift_special(KeyCode::Down), Message::Queue(Q::MoveInQueue(Direction::Next)), Playlist)),
        row((CycleSort, key('o'), Message::Browse(B::CycleSort), Playlist)),
        row((Favorite, key('f'), Message::Browse(B::ToggleFavorite), Playlist)),
        row((Delete, key('d'), Message::Overlay(OverlayRequest::Open(OverlayName::ConfirmDelete)), Playlist)),
        row((SavePlaylist, key('S'), Message::Overlay(OverlayRequest::Open(OverlayName::SavePlaylist)), Playlist)),
        row((Rescan, key('R'), Message::Browse(B::Rescan), Playlist)),
        row((TrackDetails, key('i'), Message::Overlay(OverlayRequest::Open(OverlayName::TrackDetails)), Playlist)),
    ]
}

pub(crate) fn defaults() -> Vec<KeyBinding> {
    let mut rows = global_rows();
    rows.extend(playlist_rows());
    rows.extend(digit_seek_rows());
    rows.extend(overlays::rows());
    rows
}
