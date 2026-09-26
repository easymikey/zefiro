mod audio;
mod browse;
mod config;
mod driver;
pub mod keymap;
mod loaded;
mod machine;
pub mod overlay;
mod playback;
pub mod player;
mod playlist;
mod rejection;
mod settings;
mod startup;
mod system;
mod timer;
mod transport;
mod workspace;

pub use driver::DriverRejection;
pub use machine::{Machine, Never, Rejected};
pub use rejection::Rejection;

use crate::{
    cmd::{AudioCmd, Cmd, Cue, Effect, LibraryCmd, WindowColorsCmd},
    domain::{
        Model,
        Moment,
        Startup,
        UnixSeconds,
        Workspace,
        playlist::{PlayOrder, Playlist},
    },
    message::{BrowseRequest, Message, SystemEvent, Timer, WorkspaceRequest},
};

pub fn startup(startup: Startup) -> (Model, Cmd) {
    let mut model = Model::default();
    let mut cmd = startup::seed_model(&mut model, startup);
    stamp(&mut model, &mut cmd, Moment::default());
    let cmd = cmd.then(roll_pending(&model.playlist));
    (model, cmd)
}

pub fn update(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Cmd, Rejection> {
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
    let mut cmd = update_model(model, message, now)?;
    stamp(model, &mut cmd, now);
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
            Message::System(SystemEvent::MediaKey(_))
            | Message::Overlay(_)
            | Message::Adjust { .. }
            | Message::Playback(_)
            | Message::Browse(_) => Input::Key,
            Message::System(
                SystemEvent::Volume(_) | SystemEvent::OutputRouteChanged,
            )
            | Message::Workspace(_)
            | Message::Loaded(_)
            | Message::Library(_)
            | Message::Config(_)
            | Message::Audio(_)
            | Message::Elapsed(_)
            | Message::Driver(..)
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

fn stamp(model: &mut Model, cmd: &mut Cmd, now: Moment) {
    for effect in cmd.effects_mut() {
        match effect {
            Effect::Library(
                LibraryCmd::Rescan { revision: slot, .. }
                | LibraryCmd::ScanLibrary { revision: slot, .. },
            ) => {
                let issued = model.effects.bump();
                *slot = issued;
                model.scan_generation = issued;
            }
            Effect::Library(LibraryCmd::AppendHistory { at, .. }) => {
                *at = UnixSeconds::of(now);
            }
            Effect::Audio(
                AudioCmd::Load { revision: slot, .. }
                | AudioCmd::Preload { revision: slot, .. },
            ) => {
                *slot = model.effects.bump();
            }
            Effect::After { message, .. } => stamp_timer(model, message),
            Effect::Restart(_)
            | Effect::Audio(
                AudioCmd::Pause(_)
                | AudioCmd::Seek(_)
                | AudioCmd::Volume(_)
                | AudioCmd::SetSpeed(_)
                | AudioCmd::Stop
                | AudioCmd::SetCrossfade(_)
                | AudioCmd::SetReplaygain(_)
                | AudioCmd::SetDevice(_)
                | AudioCmd::ListDevices,
            )
            | Effect::Library(
                LibraryCmd::SaveFavorites(_)
                | LibraryCmd::LoadFavorites
                | LibraryCmd::Trash(_)
                | LibraryCmd::LoadHistory { .. }
                | LibraryCmd::SavePlaylist { .. }
                | LibraryCmd::TagTracks { .. }
                | LibraryCmd::PrefetchCover(_),
            )
            | Effect::System(_)
            | Effect::Config(_)
            | Effect::Animate(_)
            | Effect::RollShuffle { .. }
            | Effect::WindowColors(_)
            | Effect::Setting { .. }
            | Effect::Quit => {}
        }
    }
}

fn stamp_timer(model: &mut Model, timer: &mut Timer) {
    let (slot, generation) = match timer {
        Timer::Toast(slot) => (slot, &mut model.toast_generation),
        Timer::Sleep(slot) => (slot, &mut model.sleep_generation),
        Timer::Mark(slot) => (slot, &mut model.mark_generation),
        Timer::Restart(_) => return,
    };
    let issued = model.effects.bump();
    *slot = issued;
    *generation = issued;
}

fn update_model(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Cmd, Rejection> {
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

fn branch(model: &mut Model, message: Message, now: Moment) -> Result<Cmd, Rejection> {
    match message {
        Message::Overlay(request) => overlay::update(model, request, now),
        Message::Adjust { row, nudge } => settings::adjust(model, row, nudge),
        Message::Workspace(workspace_request) => workspace(model, workspace_request),
        Message::Playback(playback_request) => {
            playback::playback(model, playback_request, now)
        }
        Message::Browse(browse_request) => browse::update(model, browse_request),
        Message::Loaded(loaded_request) => loaded::loaded(model, loaded_request),
        Message::Library(fact) => loaded::library(model, fact),
        Message::Config(fact) => config::config(model, fact),
        Message::Audio(audio_event) => audio::audio(model, audio_event, now),
        Message::System(event) => system::system(model, event, now),
        Message::Elapsed(timer) => timer::update(model, timer, now),
        Message::Driver(driver, driver_message) => {
            driver::update(model, (driver, driver_message), now)
        }
        Message::Key(_) | Message::Viewport { .. } => Ok(Cmd::None),
        Message::Quit => Ok(quit()),
    }
}

fn workspace(model: &mut Model, request: WorkspaceRequest) -> Result<Cmd, Rejection> {
    Ok(model.workspace.update(request)?)
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc, time::Duration};

    use rstest::rstest;

    use crate::{
        cmd::{Cmd, Effect, LibraryCmd},
        domain::{Moment, Shuffle, Startup, Track, UnixSeconds},
        update::{stamp, startup},
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

    #[test]
    fn stamp_sets_the_append_history_timestamp_from_now() {
        let (mut model, _) = startup(Startup::default());
        let track = Arc::new(Track::listed(Path::new("/music/a.flac")));
        let mut cmd = Cmd::One(Effect::Library(LibraryCmd::AppendHistory {
            track,
            at: UnixSeconds::UNSTAMPED,
        }));
        let now = Moment::new(Duration::from_secs(9));

        stamp(&mut model, &mut cmd, now);

        let at = cmd.effects().find_map(|effect| {
            if let Effect::Library(LibraryCmd::AppendHistory { at, .. }) = effect {
                Some(*at)
            } else {
                None
            }
        });
        assert_eq!(at, Some(UnixSeconds::of(now)));
    }
}
