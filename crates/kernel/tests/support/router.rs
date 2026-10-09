use std::time::Duration;

use kernel::{
    domain::{
        cursor::Cursor,
        cursor_over::CursorOver,
        direction::Direction,
        geometry::{Cells, Pixels},
        history::HistoryEntry,
        index::{TrackIndex, ViewIndex},
        library::Library,
        model::Model,
        overlay::{Overlay, OverlayName},
        player::Player,
        playhead::Playhead,
        playlist::RepeatMode,
        revision::Revision,
        speed::Speed,
        time::Moment,
        track::{AudioFormat, Tags, Track, TrackParts},
    },
    message::{
        AudioEvent,
        HistoryRequest,
        Message,
        OverlayRequest,
        PlaybackRequest,
        QueueRequest,
        SearchRequest,
        TextRequest,
        Timer,
    },
};

use crate::support::{listed_model, model_with_titled_tracks, titled_track};

pub(crate) fn cover_side_known() -> Message {
    Message::Viewport {
        visible_rows: Cells(10),
        cover_side: Some(Pixels(240)),
    }
}

pub(crate) fn cover_side_unknown() -> Message {
    Message::Viewport {
        visible_rows: Cells(10),
        cover_side: None,
    }
}

pub(crate) fn open(overlay_name: OverlayName) -> Message {
    Message::Overlay(OverlayRequest::Open(overlay_name))
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
        TextRequest::Char(character),
    )))
}

pub(crate) fn search_backspace() -> Message {
    Message::Overlay(OverlayRequest::Search(SearchRequest::Edit(
        TextRequest::Backspace,
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

pub(crate) fn moon_library_selecting(selected_index: usize) -> Model {
    let mut model = moon_library();
    model.workspace.browse.cursor = Cursor::at(3, selected_index);
    model
}

pub(crate) fn logged(log: &[&str], playlist: &[&str]) -> Model {
    let mut model = listed_model(
        playlist
            .iter()
            .map(|path| titled_track(path, path, ""))
            .collect(),
    );
    model.history = log
        .iter()
        .map(|path| HistoryEntry {
            track_source: kernel::domain::track::TrackSource::Local((*path).into()),
            title: (*path).to_string(),
            artist: None,
            played_at: Moment::default(),
        })
        .collect();
    model.workspace.overlay = Some(Overlay::History(CursorOver {
        cursor: Cursor::at(log.len(), log.len().saturating_sub(1)),
        content: (),
    }));
    model
}

pub(crate) fn playing_nothing_selected(duration: Duration) -> Model {
    Model {
        player: Player::Playing {
            track: Track::new(TrackParts {
                path: "/m/0.flac".into(),
                duration,
                tags: Tags::default(),
                audio_format: AudioFormat::default(),
            })
            .into(),
            playhead: Playhead::anchored(
                Duration::ZERO,
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        },
        ..Default::default()
    }
}

pub(crate) fn moon_library_scanned() -> Model {
    let mut model = moon_library();
    model.library = Some(Library {
        tracks: model.playlist.tracks.clone(),
        track_indexes: (0..model.playlist.tracks.len())
            .map(TrackIndex::new)
            .collect(),
    });
    model
}

pub(crate) fn repeating(mut model: Model, repeat_mode: RepeatMode) -> Model {
    model.playlist.repeat_mode = repeat_mode;
    model
}

pub(crate) fn queued(mut model: Model, rows: &[usize]) -> Model {
    model.queue = rows
        .iter()
        .map(|&row| model.playlist.tracks[row].source().clone())
        .collect();
    model
}

pub(crate) fn near_the_end() -> Message {
    Message::Audio(AudioEvent::PositionReported {
        position: Duration::from_secs(95),
        revision: Revision::default(),
    })
}

pub(crate) fn handed_off() -> Message {
    Message::Audio(AudioEvent::TrackChanged)
}

pub(crate) fn ended() -> Message {
    Message::Audio(AudioEvent::Ended)
}

pub(crate) fn acknowledged() -> Message {
    Message::Audio(AudioEvent::Loaded(None))
}

pub(crate) fn skip() -> Message {
    Message::Playback(PlaybackRequest::Next)
}

pub(crate) fn enqueue(view_index: usize) -> Message {
    Message::Queue(QueueRequest::ToggleAt(ViewIndex::new(view_index)))
}

pub(crate) fn shuffle() -> Message {
    Message::Playback(PlaybackRequest::ToggleShuffle)
}

pub(crate) fn shuffled(order: Vec<usize>) -> Message {
    Message::ShuffleRolled(order.into_iter().map(ViewIndex::new).collect())
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
    Message::Elapsed(Timer::Lookahead(Revision::default()))
}

pub(crate) fn step_speed(direction: Direction) -> Message {
    Message::Playback(PlaybackRequest::StepSpeed(direction))
}

pub(crate) fn toasted() -> Model {
    let mut model = Model::default();
    model.workspace.toasts = vec![kernel::domain::toast::Toast::info("a toast")];
    model
}

pub(crate) fn search_moon(messages: Vec<Message>) -> Vec<Message> {
    let mut all = vec![open(OverlayName::Search)];
    all.extend(typed("moon", search_char));
    all.extend(messages);
    all
}
