use std::time::Duration;

use kernel::{
    AudioEvent,
    Direction,
    HistoryRequest,
    Message,
    Model,
    Moment,
    OverlayName,
    OverlayRequest,
    PlaybackRequest,
    PlaylistRequest,
    QueueRequest,
    SearchEdit,
    SearchRequest,
    TextRequest,
    Timer,
    domain::{
        AudioFormat,
        Cursor,
        CursorOver,
        History,
        HistoryEntry,
        Loaded,
        Overlay,
        Player,
        Playhead,
        PlaylistIndex,
        Preload,
        Revision,
        Speed,
        Tags,
        Track,
        TrackIndex,
        UnixSeconds,
    },
    library::Library,
    playlist::RepeatMode,
};

use crate::support::{
    dated_track,
    model_with_titled_tracks,
    playing_model,
    titled_track,
};

pub(crate) fn open(kind: OverlayName) -> Message {
    Message::Overlay(OverlayRequest::Open(kind))
}

pub(crate) fn close() -> Message {
    Message::Overlay(OverlayRequest::Close)
}

pub(crate) fn confirm() -> Message {
    Message::Overlay(OverlayRequest::Confirm)
}

pub(crate) fn typed(text: &str, into: fn(char) -> Message) -> Vec<Message> {
    text.chars().map(into).collect()
}

pub(crate) fn search_char(character: char) -> Message {
    Message::Overlay(OverlayRequest::Search(SearchRequest::Edit(
        SearchEdit::Char(character),
    )))
}

pub(crate) fn search_backspace() -> Message {
    Message::Overlay(OverlayRequest::Search(SearchRequest::Edit(
        SearchEdit::Backspace,
    )))
}

pub(crate) fn search_nav(direction: Direction) -> Message {
    Message::Overlay(OverlayRequest::Search(SearchRequest::Navigate(direction)))
}

pub(crate) fn search_enqueue() -> Message {
    Message::Overlay(OverlayRequest::Search(SearchRequest::Enqueue))
}

pub(crate) fn history_enqueue() -> Message {
    Message::Overlay(OverlayRequest::History(HistoryRequest::Enqueue))
}

pub(crate) fn jump_char(character: char) -> Message {
    Message::Overlay(OverlayRequest::Jump(TextRequest::Char(character)))
}

pub(crate) fn text_char(character: char) -> Message {
    Message::Overlay(OverlayRequest::Text(TextRequest::Char(character)))
}

pub(crate) fn moon_library() -> Model {
    model_with_titled_tracks(&[
        ("/m/0.flac", "Moon River", "Audrey Hepburn"),
        ("/m/1.flac", "Sun Song", "Someone"),
        ("/m/2.flac", "Moonlight Sonata", "Beethoven"),
    ])
}

pub(crate) fn moon_library_selecting(row: usize) -> Model {
    let mut model = moon_library();
    model.workspace.browse.cursor = Cursor::with_len(3).at(row);
    model
}

pub(crate) fn logged(log: &[&str], playlist: &[&str]) -> Model {
    let mut model = Model {
        history: History {
            view: log
                .iter()
                .map(|path| HistoryEntry {
                    path: (*path).into(),
                    title: (*path).to_string(),
                    artist: None,
                    at: UnixSeconds::new(0),
                })
                .collect(),
        },
        ..Default::default()
    };
    model.playlist.tracks = playlist
        .iter()
        .map(|path| titled_track(path, path, ""))
        .collect();
    model.workspace.overlay = Some(Overlay::History(CursorOver {
        cursor: Cursor::with_len(log.len()).at(log.len().saturating_sub(1)),
        content: (),
    }));
    model
}

pub(crate) fn playing_nothing_selected(duration: Duration) -> Model {
    Model {
        player: Player::Playing {
            track: Track::builder()
                .path("/m/0.flac")
                .duration(duration)
                .tags(Tags::default())
                .audio_format(AudioFormat::default())
                .build()
                .into(),
            head: Playhead::anchored(
                Duration::ZERO,
                Moment::default(),
                Speed::default(),
            ),
            preload: Preload::None,
        },
        ..Default::default()
    }
}

pub(crate) fn moon_library_scanned() -> Model {
    let mut model = moon_library();
    model.library = Loaded::Ready(Library {
        all: model.playlist.tracks.clone(),
        view: (0..model.playlist.tracks.len())
            .map(TrackIndex::new)
            .collect(),
    });
    model
}

pub(crate) fn spinning(count: usize) -> Model {
    playing_model(count)
}

pub(crate) fn spinning_at(count: usize, at: usize) -> Model {
    let mut model = spinning(count);
    model.playlist.cursor = Cursor::with_len(count).at(at);
    model.player = Player::Playing {
        track: dated_track(at),
        head: Playhead::anchored(Duration::ZERO, Moment::default(), Speed::default()),
        preload: Preload::None,
    };
    model
}

pub(crate) fn repeating(mut model: Model, repeat: RepeatMode) -> Model {
    model.playlist.repeat = repeat;
    model
}

pub(crate) fn queued(mut model: Model, queue: &[usize]) -> Model {
    model.queue = queue.iter().copied().map(PlaylistIndex::new).collect();
    model
}

pub(crate) fn near_the_end() -> Message {
    Message::Audio(AudioEvent::Playhead(Duration::from_secs(95)))
}

pub(crate) fn handed_off() -> Message {
    Message::Audio(AudioEvent::TrackChanged)
}

pub(crate) fn ended() -> Message {
    Message::Audio(AudioEvent::Ended)
}

pub(crate) fn acknowledged() -> Message {
    Message::Audio(AudioEvent::Loaded { total: None })
}

pub(crate) fn skip() -> Message {
    Message::Playback(PlaybackRequest::Next)
}

pub(crate) fn enqueue(track: usize) -> Message {
    Message::Queue(QueueRequest::EnqueueTrack(PlaylistIndex::new(track)))
}

pub(crate) fn shuffle() -> Message {
    Message::Playback(PlaybackRequest::ToggleShuffle)
}

pub(crate) fn shuffled(order: Vec<usize>) -> Message {
    Message::Loaded(PlaylistRequest::ShuffleRolled(order))
}

pub(crate) fn a_lap_of(laps: usize) -> Vec<Message> {
    (0..laps).flat_map(|_| [ended(), acknowledged()]).collect()
}

pub(crate) fn media(request: PlaybackRequest) -> Message {
    Message::Playback(request)
}

pub(crate) fn mark_ab() -> Message {
    Message::Playback(PlaybackRequest::AbMark)
}

pub(crate) fn mark_fires() -> Message {
    Message::Elapsed(Timer::Mark(Revision::UNSTAMPED))
}

pub(crate) fn nudge_speed(steps: i8) -> Message {
    Message::Playback(PlaybackRequest::NudgeSpeed { steps })
}

pub(crate) fn spinning_past(at: u64) -> Model {
    let mut model = spinning(1);
    model.player = Player::Playing {
        track: dated_track(0),
        head: Playhead::anchored(
            Duration::from_secs(at),
            Moment::default(),
            Speed::default(),
        ),
        preload: Preload::None,
    };
    model
}

pub(crate) fn toasted() -> Model {
    let mut model = Model::default();
    model.workspace.toast = Some(kernel::Toast {
        level: kernel::ToastLevel::Info,
        text: "a toast".to_string(),
    });
    model
}

pub(crate) fn search_moon(then: Vec<Message>) -> Vec<Message> {
    let mut messages = vec![open(OverlayName::Search)];
    messages.extend(typed("moon", search_char));
    messages.extend(then);
    messages
}
