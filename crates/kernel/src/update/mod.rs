mod audio;
mod browse;
mod config;
mod driver;
pub mod keymap;
mod library;
pub mod machine;
mod macos;
pub mod overlay;
mod playback;
pub mod player;
mod playlist;
mod settings;
pub mod startup;
mod successor;
mod transport;
mod workspace;

use std::sync::Arc;

use crate::{
    cmd::{AudioCmd, Cmd, CoverJob, Effect, LibraryCmd, WindowColorsCmd},
    domain::{
        appearance::CoverMode,
        cue::Cue,
        geometry::Pixels,
        model::Model,
        playlist::{PlayOrder, Playlist},
        revision::{Freshness, Revision},
        settings::Settings,
        supervision::{Announcement, Decision},
        time::Moment,
        toast::Toast,
        track::Track,
        workspace::Workspace,
    },
    message::{
        DriverEvent,
        MacosEvent,
        Message,
        OverlayRequest,
        PaintError,
        QueueRequest,
        Timer,
    },
    update::machine::{Machine, Unhandled},
};

fn paint_toast(error: &PaintError) -> Toast {
    Toast::error(error.to_string()).with_text(error.diagnostic().text())
}

const DRAIN_DEPTH: usize = 8;

pub fn update(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Vec<Effect>, Unhandled> {
    let message = if let Message::Key(press) = message {
        match keymap::lookup::route(&model.workspace, press) {
            Some(routed) => routed,
            None => {
                return release_chord_prefix(&mut model.workspace, Input::Request)
                    .then(Vec::new)
                    .ok_or(Unhandled);
            }
        }
    } else {
        message
    };
    if let Message::Quit = message {
        return Ok(quit().into_parts().0);
    }
    let before = shown_cover(model);
    let mut effects = route(model, message, now)?;
    effects.extend(decode_cover(before.as_ref(), model));
    Ok(effects)
}

fn route(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Vec<Effect>, Unhandled> {
    if let Message::Viewport {
        visible_rows,
        cover_side,
    } = message
    {
        let workspace = &mut model.workspace;
        if (workspace.visible_rows, workspace.cover_side) == (visible_rows, cover_side)
        {
            return Err(Unhandled);
        }
        workspace.visible_rows = visible_rows;
        workspace.cover_side = cover_side;
        return Ok(Vec::new());
    }
    let mut effects = update_model(model, message, now)?;
    effects.extend(roll_pending(&model.playlist));
    Ok(effects)
}

pub(crate) fn cover_side(workspace: &Workspace, settings: &Settings) -> Option<Pixels> {
    match settings.appearance_settings.cover_mode {
        CoverMode::Plain | CoverMode::Vinyl => workspace.cover_side,
        CoverMode::Milkdrop | CoverMode::Off => None,
    }
}

fn shown_cover(model: &Model) -> Option<(Arc<Track>, Pixels)> {
    let side = cover_side(&model.workspace, &model.settings)?;
    let track = model.player.current()?;
    Some((Arc::clone(track), side))
}

fn decode_cover(
    before: Option<&(Arc<Track>, Pixels)>,
    model: &Model,
) -> Option<Effect> {
    shown_cover(model)
        .filter(|(track, side)| {
            before.is_none_or(|(shown, shown_side)| {
                (shown.path(), shown_side) != (track.path(), side)
            })
        })
        .map(|(track, side)| {
            Effect::Library(LibraryCmd::DecodeCover(CoverJob {
                path: track.path().to_path_buf(),
                side,
            }))
        })
}

fn drain(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Vec<Effect>, Unhandled> {
    branch(model, message, now).map(|cmd| drained(model, cmd))
}

fn drained(model: &mut Model, cmd: Cmd) -> Vec<Effect> {
    let (effects, messages) = cmd.into_parts();
    effects
        .into_iter()
        .chain(follow_up(model, messages, 1))
        .collect()
}

fn follow_up(model: &mut Model, messages: Vec<Message>, depth: usize) -> Vec<Effect> {
    messages
        .into_iter()
        .flat_map(|message| follow_one(model, message, depth))
        .collect()
}

fn follow_one(model: &mut Model, message: Message, depth: usize) -> Vec<Effect> {
    let clock = model.workspace.clock;
    let Some(cmd) = branch(model, message, clock).ok() else {
        return Vec::new();
    };
    let (effects, queued) = cmd.into_parts();
    if depth >= DRAIN_DEPTH {
        debug_assert!(queued.is_empty(), "message chain deeper than {DRAIN_DEPTH}");
        return effects;
    }
    effects
        .into_iter()
        .chain(follow_up(model, queued, depth + 1))
        .collect()
}

pub(crate) fn quit() -> Cmd {
    Cmd::from_iter([
        Effect::Audio(AudioCmd::Stop),
        Effect::Config(crate::cmd::ConfigCmd::Flush),
        Effect::WindowColors(WindowColorsCmd::Reset),
        Effect::Quit,
    ])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Input {
    Request,
    ChordPrefix,
    Event,
}

impl Input {
    fn of(message: &Message) -> Self {
        match message {
            Message::ChordPrefix(_) => Input::ChordPrefix,
            Message::Macos(MacosEvent::MediaKeyPressed(_))
            | Message::Overlay(_)
            | Message::Step { .. }
            | Message::Playback(_)
            | Message::Browse(_)
            | Message::Queue(_) => Input::Request,
            Message::Macos(
                MacosEvent::VolumeChanged(_)
                | MacosEvent::OutputRouteChanged
                | MacosEvent::Error(_),
            )
            | Message::Toast(_)
            | Message::Paint(_)
            | Message::ShuffleRolled(_)
            | Message::Library(_)
            | Message::Config(_)
            | Message::Audio(_)
            | Message::Elapsed(_)
            | Message::Driver { .. }
            | Message::Key(_)
            | Message::Viewport { .. }
            | Message::Quit => Input::Event,
        }
    }
}

fn roll_pending(playlist: &Playlist) -> Option<Effect> {
    match playlist.play_order {
        PlayOrder::ShufflePending => Some(Effect::RollShuffle(playlist.tracks.len())),
        PlayOrder::Linear | PlayOrder::Shuffled(_) => None,
    }
}

fn update_model(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Vec<Effect>, Unhandled> {
    let input = Input::of(&message);
    let previous = model.workspace.clock;
    model.workspace.clock = now;
    let dismissed = dismissal(&mut model.workspace, input);
    let effects = match drain(model, message, now) {
        Ok(effects) => effects,
        Err(refusal) => {
            restore(&mut model.workspace, dismissed);
            model.workspace.clock = previous;
            return if release_chord_prefix(&mut model.workspace, input) {
                Ok(Vec::new())
            } else {
                Err(refusal)
            };
        }
    };
    release_chord_prefix(&mut model.workspace, input);
    let cue = dismissed.map(|_| Effect::Animate(Cue::ToastDismissed));
    Ok(cue.into_iter().chain(effects).collect())
}

fn dismissal(workspace: &mut Workspace, input: Input) -> Option<Toast> {
    match input {
        Input::Request | Input::ChordPrefix => workspace.dismiss_newest(),
        Input::Event => None,
    }
}

fn restore(workspace: &mut Workspace, dismissed_toast: Option<Toast>) {
    if let Some(toast) = dismissed_toast {
        workspace.toasts.insert(0, toast);
    }
}

fn release_chord_prefix(workspace: &mut Workspace, input: Input) -> bool {
    match input {
        Input::Request => workspace.chord_prefix.take().is_some(),
        Input::ChordPrefix | Input::Event => false,
    }
}

fn config_parts(model: &mut Model) -> config::ConfigParts<'_> {
    let Model {
        workspace,
        revisions,
        settings,
        themes,
        music_dir,
        ..
    } = model;
    config::ConfigParts {
        workspace,
        revisions,
        settings,
        themes,
        music_dir,
    }
}

pub(crate) fn playback_parts(model: &mut Model) -> player::PlaybackParts<'_> {
    let Model {
        player,
        transport,
        playlist,
        queue,
        workspace,
        revisions,
        settings,
        ..
    } = model;
    player::PlaybackParts {
        player,
        transport,
        playlist,
        queue,
        workspace,
        revisions,
        settings,
    }
}

fn browse_parts(model: &mut Model) -> browse::BrowseParts<'_> {
    let Model {
        player,
        transport,
        playlist,
        queue,
        workspace,
        revisions,
        settings,
        library,
        favorites,
        scan_status,
        music_dir,
        playlist_source,
        ..
    } = model;
    browse::BrowseParts {
        playback_parts: player::PlaybackParts {
            player,
            transport,
            playlist,
            queue,
            workspace,
            revisions,
            settings,
        },
        library,
        favorites,
        scan_status,
        music_dir,
        playlist_source,
    }
}

pub(crate) fn library_parts(model: &mut Model) -> library::LibraryParts<'_> {
    let Model {
        library,
        favorites,
        history,
        scan_status,
        music_dir,
        revisions,
        workspace,
        playlist,
        playlist_source,
        player,
        queue,
        ..
    } = model;
    library::LibraryParts {
        library,
        favorites,
        history,
        scan_status,
        music_dir,
        revisions,
        workspace,
        playlist,
        playlist_source,
        player,
        queue,
    }
}

fn elapsed(model: &mut Model, timer: Timer, now: Moment) -> Result<Cmd, Unhandled> {
    match timer {
        Timer::Toast(revision) => toast_expired(
            &mut model.workspace,
            revision,
            revision.freshness(model.revisions.toast),
        ),
        Timer::Sleep(revision) => {
            sleep_fired(&mut playback_parts(model), revision, now)
        }
        Timer::Lookahead(revision) => {
            audio::lookahead_fired(&mut playback_parts(model), revision, now)
        }
    }
}

fn toast_expired(
    workspace: &mut Workspace,
    revision: Revision,
    freshness: Freshness,
) -> Result<Cmd, Unhandled> {
    match freshness {
        Freshness::Awaited if workspace.toasts.is_empty() => Err(Unhandled),
        Freshness::Awaited => Ok(workspace.expire_toasts(revision)),
        Freshness::Stale => Err(Unhandled),
    }
}

fn sleep_fired(
    playback_parts: &mut player::PlaybackParts<'_>,
    revision: Revision,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    match revision.freshness(playback_parts.revisions.sleep) {
        Freshness::Awaited => {
            let cleared = playback_parts
                .transport
                .transition(transport::TransportMessage::SleepFired)?;
            match player::update_player(
                playback_parts,
                player::PlayerMessage::SleepFired(now),
                now,
            ) {
                Ok(paused) => Ok(paused.then(cleared)),
                Err(Unhandled) => Ok(cleared),
            }
        }
        Freshness::Stale => Err(Unhandled),
    }
}

fn driver_died(
    model: &mut Model,
    driver_death: driver::DriverDeath,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    let driver_name = driver_death.driver_name;
    let toast = Toast::error(format!("The {driver_name} driver stopped"))
        .with_text(driver_death.error.to_string());
    let (decision, died) = driver::died(&mut model.drivers, driver_death, now)?;
    let decided = match decision {
        Decision::Degrade(Announcement::Toast) => {
            model.workspace.show(toast, &mut model.revisions)
        }
        Decision::Degrade(Announcement::Silent) => Cmd::none(),
        Decision::Restart => {
            let startup = startup::startup_cmd(model, driver_name);
            let resumed = driver::resume_driver(
                driver::ResumeParts {
                    player: &model.player,
                    revisions: &mut model.revisions,
                },
                driver_name,
                now,
            );
            Cmd::from(Effect::Restart(driver_name))
                .then(startup)
                .then(resumed)
        }
    };
    Ok(died.then(decided))
}

fn update_overlay(
    model: &mut Model,
    request: OverlayRequest,
) -> Result<Cmd, Unhandled> {
    overlay::update(
        overlay::OverlayParts {
            workspace: &mut model.workspace,
            playlist: &model.playlist,
            player: &model.player,
            history: &model.history,
            music_dir: &model.music_dir,
        },
        request,
    )
}

fn update_queue(
    model: &mut Model,
    queue_request: QueueRequest,
) -> Result<Cmd, Unhandled> {
    browse::queue(
        browse::QueueParts {
            playlist: &model.playlist,
            history: &model.history,
            browse: &mut model.workspace.browse,
            queue: &mut model.queue,
        },
        queue_request,
    )
}

fn branch(model: &mut Model, message: Message, now: Moment) -> Result<Cmd, Unhandled> {
    match message {
        Message::Overlay(request) => update_overlay(model, request),
        Message::Step { row, direction } => {
            settings::step_setting(config_parts(model), row, direction)
        }
        Message::Toast(toast) => Ok(model.workspace.show(toast, &mut model.revisions)),
        Message::Playback(playback_request) => {
            playback::update(&mut playback_parts(model), playback_request, now)
        }
        Message::ChordPrefix(prefix) => {
            model.workspace.chord_prefix = Some(prefix);
            Ok(Cmd::none())
        }
        Message::Browse(browse_request) => {
            browse::update(browse_parts(model), browse_request, now)
        }
        Message::Queue(queue_request) => update_queue(model, queue_request),
        Message::ShuffleRolled(order) => model
            .playlist
            .transition(playlist::PlaylistMessage::ShuffleRolled(order)),
        Message::Library(event) => library::update(library_parts(model), event),
        Message::Config(event) => config::update(config_parts(model), event),
        Message::Audio(audio_event) => {
            audio::update(&mut playback_parts(model), audio_event, now)
        }
        Message::Macos(event) => macos::update(&mut playback_parts(model), event, now),
        Message::Paint(error) => Ok(model
            .workspace
            .show(paint_toast(&error), &mut model.revisions)),
        Message::Elapsed(timer) => elapsed(model, timer, now),
        Message::Driver {
            driver_name,
            event: DriverEvent::Died(error),
        } => driver_died(model, driver::DriverDeath { driver_name, error }, now),
        Message::Driver { driver_name, event } => {
            driver::update(&mut model.drivers, driver_name, event)
        }
        Message::Key(_) | Message::Viewport { .. } | Message::Quit => Err(Unhandled),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        cmd::Effect,
        domain::{
            config::Diagnostic,
            cue::Cue,
            driver::DriverName,
            geometry::{Cells, Pixels},
            key::{Key, KeyCode, KeyPress, Modifiers},
            model::Model,
            revision::Revision,
            startup::{Shuffle, Startup},
            time::Moment,
            toast::Toast,
            track::Track,
        },
        message::{DriverEvent, Message, PaintError, Timer},
        update::{branch, machine::Unhandled, startup::startup, update},
    };

    #[test]
    fn a_queued_follow_up_message_adds_its_effects() {
        let mut model = Model::default();
        let full_message = Message::Driver {
            driver_name: DriverName::Audio,
            event: DriverEvent::Full,
        };

        let effects = update(&mut model, full_message, Moment::default()).unwrap();

        assert_eq!(
            effects,
            vec![
                Effect::Animate(Cue::ToastRaised),
                Effect::After {
                    delay: Duration::from_secs(5),
                    timer: Timer::Toast(Revision::default().next()),
                },
            ]
        );
        assert_eq!(
            model.workspace.toasts,
            vec![Toast::info("The audio driver is falling behind")]
        );
    }

    #[rstest]
    #[case::unbound('w')]
    #[case::empty_playlist('j')]
    fn a_refused_key_preserves_the_model(#[case] letter: char) {
        let mut model = Model::default();
        model.workspace.clock = Moment::new(Duration::from_secs(1));
        model.workspace.toasts = vec![Toast::info("hello")];
        let before = model.clone();
        let key = Key {
            code: KeyCode::Char(letter),
            modifiers: Modifiers::default(),
        };

        assert_eq!(
            update(
                &mut model,
                Message::Key(KeyPress { key, typed: key }),
                Moment::new(Duration::from_secs(2)),
            ),
            Err(Unhandled)
        );
        assert_eq!(model, before);
    }

    fn startup_with(shuffle: Shuffle) -> Startup {
        Startup {
            playlist_tracks: vec![
                Arc::new(Track::listed(Path::new("/music/a.flac"))),
                Arc::new(Track::listed(Path::new("/music/b.flac"))),
            ],
            shuffle,
            ..Startup::default()
        }
    }

    fn rolled_len(effects: Vec<Effect>) -> Option<usize> {
        effects.into_iter().find_map(|effect| {
            if let Effect::RollShuffle(len) = effect {
                Some(len)
            } else {
                None
            }
        })
    }

    #[rstest]
    #[case::on_rolls(Shuffle::On, Some(2))]
    #[case::off_rolls_nothing(Shuffle::Off, None)]
    fn startup_rolls_shuffle_only_when_on(
        #[case] shuffle: Shuffle,
        #[case] expected: Option<usize>,
    ) {
        let (_, cmd) = startup(startup_with(shuffle));

        assert_eq!(rolled_len(cmd), expected);
    }

    #[rstest]
    #[case::window_colors(
        PaintError::WriteWindowColors(diagnostic()),
        "Window colors failed"
    )]
    #[case::probe(PaintError::Query(diagnostic()), "Terminal probe failed")]
    fn a_paint_failure_shows_its_toast(#[case] error: PaintError, #[case] title: &str) {
        let mut model = Model::default();

        let effects = update(&mut model, Message::from(error), Moment::default());

        assert!(effects.is_ok());
        assert_eq!(
            model.workspace.toasts.last(),
            Some(&Toast::error(title).with_text("broken pipe"))
        );
    }

    fn diagnostic() -> Diagnostic {
        Diagnostic::from_error(&std::io::Error::other("broken pipe"))
    }

    #[rstest]
    #[case::unchanged(Cells(0), None, Err(Unhandled))]
    #[case::new_rows(Cells(12), None, Ok(Vec::new()))]
    #[case::new_cover_side(Cells(0), Some(Pixels(240)), Ok(Vec::new()))]
    fn a_viewport_is_handled_only_when_it_changes(
        #[case] visible_rows: Cells,
        #[case] cover_side: Option<Pixels>,
        #[case] expected: Result<Vec<Effect>, Unhandled>,
    ) {
        let mut model = Model::default();
        let viewport_message = Message::Viewport {
            visible_rows,
            cover_side,
        };

        let effects = update(&mut model, viewport_message, Moment::default());

        assert_eq!(effects, expected);
        assert_eq!(model.workspace.visible_rows, visible_rows);
        assert_eq!(model.workspace.cover_side, cover_side);
    }

    #[test]
    fn a_key_reaching_the_branch_is_refused() {
        let mut model = Model::default();
        let before = model.clone();
        let key = Key {
            code: KeyCode::Char('j'),
            modifiers: Modifiers::default(),
        };

        let result = branch(
            &mut model,
            Message::Key(KeyPress { key, typed: key }),
            Moment::default(),
        );

        assert_eq!(result, Err(Unhandled));
        assert_eq!(model, before);
    }

    #[test]
    fn a_viewport_reaching_the_branch_is_refused() {
        let mut model = Model::default();
        let before = model.clone();
        let viewport_message = Message::Viewport {
            visible_rows: Cells(12),
            cover_side: Some(Pixels(240)),
        };

        let result = branch(&mut model, viewport_message, Moment::default());

        assert_eq!(result, Err(Unhandled));
        assert_eq!(model, before);
    }
}
