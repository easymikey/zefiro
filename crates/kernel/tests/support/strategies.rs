use std::{ops::Range, time::Duration};

use kernel::{
    AudioError,
    AudioEvent,
    Bounded,
    BrowseRequest,
    ConfigEvent,
    DecodeError,
    Direction,
    DriverMessage,
    Favorites,
    HistoryRequest,
    Key,
    KeyCode,
    KeyPress,
    LibraryError,
    LibraryEvent,
    MacosEvent,
    Message,
    Model,
    Moment,
    OverlayName,
    OverlayRequest,
    Percent,
    PlaybackRequest,
    PlaylistRequest,
    QueueRequest,
    SearchEdit,
    SearchRequest,
    SettingsRowRequest,
    TextRequest,
    Timer,
    Toast,
    domain::{
        ChordPrefix,
        Driver,
        DriverError,
        OutputDevice,
        PlaylistIndex,
        Revision,
        SettingRow,
        StreamError,
        ThemeName,
    },
    message::SeekTenths,
    playlist::{PlaylistFileName, RepeatMode},
    update::update,
};
use proptest::{
    prelude::{Just, Strategy, prop_oneof},
    sample::select,
};
use strum::IntoEnumIterator;

use crate::support::{model_with_dated_tracks, playing_model};

pub(crate) fn repeat_mode() -> impl Strategy<Value = RepeatMode> {
    prop_oneof![
        Just(RepeatMode::Off),
        Just(RepeatMode::All),
        Just(RepeatMode::One),
    ]
}

pub(crate) fn unmodified_key_code() -> impl Strategy<Value = KeyCode> {
    prop_oneof![
        proptest::char::range('!', '~').prop_map(KeyCode::Char),
        Just(KeyCode::Enter),
        Just(KeyCode::Esc),
        Just(KeyCode::Backspace),
        Just(KeyCode::Up),
        Just(KeyCode::Down),
        Just(KeyCode::Left),
        Just(KeyCode::Right),
        Just(KeyCode::Home),
        Just(KeyCode::End),
        Just(KeyCode::Tab),
        Just(KeyCode::PageUp),
        Just(KeyCode::PageDown),
    ]
}

pub(crate) fn durations(len: Range<usize>) -> impl Strategy<Value = Vec<Duration>> {
    proptest::collection::vec((0u64..7200).prop_map(Duration::from_secs), len)
}

fn direction() -> impl Strategy<Value = Direction> {
    prop_oneof![Just(Direction::Next), Just(Direction::Previous)]
}

fn key_press() -> impl Strategy<Value = KeyPress> {
    select(vec![
        KeyCode::Char('j'),
        KeyCode::Char('q'),
        KeyCode::Enter,
        KeyCode::Esc,
        KeyCode::PageDown,
    ])
    .prop_map(|code| {
        let key = Key::plain(code);
        KeyPress { key, typed: key }
    })
}

fn playlist_index() -> impl Strategy<Value = PlaylistIndex> {
    (0usize..6).prop_map(PlaylistIndex::new)
}

fn overlay_name() -> impl Strategy<Value = OverlayName> {
    select(OverlayName::iter().collect::<Vec<_>>())
}

fn overlay_input() -> impl Strategy<Value = OverlayRequest> {
    let typed = prop_oneof![Just('1'), Just(':'), Just('a'), Just(' ')];
    prop_oneof![
        typed.clone().prop_map(|character| {
            OverlayRequest::Search(SearchRequest::Edit(SearchEdit::Char(character)))
        }),
        select(vec![
            SearchEdit::Backspace,
            SearchEdit::DeleteWord,
            SearchEdit::Clear,
        ])
        .prop_map(|edit| OverlayRequest::Search(SearchRequest::Edit(edit))),
        direction().prop_map(|direction| OverlayRequest::Search(
            SearchRequest::Navigate(direction)
        )),
        Just(OverlayRequest::Search(SearchRequest::Enqueue)),
        direction().prop_map(|direction| {
            OverlayRequest::Settings(SettingsRowRequest::Navigate(direction))
        }),
        direction().prop_map(|direction| OverlayRequest::Settings(
            SettingsRowRequest::Adjust(direction)
        )),
        Just(OverlayRequest::Settings(SettingsRowRequest::Activate)),
        typed
            .clone()
            .prop_map(|character| OverlayRequest::Text(TextRequest::Char(character))),
        Just(OverlayRequest::Text(TextRequest::Backspace)),
        typed.prop_map(|character| OverlayRequest::Jump(TextRequest::Char(character))),
    ]
}

