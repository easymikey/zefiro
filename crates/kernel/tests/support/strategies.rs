use std::{ops::Range, time::Duration};

use kernel::{
    AudioEvent,
    AudioFailure,
    Bounded,
    BrowseRequest,
    ConfigFact,
    DecodeFault,
    DriverMessage,
    EngineRejection,
    Gesture,
    HistoryRequest,
    JumpRequest,
    Key,
    KeyCode,
    KeyPress,
    LibraryFact,
    LibraryFailure,
    LoadedRequest,
    Message,
    Model,
    Moment,
    Nudge,
    OverlayName,
    OverlayRequest,
    Percent,
    PlaybackRequest,
    SearchEdit,
    SearchRequest,
    SettingsRowRequest,
    SystemEvent,
    TextRequest,
    Timer,
    Toast,
    WorkspaceRequest,
    domain::{
        ChordPrefix,
        Driver,
        DriverFailure,
        OutputFault,
        PlaylistIndex,
        Revision,
        SettingRow,
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

fn nudge() -> impl Strategy<Value = Nudge> {
    prop_oneof![Just(Nudge::Up), Just(Nudge::Down)]
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
    select(vec![
        OverlayName::Help,
        OverlayName::Search,
        OverlayName::SavePlaylist,
        OverlayName::History,
        OverlayName::Settings,
        OverlayName::ConfirmDelete,
        OverlayName::TrackDetails,
        OverlayName::JumpToTime,
        OverlayName::SourceDir,
    ])
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
        nudge()
            .prop_map(|nudge| OverlayRequest::Search(SearchRequest::Navigate(nudge))),
        Just(OverlayRequest::Search(SearchRequest::Enqueue)),
        nudge().prop_map(|nudge| {
            OverlayRequest::Settings(SettingsRowRequest::Navigate(nudge))
        }),
        nudge().prop_map(|nudge| OverlayRequest::Settings(SettingsRowRequest::Adjust(
            nudge
        ))),
        Just(OverlayRequest::Settings(SettingsRowRequest::Activate)),
        typed
            .clone()
            .prop_map(|character| OverlayRequest::Text(TextRequest::Char(character))),
        Just(OverlayRequest::Text(TextRequest::Backspace)),
        typed.prop_map(|character| OverlayRequest::Jump(JumpRequest::Char(character))),
    ]
}

fn overlay() -> impl Strategy<Value = OverlayRequest> {
    prop_oneof![
        overlay_name().prop_map(OverlayRequest::Open),
        Just(OverlayRequest::Close),
        Just(OverlayRequest::Confirm),
        overlay_input(),
        Just(OverlayRequest::Jump(JumpRequest::Backspace)),
        select(vec![
            HistoryRequest::Top,
            HistoryRequest::Bottom,
            HistoryRequest::Enqueue,
            HistoryRequest::Navigate(Nudge::Up),
            HistoryRequest::Navigate(Nudge::Down),
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
            PlaybackRequest::Prev,
            PlaybackRequest::ToggleShuffle,
            PlaybackRequest::CycleRepeat,
            PlaybackRequest::CycleSleep,
            PlaybackRequest::AbMark,
        ]),
        (-30i64..30).prop_map(PlaybackRequest::SeekBy),
        (-3i8..3).prop_map(PlaybackRequest::NudgeVolume),
        (-3i8..3).prop_map(PlaybackRequest::NudgeSpeed),
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
            BrowseRequest::Enqueue,
            BrowseRequest::PlayNext,
            BrowseRequest::Dequeue,
            BrowseRequest::CycleSort,
            BrowseRequest::Rescan,
            BrowseRequest::ToggleFavorite,
            BrowseRequest::SavePlaylist(PlaylistFileName::new("mix").unwrap()),
        ]),
        playlist_index().prop_map(BrowseRequest::Trash),
        playlist_index().prop_map(BrowseRequest::EnqueueTrack),
        playlist_index().prop_map(BrowseRequest::CursorTo),
        (-4i64..4).prop_map(BrowseRequest::CursorBy),
        nudge().prop_map(BrowseRequest::MoveInQueue),
        nudge().prop_map(BrowseRequest::PageBy),
    ]
}

