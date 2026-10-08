use crate::{
    domain::{
        bounded::Bounded,
        chord::{Chord, ChordPrefix, KeyPattern},
        direction::Direction,
        key::KeyCode,
        keymap::{Action, KeyContext},
        overlay::OverlayName,
        transport::{SEEK_LARGE, SEEK_MEDIUM, SEEK_SMALL},
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
        chord::{KeyBinding, bare, ctrl, digit_char, digits, key, shifted},
        overlays,
    },
};

fn row(key_context: KeyContext) -> impl Fn(Action, Chord, Message) -> KeyBinding {
    move |action, chord, message| KeyBinding {
        pattern: KeyPattern::Chord(chord),
        message,
        action: Some(action),
        key_context,
    }
}

fn digit_seek_rows() -> impl Iterator<Item = KeyBinding> {
    digits().filter_map(|digit| {
        let tenths = SeekTenths::clamped(digit);
        Some(row(KeyContext::Global)(
            Action::SeekTenth(digit),
            key(digit_char(digit)?),
            Message::Playback(PlaybackRequest::SeekTenths(tenths)),
        ))
    })
}

#[rustfmt::skip]
fn global_rows() -> Vec<KeyBinding> {
    use crate::{domain::{keymap::{Action::{AbRepeat, AddServer, Help, History, JumpToTime, MusicDir, Next, PlayPause, Previous, Quit, Repeat, Search, SeekBack, SeekBackLong, SeekBackShort, SeekForward, SeekForwardLong, SeekForwardShort, Servers, Settings, Shuffle, SleepTimer, SpeedDown, SpeedUp, VolumeDown, VolumeUp}, KeyContext::{Global}}}};
    use PlaybackRequest as P;
    let row = row(Global);
    vec![
        row(PlayPause, key(' '), Message::Playback(P::Toggle)),
        row(Next, key('n'), Message::Playback(P::Next)),
        row(Previous, key('p'), Message::Playback(P::Previous)),
        row(SeekBack, key('h'), Message::Playback(P::SeekBy { direction: Direction::Previous, by: SEEK_MEDIUM })),
        row(SeekForward, key('l'), Message::Playback(P::SeekBy { direction: Direction::Next, by: SEEK_MEDIUM })),
        row(SeekBackShort, bare(KeyCode::Left), Message::Playback(P::SeekBy { direction: Direction::Previous, by: SEEK_SMALL })),
        row(SeekForwardShort, bare(KeyCode::Right), Message::Playback(P::SeekBy { direction: Direction::Next, by: SEEK_SMALL })),
        row(SeekBackLong, shifted(KeyCode::Left), Message::Playback(P::SeekBy { direction: Direction::Previous, by: SEEK_LARGE })),
        row(SeekForwardLong, shifted(KeyCode::Right), Message::Playback(P::SeekBy { direction: Direction::Next, by: SEEK_LARGE })),
        row(VolumeUp, key('+'), Message::Playback(P::StepVolume(Direction::Next))),
        row(VolumeUp, key('='), Message::Playback(P::StepVolume(Direction::Next))),
        row(VolumeDown, key('-'), Message::Playback(P::StepVolume(Direction::Previous))),
        row(VolumeDown, key('_'), Message::Playback(P::StepVolume(Direction::Previous))),
        row(Shuffle, key('s'), Message::Playback(P::ToggleShuffle)),
        row(Repeat, key('r'), Message::Playback(P::CycleRepeat)),
        row(SleepTimer, key('z'), Message::Playback(P::CycleSleep)),
        row(AbRepeat, key('b'), Message::Playback(P::AbMark)),
        row(SpeedDown, key('['), Message::Playback(P::StepSpeed(Direction::Previous))),
        row(SpeedUp, key(']'), Message::Playback(P::StepSpeed(Direction::Next))),
        row(JumpToTime, ctrl('j'), Message::Overlay(OverlayRequest::Open(OverlayName::JumpToTime))),
        row(Search, key('/'), Message::Overlay(OverlayRequest::Open(OverlayName::Search))),
        row(History, key('H'), Message::Overlay(OverlayRequest::Open(OverlayName::History))),
        row(Settings, key(','), Message::Overlay(OverlayRequest::Open(OverlayName::Settings))),
        row(MusicDir, key('L'), Message::Overlay(OverlayRequest::Open(OverlayName::MusicDir))),
        row(AddServer, key('u'), Message::Overlay(OverlayRequest::Open(OverlayName::AddServer))),
        row(Servers, key('c'), Message::Overlay(OverlayRequest::Open(OverlayName::Servers))),
        row(Help, key('?'), Message::Overlay(OverlayRequest::Open(OverlayName::Help))),
        row(Help, ctrl('k'), Message::Overlay(OverlayRequest::Open(OverlayName::Help))),
        row(Quit, key('q'), Message::Quit),
    ]
}