fn overlay() -> impl Strategy<Value = OverlayRequest> {
    prop_oneof![
        overlay_name().prop_map(OverlayRequest::Open),
        Just(OverlayRequest::Close),
        Just(OverlayRequest::Confirm),
        overlay_input(),
        Just(OverlayRequest::Jump(TextRequest::Backspace)),
        select(vec![
            HistoryRequest::Top,
            HistoryRequest::Bottom,
            HistoryRequest::Enqueue,
            HistoryRequest::Navigate(Direction::Previous),
            HistoryRequest::Navigate(Direction::Next),
        ])
        .prop_map(OverlayRequest::History),
    ]
}

fn playback() -> impl Strategy<Value = PlaybackRequest> {
    prop_oneof![
        select(vec![
            PlaybackRequest::Toggle,
            PlaybackRequest::Play,
            PlaybackRequest::Pause,
            PlaybackRequest::SeekForward,
            PlaybackRequest::SeekBack,
            PlaybackRequest::Hold,
            PlaybackRequest::Release,
            PlaybackRequest::Stop,
            PlaybackRequest::Next,
            PlaybackRequest::Previous,
            PlaybackRequest::ToggleShuffle,
            PlaybackRequest::CycleRepeat,
            PlaybackRequest::CycleSleep,
            PlaybackRequest::AbMark,
        ]),
        (-30i64..30).prop_map(|seconds| PlaybackRequest::SeekBy { seconds }),
        (-3i8..3).prop_map(|steps| PlaybackRequest::NudgeVolume { steps }),
        (-3i8..3).prop_map(|steps| PlaybackRequest::NudgeSpeed { steps }),
        (0u64..200).prop_map(|secs| PlaybackRequest::SeekTo(Duration::from_secs(secs))),
        (0u8..10).prop_map(|tenths| {
            PlaybackRequest::SeekFraction(SeekTenths::try_from(tenths).unwrap())
        }),
    ]
}

fn browse() -> impl Strategy<Value = BrowseRequest> {
    prop_oneof![
        select(vec![
            BrowseRequest::ChordPrefix(ChordPrefix::G),
            BrowseRequest::Top,
            BrowseRequest::Bottom,
            BrowseRequest::PlaySelected,
            BrowseRequest::CycleSort,
            BrowseRequest::FullScan,
            BrowseRequest::ToggleFavorite,
            BrowseRequest::SavePlaylist(PlaylistFileName::new("mix").unwrap()),
        ]),
        playlist_index().prop_map(BrowseRequest::Trash),
        playlist_index().prop_map(BrowseRequest::CursorTo),
        (-4i64..4).prop_map(|rows| BrowseRequest::CursorBy { rows }),
        direction().prop_map(BrowseRequest::PageBy),
    ]
}

fn queue() -> impl Strategy<Value = QueueRequest> {
    prop_oneof![
        select(vec![
            QueueRequest::Enqueue,
            QueueRequest::PlayNext,
            QueueRequest::Dequeue,
        ]),
        playlist_index().prop_map(QueueRequest::EnqueueTrack),
        direction().prop_map(QueueRequest::MoveInQueue),
    ]
}

fn audio() -> impl Strategy<Value = AudioEvent> {
    let failure = prop_oneof![
        Just(AudioError::Decode {
            path: "/tmp/track0.flac".into(),
            kind: DecodeError::Corrupt,
        }),
        Just(AudioError::OutputLost {
            kind: StreamError::DeviceGone,
        }),
    ];
    prop_oneof![
        (0u64..200).prop_map(|secs| AudioEvent::Playhead(Duration::from_secs(secs))),
        select(vec![
            AudioEvent::TrackChanged,
            AudioEvent::Ended,
            AudioEvent::DeviceFellBack(OutputDevice::SystemDefault),
            AudioEvent::DevicesListed(Vec::new()),
            AudioEvent::Loaded { total: None },
        ]),
        failure.prop_map(AudioEvent::Error),
    ]
}

