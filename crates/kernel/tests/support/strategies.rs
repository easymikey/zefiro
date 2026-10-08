use std::time::Duration;

use kernel::{
    domain::{
        bounded::Bounded,
        chord::ChordPrefix,
        device::OutputDevice,
        direction::Direction,
        driver::{DriverError, DriverName},
        favorites::Favorites,
        geometry::Cells,
        index::ViewIndex,
        key::{Key, KeyCode, KeyPress},
        model::Model,
        overlay::OverlayName,
        percent::Percent,
        playlist::{PlaylistFileName, RepeatMode},
        revision::Revision,
        setting_row::SettingRow,
        theme::ThemeName,
        time::Moment,
        toast::Toast,
        transport::OutputError,
    },
    message::{
        AudioError,
        AudioEvent,
        BrowseRequest,
        ConfigEvent,
        DecodeError,
        DriverEvent,
        HistoryRequest,
        LibraryError,
        LibraryEvent,
        MacosEvent,
        Message,
        OverlayRequest,
        PlaybackRequest,
        QueueRequest,
        SearchEdit,
        SearchRequest,
        SeekTenths,
        SettingRowRequest,
        TextRequest,
        Timer,
    },
    update::update,
};
use proptest::{
    prelude::{Just, Strategy, prop_oneof},
    sample::select,
};
use strum::IntoEnumIterator;

use crate::support::{device, model_with_dated_tracks, playing_model};

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

fn playlist_index() -> impl Strategy<Value = ViewIndex> {
    (0usize..6).prop_map(ViewIndex::new)
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
            OverlayRequest::Settings(SettingRowRequest::Navigate(direction))
        }),
        direction().prop_map(|direction| OverlayRequest::Settings(
            SettingRowRequest::Step(direction)
        )),
        Just(OverlayRequest::Settings(SettingRowRequest::Activate)),
        typed.prop_map(|character| OverlayRequest::Text(TextRequest::Char(character))),
        Just(OverlayRequest::Text(TextRequest::Backspace)),
    ]
}

fn overlay() -> impl Strategy<Value = OverlayRequest> {
    prop_oneof![
        overlay_name().prop_map(OverlayRequest::Open),
        Just(OverlayRequest::Close),
        Just(OverlayRequest::Confirm),
        overlay_input(),
        select(vec![
            HistoryRequest::SelectFirst,
            HistoryRequest::SelectLast,
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
            PlaybackRequest::HoldForOverlay,
            PlaybackRequest::Release,
            PlaybackRequest::Stop,
            PlaybackRequest::Next,
            PlaybackRequest::Previous,
            PlaybackRequest::ToggleShuffle,
            PlaybackRequest::CycleRepeat,
            PlaybackRequest::CycleSleep,
            PlaybackRequest::AbMark,
        ]),
        (direction(), 0u64..30).prop_map(|(direction, secs)| PlaybackRequest::SeekBy {
            direction,
            by: Duration::from_secs(secs),
        }),
        direction().prop_map(PlaybackRequest::StepVolume),
        direction().prop_map(PlaybackRequest::StepSpeed),
        (0u64..200).prop_map(|secs| PlaybackRequest::SeekTo(Duration::from_secs(secs))),
        (0u8..10).prop_map(|tenths| PlaybackRequest::SeekTenths(SeekTenths::clamped(
            tenths
        ))),
    ]
}

fn browse() -> impl Strategy<Value = BrowseRequest> {
    prop_oneof![
        select(vec![
            BrowseRequest::SelectFirst,
            BrowseRequest::SelectLast,
            BrowseRequest::PlaySelected,
            BrowseRequest::CycleSort,
            BrowseRequest::Rescan,
            BrowseRequest::ToggleFavorite,
            BrowseRequest::SavePlaylist(PlaylistFileName::new("mix").unwrap()),
        ]),
        (0usize..4).prop_map(|row| {
            BrowseRequest::Trash(kernel::domain::track::TrackSource::Local(
                format!("/tmp/track{row}.flac").into(),
            ))
        }),
        (-4isize..4).prop_map(|rows| BrowseRequest::CursorBy { rows }),
        direction().prop_map(BrowseRequest::PageBy),
    ]
}

fn queue() -> impl Strategy<Value = QueueRequest> {
    prop_oneof![
        select(vec![
            QueueRequest::Toggle,
            QueueRequest::PlayNext,
            QueueRequest::Dequeue,
        ]),
        playlist_index().prop_map(QueueRequest::ToggleAt),
        direction().prop_map(QueueRequest::Move),
    ]
}

