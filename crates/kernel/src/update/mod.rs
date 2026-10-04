mod audio;
mod browse;
mod config;
mod driver;
pub mod keymap;
mod library;
mod machine;
mod macos;
pub mod overlay;
mod playback;
pub mod player;
mod playlist;
mod settings;
mod startup;
mod timer;
mod transport;
mod workspace;

pub use machine::{Driver, Machine, Unhandled};

use crate::{
    cmd::{AudioCmd, Cmd, Cue, Effect, WindowColorsCmd},
    domain::{
        DriverName,
        Model,
        Moment,
        Startup,
        Toast,
        Workspace,
        playlist::{PlayOrder, Playlist},
    },
    message::{BrowseRequest, MacosEvent, Message, Timer},
};

#[must_use]
pub fn startup(startup: Startup) -> (Model, Vec<Effect>) {
    let mut model = Model::default();
    let (mut effects, messages) = startup::seed_model(&mut model, startup).into_parts();
    for queued in messages {
        if let Ok(drained) = drain(&mut model, (queued, 0), Moment::default()) {
            effects.extend(drained);
        }
    }
    effects.extend(roll_pending(&model.playlist));
    (model, effects)
}

const DRAIN_DEPTH: usize = 8;

pub fn update(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Vec<Effect>, Unhandled> {
    if let Message::Quit = message {
        return Ok(quit().into_parts().0);
    }
    if let Message::Viewport { visible_rows } = message {
        model.workspace.visible_rows = visible_rows;
        return Ok(Vec::new());
    }
    let message = if let Message::Key(press) = message {
        match keymap::route(&model.workspace, press) {
            Some(routed) => routed,
            None => return Err(Unhandled),
        }
    } else {
        message
    };
    let mut effects = update_model(model, message, now)?;
    effects.extend(roll_pending(&model.playlist));
    Ok(effects)
}

fn drain(
    model: &mut Model,
    (message, depth): (Message, usize),
    now: Moment,
) -> Result<Vec<Effect>, Unhandled> {
    let (mut effects, messages) = branch(model, message, now)?.into_parts();
    if depth >= DRAIN_DEPTH {
        debug_assert!(
            messages.is_empty(),
            "message chain deeper than {DRAIN_DEPTH}"
        );
        return Ok(effects);
    }
    for queued in messages {
        effects.extend(drain(model, (queued, depth + 1), now)?);
    }
    Ok(effects)
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
    Key,
    ChordPrefix,
    Event,
}

impl Input {
    fn of(message: &Message) -> Self {
        match message {
            Message::Browse(BrowseRequest::ChordPrefix(_)) => Input::ChordPrefix,
            Message::Macos(MacosEvent::MediaKey(_))
            | Message::Overlay(_)
            | Message::Step { .. }
            | Message::Playback(_)
            | Message::Browse(_)
            | Message::Queue(_) => Input::Key,
            Message::Macos(
                MacosEvent::Volume(_)
                | MacosEvent::OutputRouteChanged
                | MacosEvent::Error(_),
            )
            | Message::Toast(_)
            | Message::Playlist(_)
            | Message::ShuffleRolled(_)
            | Message::Library(_)
            | Message::Config(_)
            | Message::Audio(_)
            | Message::Elapsed(_)
            | Message::Driver { .. }
            | Message::DriverDied(_)
            | Message::Key(_)
            | Message::Viewport { .. }
            | Message::Quit => Input::Event,
        }
    }
}

fn roll_pending(playlist: &Playlist) -> Option<Effect> {
    match playlist.play_order {
        PlayOrder::ShufflePending => Some(Effect::RollShuffle(playlist.tracks.len())),
        PlayOrder::Linear | PlayOrder::Shuffle(_) => None,
    }
}

fn update_model(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Vec<Effect>, Unhandled> {
    let input = Input::of(&message);
    model.workspace.clock = now;
    let dismissed = dismissal(&mut model.workspace, input);
    let effects = match drain(model, (message, 0), now) {
        Ok(effects) => effects,
        Err(refusal) => {
            restore(&mut model.workspace, dismissed);
            return Err(refusal);
        }
    };
    released(&mut model.workspace, input);
    let cue = dismissed.map(|_| Effect::Animate(Cue::ToastDismissed));
    Ok(cue.into_iter().chain(effects).collect())
}

fn dismissal(workspace: &mut Workspace, input: Input) -> Option<Toast> {
    match input {
        Input::Key | Input::ChordPrefix => workspace.dismiss_newest(),
        Input::Event => None,
    }
}

fn restore(workspace: &mut Workspace, dismissed: Option<Toast>) {
    if let Some(toast) = dismissed {
        workspace.toasts.insert(0, toast);
    }
}

fn released(workspace: &mut Workspace, input: Input) {
    match input {
        Input::Key => workspace.chord_prefix = None,
        Input::ChordPrefix | Input::Event => {}
    }
}

fn config_parts(model: &mut Model) -> config::ConfigParts<'_> {
    let Model {
        workspace,
        revisions,
        settings,
        themes,
        appearance_settings,
        music_dir,
        ..
    } = model;
    config::ConfigParts {
        workspace,
        revisions,
        settings,
        themes,
        appearance_settings,
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
        ..
    } = model;
    browse::BrowseParts {
        playback: player::PlaybackParts {
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
    }
}

fn elapsed(model: &mut Model, timer: Timer, now: Moment) -> Result<Cmd, Unhandled> {
    match timer {
        Timer::Toast(revision) => Ok(timer::toast_expired(
            &mut model.workspace,
            revision,
            revision.freshness(model.revisions.toast),
        )),
        Timer::Sleep(revision) => {
            timer::sleep_fired(&mut playback_parts(model), revision, now)
        }
        Timer::Lookahead(revision) => {
            audio::lookahead_fired(&mut playback_parts(model), revision, now)
        }
    }
}

fn driver_died(model: &mut Model, driver: DriverName, now: Moment) -> Cmd {
    let parts = driver::DriverParts {
        drivers: &mut model.drivers,
        workspace: &mut model.workspace,
        revisions: &mut model.revisions,
    };
    match driver::decided(parts, driver, now) {
        driver::Restart::Declined(cmd) => cmd,
        driver::Restart::Granted => {
            let startup = startup::startup_cmd(model, driver);
            let resumed =
                driver::resumed(&model.player, &mut model.revisions, (driver, now));
            Cmd::from(Effect::Restart(driver))
                .then(startup)
                .then(resumed)
        }
    }
}

fn branch(model: &mut Model, message: Message, now: Moment) -> Result<Cmd, Unhandled> {
    match message {
        Message::Overlay(request) => overlay::update(
            overlay::OverlayParts {
                workspace: &mut model.workspace,
                playlist: &model.playlist,
                player: &model.player,
                history: &model.history,
                appearance_settings: &model.appearance_settings,
                music_dir: &model.music_dir,
            },
            request,
        ),
        Message::Step { row, direction } => {
            settings::step_setting(config_parts(model), row, direction)
        }
        Message::Toast(toast) => Ok(model.workspace.show(toast, &mut model.revisions)),
        Message::Playback(playback_request) => {
            playback::update(&mut playback_parts(model), playback_request, now)
        }
        Message::Browse(browse_request) => {
            browse::update(browse_parts(model), browse_request, now)
        }
        Message::Queue(queue_request) => browse::queue(
            browse::QueueParts {
                playlist: &model.playlist,
                browse: &mut model.workspace.browse,
                queue: &mut model.queue,
            },
            queue_request,
        ),
        Message::Playlist(loaded_request) => {
            library::update(playback_parts(model), loaded_request, now)
        }
        Message::ShuffleRolled(order) => model
            .playlist
            .transition(playlist::PlaylistMessage::ShuffleRolled(order)),
        Message::Library(event) => library::library(library_parts(model), event),
        Message::Config(event) => config::update(config_parts(model), event),
        Message::Audio(audio_event) => {
            audio::update(&mut playback_parts(model), audio_event, now)
        }
        Message::Macos(event) => macos::update(&mut playback_parts(model), event, now),
        Message::Elapsed(timer) => elapsed(model, timer, now),
        Message::Driver { driver, event } => {
            driver::update(&mut model.drivers, driver, event)
        }
        Message::DriverDied(driver) => Ok(driver_died(model, driver, now)),
        Message::Key(_) | Message::Viewport { .. } => Ok(Cmd::none()),
        Message::Quit => Ok(quit()),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use rstest::rstest;

    use crate::{
        cmd::Effect,
        domain::{Shuffle, Startup, Track},
        update::startup,
    };

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
    #[case::enabled_rolls(Shuffle::Enabled, Some(2))]
    #[case::disabled_rolls_nothing(Shuffle::Disabled, None)]
    fn startup_rolls_shuffle_only_when_enabled(
        #[case] shuffle: Shuffle,
        #[case] expected: Option<usize>,
    ) {
        let (_, cmd) = startup(startup_with(shuffle));

        assert_eq!(rolled_len(cmd), expected);
    }
}