fn loaded() -> impl Strategy<Value = PlaylistRequest> {
    prop_oneof![
        playlist_index().prop_map(PlaylistRequest::JumpTo),
        proptest::collection::vec(0usize..6, 0..6)
            .prop_map(PlaylistRequest::ShuffleRolled),
    ]
}

fn library() -> impl Strategy<Value = LibraryEvent> {
    select(vec![
        LibraryEvent::HistoryLoaded(Vec::new()),
        LibraryEvent::FavoritesLoaded(Favorites::default()),
        LibraryEvent::Error(LibraryError::NoUserDirs),
    ])
}

fn config() -> impl Strategy<Value = ConfigEvent> {
    select(vec![
        ConfigEvent::ThemesLoaded(vec![ThemeName::from_static("dusk")]),
        ConfigEvent::MusicDirReloaded("/tmp".into()),
        ConfigEvent::ThemeReloaded(ThemeName::from_static("dusk")),
    ])
}

fn driver() -> impl Strategy<Value = Message> {
    let driver = select(Driver::ALL.to_vec());
    let change = prop_oneof![
        Just(DriverMessage::Died(DriverError::Panicked(
            "boom".to_string()
        ))),
        Just(DriverMessage::Stopped),
        Just(DriverMessage::Full),
    ];
    (driver, change).prop_map(|(driver, change)| Message::Driver {
        driver,
        event: change,
    })
}

fn stamped() -> impl Strategy<Value = Revision> {
    (0u64..20).prop_map(|stamp| {
        (0..stamp).fold(Revision::default(), |revision, _| revision.next())
    })
}

fn event() -> impl Strategy<Value = Message> {
    prop_oneof![
        (
            select(vec![SettingRow::Crossfade, SettingRow::Replaygain]),
            direction()
        )
            .prop_map(|(row, direction)| Message::Adjust { row, direction }),
        Just(Message::Toast(Toast::info("hello".to_string()))),
        stamped().prop_map(|revision| Message::Elapsed(Timer::Toast(revision))),
        stamped().prop_map(|revision| Message::Elapsed(Timer::Sleep(revision))),
        (0usize..50).prop_map(|visible_rows| Message::Viewport { visible_rows }),
        key_press().prop_map(Message::Key),
    ]
}

fn system() -> impl Strategy<Value = MacosEvent> {
    prop_oneof![
        (0u8..=100).prop_map(|volume| MacosEvent::Volume(Percent::clamped(volume))),
        Just(MacosEvent::OutputRouteChanged),
        select(vec![
            PlaybackRequest::Play,
            PlaybackRequest::Pause,
            PlaybackRequest::Toggle,
            PlaybackRequest::Stop,
            PlaybackRequest::Next,
            PlaybackRequest::Previous,
            PlaybackRequest::SeekForward,
            PlaybackRequest::SeekBack,
        ])
        .prop_map(MacosEvent::MediaKey),
        (0u64..200).prop_map(|secs| MacosEvent::MediaKey(PlaybackRequest::SeekTo(
            Duration::from_secs(secs)
        ))),
    ]
}

pub(crate) fn message() -> impl Strategy<Value = Message> {
    prop_oneof![
        overlay().prop_map(Message::Overlay),
        playback().prop_map(Message::Playback),
        browse().prop_map(Message::Browse),
        queue().prop_map(Message::Queue),
        audio().prop_map(Message::Audio),
        loaded().prop_map(Message::Playlist),
        library().prop_map(Message::Library),
        config().prop_map(Message::Config),
        driver(),
        event(),
        system().prop_map(Message::Macos),
    ]
}

pub(crate) fn reached_model() -> impl Strategy<Value = Model> {
    let seed = prop_oneof![
        Just(Model::default()),
        (1usize..6).prop_map(model_with_dated_tracks),
        (1usize..6).prop_map(playing_model),
    ];
    (seed, proptest::collection::vec(message(), 0..16)).prop_map(|(seed, path)| {
        path.into_iter().fold(seed, |mut model, message| {
            let _ = update(&mut model, message, Moment::default());
            model
        })
    })
}