fn audio() -> impl Strategy<Value = AudioEvent> {
    let error = prop_oneof![Just(AudioError::Decode {
        path: "/tmp/track0.flac".into(),
        error: DecodeError::Corrupt,
    }),];
    prop_oneof![
        (0u64..200)
            .prop_map(|secs| AudioEvent::PositionReported(Duration::from_secs(secs))),
        select(vec![
            AudioEvent::TrackChanged,
            AudioEvent::Ended,
            AudioEvent::DeviceFellBack(OutputDevice::SystemDefault),
            AudioEvent::DeviceOpened(device("Speakers")),
            AudioEvent::DeviceOpened(device("Headphones")),
            AudioEvent::DevicesListed(Vec::new()),
            AudioEvent::Loaded(None),
            AudioEvent::OutputLost(OutputError::DeviceGone),
        ]),
        error.prop_map(AudioEvent::Error),
    ]
}

fn loaded() -> impl Strategy<Value = PlaybackRequest> {
    playlist_index().prop_map(PlaybackRequest::JumpTo)
}

fn library() -> impl Strategy<Value = LibraryEvent> {
    prop_oneof![
        select(vec![
            LibraryEvent::HistoryLoaded(Vec::new()),
            LibraryEvent::FavoritesLoaded(Favorites::default()),
            LibraryEvent::Error(LibraryError::NoUserDirs),
        ]),
        (0usize..4).prop_map(|row| {
            LibraryEvent::Trashed(format!("/tmp/track{row}.flac").into())
        }),
    ]
}

fn config() -> impl Strategy<Value = ConfigEvent> {
    select(vec![
        ConfigEvent::ThemesLoaded {
            theme_names: vec![ThemeName::from_static("dusk")],
            refused: Vec::new(),
        },
        ConfigEvent::MusicDirReloaded("/tmp".into()),
        ConfigEvent::ThemeReloaded(ThemeName::from_static("dusk")),
    ])
}

fn driver() -> impl Strategy<Value = Message> {
    let driver_name = select(DriverName::ALL.to_vec());
    let change = prop_oneof![
        Just(DriverEvent::Died(DriverError::Panicked)),
        Just(DriverEvent::Stopped),
        Just(DriverEvent::Full),
    ];
    (driver_name, change).prop_map(|(driver_name, change)| Message::Driver {
        driver_name,
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
            select(vec![SettingRow::Crossfade, SettingRow::ReplayGain]),
            direction()
        )
            .prop_map(|(row, direction)| Message::Step { row, direction }),
        Just(Message::Toast(Toast::info("hello".to_string()))),
        stamped().prop_map(|revision| Message::Elapsed(Timer::Toast(revision))),
        stamped().prop_map(|revision| Message::Elapsed(Timer::Sleep(revision))),
        (0u16..50).prop_map(|rows| Message::Viewport {
            visible_rows: Cells(rows),
            cover_side: None,
        }),
        key_press().prop_map(Message::Key),
    ]
}

fn system() -> impl Strategy<Value = MacosEvent> {
    prop_oneof![
        (0u8..=100)
            .prop_map(|volume| MacosEvent::VolumeChanged(Percent::clamped(volume))),
        Just(MacosEvent::OutputRouteChanged),
        select(vec![
            PlaybackRequest::Play,
            PlaybackRequest::Pause,
            PlaybackRequest::Toggle,
            PlaybackRequest::Stop,
            PlaybackRequest::Next,
            PlaybackRequest::Previous,
            PlaybackRequest::SeekBy {
                direction: Direction::Next,
                by: Duration::from_secs(10),
            },
            PlaybackRequest::SeekBy {
                direction: Direction::Previous,
                by: Duration::from_secs(10),
            },
        ])
        .prop_map(MacosEvent::MediaKeyPressed),
        (0u64..200).prop_map(|secs| MacosEvent::MediaKeyPressed(
            PlaybackRequest::SeekTo(Duration::from_secs(secs))
        )),
    ]
}

pub(crate) fn message() -> impl Strategy<Value = Message> {
    prop_oneof![
        overlay().prop_map(Message::Overlay),
        playback().prop_map(Message::Playback),
        browse().prop_map(Message::Browse),
        Just(Message::ChordPrefix(ChordPrefix::G)),
        queue().prop_map(Message::Queue),
        audio().prop_map(Message::Audio),
        loaded().prop_map(Message::Playback),
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
            drop(update(&mut model, message, Moment::default()));
            model
        })
    })
}
