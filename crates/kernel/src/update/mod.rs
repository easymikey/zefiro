mod audio;
mod browse;
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
        Startup,
        Workspace,
        playlist::{PlayOrder, Playlist},
    },
    message::{BrowseRequest, Message, Timer, WorkspaceRequest},
    update::{transport::TransportMessage, workspace::KeymapReload},
};

pub fn startup(startup: Startup) -> (Model, Cmd) {
    let mut model = Model::default();
    let mut cmd = startup::seed_model(&mut model, startup);
    stamp(&mut model, &mut cmd);
    let cmd = cmd.then(roll_pending(&model.playlist));
    (model, cmd)
}

pub fn update(model: &mut Model, message: Message) -> Result<Cmd, Rejection> {
    if let Message::Quit = message {
        return Ok(quit());
    }
    let mut cmd = update_model(model, message)?;
    stamp(model, &mut cmd);
    Ok(cmd.then(roll_pending(&model.playlist)))
}

fn quit() -> Cmd {
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
            Message::Overlay(_)
            | Message::Adjust { .. }
            | Message::Playback(_)
            | Message::Browse(_) => Input::Key,
            Message::Workspace(_)
            | Message::Loaded(_)
            | Message::Audio(_)
            | Message::SystemVolume(_)
            | Message::Elapsed(_)
            | Message::Driver(..)
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

fn stamp(model: &mut Model, cmd: &mut Cmd) {
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
            Effect::Library(LibraryCmd::AppendHistory { revision: slot, .. })
            | Effect::Audio(
                AudioCmd::Load { revision: slot, .. }
                | AudioCmd::Preload { revision: slot, .. },
            ) => {
                *slot = model.effects.bump();
            }
            Effect::After { message, .. } => stamp_timer(model, message),
            Effect::Audio(
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
    let issued = model.effects.bump();
    let (slot, generation) = match timer {
        Timer::Toast(slot) => (slot, &mut model.toast_generation),
        Timer::Sleep(slot) => (slot, &mut model.sleep_generation),
    };
    *slot = issued;
    *generation = issued;
}

fn update_model(model: &mut Model, message: Message) -> Result<Cmd, Rejection> {
    let input = Input::of(&message);
    let dismissed = dismissal(&model.workspace, input);
    let cmd = branch(model, message)?;
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

fn branch(model: &mut Model, message: Message) -> Result<Cmd, Rejection> {
    match message {
        Message::Overlay(request) => overlay::update(model, request),
        Message::Adjust { row, nudge } => settings::adjust(model, row, nudge),
        Message::Workspace(workspace_request) => workspace(model, workspace_request),
        Message::Playback(playback_request) => {
            playback::playback(model, playback_request)
        }
        Message::Browse(browse_request) => browse::update(model, browse_request),
        Message::Loaded(loaded_request) => loaded::loaded(model, loaded_request),
        Message::Audio(audio_event) => audio::audio(model, audio_event),
        Message::SystemVolume(volume) => Ok(model
            .transport
            .update(TransportMessage::SetVolume(volume))?),
        Message::Elapsed(timer) => timer::update(model, timer),
        Message::Driver(driver, driver_message) => {
            driver::update(model, driver, driver_message)
        }
        Message::Quit => Ok(quit()),
    }
}

fn workspace(model: &mut Model, request: WorkspaceRequest) -> Result<Cmd, Rejection> {
    let reload = match &request {
        WorkspaceRequest::KeymapReloaded(keys) => model.workspace.keymap_reload(keys),
        WorkspaceRequest::ShowToast(_)
        | WorkspaceRequest::ClearToast
        | WorkspaceRequest::ThemeReloaded
        | WorkspaceRequest::SourceFailed { .. }
        | WorkspaceRequest::SourceRecovered(_)
        | WorkspaceRequest::ConfigFailed(_) => KeymapReload::Unchanged,
    };
    let themed = matches!(request, WorkspaceRequest::ThemeReloaded);
    let cmd = model.workspace.update(request)?;
    if let KeymapReload::Fresh = reload {
        let _ = model.config_generation.bump();
    }
    if !themed {
        return Ok(cmd);
    }
    let _ = model.theme_generation.bump();
    Ok(cmd.then(Cue::ThemeChanged.into()))
}
