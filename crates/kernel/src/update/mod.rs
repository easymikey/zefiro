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
pub use machine::{Machine, Rejected};

use crate::{
    cmd::{AudioCmd, Cmd, Cue, Effect, WindowColorsCmd},
    domain::{
        Model,
        Moment,
        Startup,
        Workspace,
        playlist::{PlayOrder, Playlist},
    },
    message::{BrowseRequest, MacosEvent, Message},
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
    let dismissed = dismissal(&model.workspace, input);
    let cmd = branch(model, message, now)?;
    released(&mut model.workspace, input, &cmd);
    Ok(dismissed.then(cmd))
}

fn dismissal(workspace: &Workspace, input: Input) -> Cmd {
    match (input, &workspace.toast) {
        (Input::Key | Input::ChordPrefix, Some(_)) => Cue::ToastDismissed.into(),
        (Input::Key | Input::ChordPrefix, None) | (Input::Event, Some(_) | None) => {
            Cmd::None
        }
    }
}

fn released(workspace: &mut Workspace, input: Input, cmd: &Cmd) {
    match input {
        Input::Event => return,
        Input::Key => workspace.chord = None,
        Input::ChordPrefix => {}
    }
    let raised = cmd
        .effects()
        .any(|effect| matches!(effect, Effect::Animate(Cue::ToastRaised)));
    if !raised {
        workspace.toast = None;
    }
}

fn branch(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    match message {
        Message::Overlay(request) => overlay::update(model, request, now),
        Message::Adjust { row, direction } => settings::adjust(model, row, direction),
        Message::Toast(toast) => Ok(model.workspace.show(toast, &mut model.revisions)),
        Message::Playback(playback_request) => {
            playback::update(model, playback_request, now)
        }
        Message::Browse(browse_request) => browse::update(model, browse_request, now),
        Message::Queue(queue_request) => browse::queue(model, queue_request),
        Message::Playlist(loaded_request) => {
            library::update(model, loaded_request, now)
        }
        Message::Library(event) => library::library(model, event),
        Message::Config(event) => config::update(model, event),
        Message::Audio(audio_event) => audio::update(model, audio_event, now),
        Message::Macos(event) => macos::update(model, event, now),
        Message::Elapsed(timer) => timer::update(model, timer, now),
        Message::Driver { driver, event } => {
            Ok(match driver::update(model, driver, event)? {
                Some(driver::DriverSignal::Died(failure)) => {
                    driver::decided(model, driver::Died { driver, failure }, now)
                }
                Some(driver::DriverSignal::Congested) => {
                    driver::inbox_full(model, driver)
                }
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
