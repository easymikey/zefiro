mod audio;
mod browse;
mod config;
mod driver;
mod error;
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

pub use driver::DriverStatusError;
pub use error::UpdateError;
pub use machine::Machine;

use crate::{
    cmd::{AudioCmd, Cmd, Cue, Effect, WindowColorsCmd},
    domain::{
        Model,
        Moment,
        Startup,
        Toast,
        Workspace,
        playlist::{PlayOrder, Playlist},
    },
    message::{BrowseRequest, MacosEvent, Message, Timer},
};

pub fn startup(startup: Startup) -> (Model, Cmd) {
    let mut model = Model::default();
    let cmd = startup::seed_model(&mut model, startup);
    let cmd = cmd.then(roll_pending(&model.playlist));
    (model, cmd)
}

pub fn update(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    if let Message::Quit = message {
        return Ok(quit());
    }
    if let Message::Viewport { visible_rows } = message {
        model.workspace.visible_rows = visible_rows;
        return Ok(Cmd::None);
    }
    let message = if let Message::Key(press) = message {
        match keymap::route(&model.workspace, press) {
            Some(routed) => routed,
            None => return Ok(Cmd::None),
        }
    } else {
        message
    };
    let cmd = update_model(model, message, now)?;
    Ok(cmd.then(roll_pending(&model.playlist)))
}

pub(crate) fn quit() -> Cmd {
    Cmd::Batch(vec![
        Effect::Audio(AudioCmd::Stop),
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
            | Message::Adjust { .. }
            | Message::Playback(_)
            | Message::Browse(_)
            | Message::Queue(_) => Input::Key,
            Message::Macos(
                MacosEvent::Volume(_)
                | MacosEvent::OutputRouteChanged
                | MacosEvent::HardwareWatchError(_),
            )
            | Message::Toast(_)
            | Message::Playlist(_)
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

fn roll_pending(playlist: &Playlist) -> Cmd {
    match playlist.play_order {
        PlayOrder::ShufflePending => Effect::RollShuffle {
            len: playlist.tracks.len(),
        }
        .into(),
        PlayOrder::Linear | PlayOrder::Shuffle(_) => Cmd::None,
    }
}

fn update_model(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let input = Input::of(&message);
    model.workspace.clock = now;
    let dismissed = dismissal(&mut model.workspace, input);
    let cmd = match branch(model, message, now) {
        Ok(cmd) => cmd,
        Err(refusal) => {
            restore(&mut model.workspace, dismissed);
            return Err(refusal);
        }
    };
    released(&mut model.workspace, input);
    let cue = dismissed.map_or(Cmd::None, |_| Cue::ToastDismissed.into());
    Ok(cue.then(cmd))
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

fn elapsed(model: &mut Model, timer: Timer, now: Moment) -> Result<Cmd, UpdateError> {
    match timer {
        Timer::Toast(revision) => Ok(timer::toast_expired(
            &mut model.workspace,
            revision,
            revision.reply(model.revisions.toast),
        )),
        Timer::Sleep(revision) => {
            timer::sleep_fired(&mut playback_parts(model), revision, now)
        }
        Timer::Lookahead(revision) => {
            audio::mark_fired(&mut playback_parts(model), revision, now)
        }
    }
}

fn branch(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    match message {
        Message::Overlay(request) => overlay::update(model, request, now),
        Message::Adjust { row, direction } => {
            Ok(settings::adjust(config_parts(model), row, direction))
        }
        Message::Toast(toast) => Ok(model.workspace.show(toast, &mut model.revisions)),
        Message::Playback(playback_request) => {
            playback::update(&mut playback_parts(model), playback_request, now)
        }
        Message::Browse(browse_request) => browse::update(model, browse_request, now),
        Message::Queue(queue_request) => browse::queue(model, queue_request),
        Message::Playlist(loaded_request) => {
            library::update(model, loaded_request, now)
        }
        Message::ShuffleRolled(order) => Ok(model
            .playlist
            .apply(playlist::PlaylistMessage::ShuffleRolled(order))),
        Message::Library(event) => library::library(model, event),
        Message::Config(event) => config::update(config_parts(model), event),
        Message::Audio(audio_event) => {
            audio::update(&mut playback_parts(model), audio_event, now)
        }
        Message::Macos(event) => macos::update(&mut playback_parts(model), event, now),
        Message::Elapsed(timer) => elapsed(model, timer, now),
        Message::Driver { driver, event } => {
            Ok(match driver::update(&mut model.drivers, driver, event)? {
                Some(driver::DriverSignal::Died) => driver::decided(model, driver, now),
                Some(driver::DriverSignal::Full) => model.workspace.show(
                    Toast::info(format!("The {driver} driver is falling behind")),
                    &mut model.revisions,
                ),
                None => Cmd::None,
            })
        }
        Message::Key(_) | Message::Viewport { .. } => Ok(Cmd::None),
        Message::Quit => Ok(quit()),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use rstest::rstest;

    use crate::{
        cmd::{Cmd, Effect},
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

    fn rolled_len(cmd: Cmd) -> Option<usize> {
        cmd.into_iter().find_map(|effect| {
            if let Effect::RollShuffle { len } = effect {
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