#[rustfmt::skip]
fn playlist_rows() -> Vec<KeyBinding> {
    use crate::{domain::{keymap::{Action::{Bottom, CycleSort, Delete, Dequeue, Down, Enqueue, Favorite, FullScan, LevelUp, NextCatalog, PageDown, PageUp, PlayNext, PlaySelected, PreviousCatalog, QueueMoveDown, QueueMoveUp, SavePlaylist, Top, TrackDetails, Up}, KeyContext::{Playlist}}}};
    use BrowseRequest as B;
    use QueueRequest as Q;
    let row = row(Playlist);
    vec![
        row(Down, key('j'), Message::Browse(B::CursorBy { rows: 1 })),
        row(Down, bare(KeyCode::Down), Message::Browse(B::CursorBy { rows: 1 })),
        row(Up, key('k'), Message::Browse(B::CursorBy { rows: -1 })),
        row(Up, bare(KeyCode::Up), Message::Browse(B::CursorBy { rows: -1 })),
        row(Top, Chord::Sequence { prefix: ChordPrefix::G, key: ChordPrefix::G.key() }, Message::Browse(B::SelectFirst)),
        row(Top, bare(KeyCode::Home), Message::Browse(B::SelectFirst)),
        row(Bottom, key('G'), Message::Browse(B::SelectLast)),
        row(Bottom, bare(KeyCode::End), Message::Browse(B::SelectLast)),
        row(PageDown, bare(KeyCode::PageDown), Message::Browse(B::PageBy(Direction::Next))),
        row(PageDown, ctrl('d'), Message::Browse(B::PageBy(Direction::Next))),
        row(PageUp, bare(KeyCode::PageUp), Message::Browse(B::PageBy(Direction::Previous))),
        row(PageUp, ctrl('u'), Message::Browse(B::PageBy(Direction::Previous))),
        row(PlaySelected, bare(KeyCode::Enter), Message::Browse(B::PlaySelected)),
        row(NextCatalog, bare(KeyCode::Tab), Message::Browse(B::StepCatalog(Direction::Next))),
        row(PreviousCatalog, shifted(KeyCode::Tab), Message::Browse(B::StepCatalog(Direction::Previous))),
        row(LevelUp, bare(KeyCode::Backspace), Message::Browse(B::LevelUp)),
        row(Enqueue, key('a'), Message::Queue(Q::Toggle)),
        row(PlayNext, key('A'), Message::Queue(Q::PlayNext)),
        row(Dequeue, key('x'), Message::Queue(Q::Dequeue)),
        row(QueueMoveUp, shifted(KeyCode::Up), Message::Queue(Q::Move(Direction::Previous))),
        row(QueueMoveDown, shifted(KeyCode::Down), Message::Queue(Q::Move(Direction::Next))),
        row(CycleSort, key('o'), Message::Browse(B::CycleSort)),
        row(Favorite, key('f'), Message::Browse(B::ToggleFavorite)),
        row(Delete, key('d'), Message::Overlay(OverlayRequest::Open(OverlayName::ConfirmTrash))),
        row(SavePlaylist, key('S'), Message::Overlay(OverlayRequest::Open(OverlayName::SavePlaylist))),
        row(FullScan, key('R'), Message::Browse(B::Rescan)),
        row(TrackDetails, key('i'), Message::Overlay(OverlayRequest::Open(OverlayName::TrackDetails))),
    ]
}

pub(crate) fn defaults() -> Vec<KeyBinding> {
    global_rows()
        .into_iter()
        .chain(playlist_rows())
        .chain(digit_seek_rows())
        .chain(overlays::rows())
        .collect()
}