fn audio() -> impl Strategy<Value = AudioEvent> {
    let failure = prop_oneof![
        Just(AudioFailure::Decode {
            path: "/tmp/track0.flac".into(),
            fault: DecodeFault::Corrupt,
        }),
        Just(AudioFailure::OutputLost {
            fault: OutputFault::DeviceGone,
        }),
    ];
    prop_oneof![
        (0u64..200).prop_map(|secs| AudioEvent::Playhead(Duration::from_secs(secs))),
        select(vec![
            AudioEvent::TrackChanged,
            AudioEvent::Ended,
            AudioEvent::DeviceFellBack(None),
            AudioEvent::DevicesLoaded(Vec::new()),
            AudioEvent::Loaded { total: None },
            AudioEvent::Rejected(EngineRejection::WhileNotPlaying(
                "/tmp/track1.flac".into()
            )),
        ]),
        failure.prop_map(AudioEvent::Error),
    ]
}

fn loaded() -> impl Strategy<Value = LoadedRequest> {
    prop_oneof![
        playlist_index().prop_map(LoadedRequest::Jump),
        proptest::collection::vec(0usize..6, 0..6)
            .prop_map(LoadedRequest::ShuffleRolled),
    ]
}

fn library() -> impl Strategy<Value = LibraryFact> {
    select(vec![
        LibraryFact::HistoryLoaded(Vec::new()),
        LibraryFact::FavoritesLoaded(std::collections::HashSet::new()),
        LibraryFact::Failed(LibraryFailure::NoDirectory),
    ])
}

fn config() -> impl Strategy<Value = ConfigFact> {
    select(vec![
        ConfigFact::ThemesLoaded(vec![ThemeName::from_static("dusk")]),
        ConfigFact::MusicDirReloaded("/tmp".into()),
        ConfigFact::ThemeReloaded(ThemeName::from_static("dusk")),
    ])
}

fn driver() -> impl Strategy<Value = Message> {
    let driver = select(vec![
        Driver::Audio,
        Driver::Library,
        Driver::Config,
        Driver::Macos,
    ]);
    let change = prop_oneof![
        Just(DriverMessage::Died(DriverFailure::Panicked(
            "boom".to_string()
        ))),
        Just(DriverMessage::Stopped),
        Just(DriverMessage::Congested),
    ];
    (driver, change).prop_map(|(driver, change)| Message::Driver(driver, change))
}

fn stamped() -> impl Strategy<Value = Revision> {
    (0u64..20).prop_map(|stamp| {
        (0..stamp).fold(Revision::UNSTAMPED, |revision, _| revision.next())
    })
}

fn event() -> impl Strategy<Value = Message> {
    prop_oneof![
        (
            select(vec![SettingRow::Crossfade, SettingRow::Replaygain]),
            nudge()
        )
            .prop_map(|(row, nudge)| Message::Adjust { row, nudge }),
        select(vec![
            WorkspaceRequest::ShowToast(Toast::info("hello".to_string())),
            WorkspaceRequest::ClearToast,
        ])
        .prop_map(Message::Workspace),
        stamped().prop_map(|revision| Message::Elapsed(Timer::Toast(revision))),
        stamped().prop_map(|revision| Message::Elapsed(Timer::Sleep(revision))),
        select(vec![
            Driver::Audio,
            Driver::Library,
            Driver::Config,
            Driver::Macos
        ])
        .prop_map(|driver| Message::Elapsed(Timer::Restart(driver))),
        (0usize..50).prop_map(|visible_rows| Message::Viewport { visible_rows }),
        key_press().prop_map(Message::Key),
    ]
}

fn system() -> impl Strategy<Value = SystemEvent> {
    prop_oneof![
        (0u8..=100).prop_map(|volume| SystemEvent::Volume(Percent::clamped(volume))),
        Just(SystemEvent::OutputRouteChanged),
        select(vec![
            Gesture::Play,
            Gesture::Pause,
            Gesture::Toggle,
            Gesture::Stop,
            Gesture::Next,
            Gesture::Previous,
            Gesture::SeekForward,
            Gesture::SeekBack,
        ])
        .prop_map(SystemEvent::MediaKey),
        (0u64..200).prop_map(|secs| SystemEvent::MediaKey(Gesture::Scrub(
            Duration::from_secs(secs)
        ))),
    ]
}

pub(crate) fn message() -> impl Strategy<Value = Message> {
    prop_oneof![
        overlay().prop_map(Message::Overlay),
        playback().prop_map(Message::Playback),
        browse().prop_map(Message::Browse),
        audio().prop_map(Message::Audio),
        loaded().prop_map(Message::Loaded),
        library().prop_map(Message::Library),
        config().prop_map(Message::Config),
        driver(),
        event(),
        system().prop_map(Message::System),
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
