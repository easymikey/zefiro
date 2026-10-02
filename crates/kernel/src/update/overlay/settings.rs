use crate::{
    Cmd,
    domain::{
        AppearanceSetting,
        Direction,
        Model,
        Moment,
        Overlay,
        SettingRow,
        Workspace,
    },
    message::SettingsRowRequest,
    update::{
        error::UpdateError,
        machine::Machine,
        overlay::{FollowUp, InnerMessage, OverlayMessage, OverlayOutcome, follow},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCursorMessage {
    Navigate(SettingRow),
    Adjust(Direction),
    Noop,
}

pub(crate) fn transition(
    selected: &mut SettingRow,
    message: SettingsCursorMessage,
) -> OverlayOutcome {
    match message {
        SettingsCursorMessage::Navigate(row) => {
            *selected = row;
            OverlayOutcome::default()
        }
        SettingsCursorMessage::Adjust(direction) => {
            OverlayOutcome::from(FollowUp::Adjust {
                row: *selected,
                direction,
            })
        }
        SettingsCursorMessage::Noop => OverlayOutcome::default(),
    }
}

pub(crate) fn request(
    model: &mut Model,
    request: SettingsRowRequest,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let Model {
        workspace,
        appearance_settings,
        ..
    } = &mut *model;
    let message = resolve(workspace, appearance_settings, request);
    let effect = workspace
        .overlay
        .transition(OverlayMessage::Inner(InnerMessage::Settings(message)))?;
    follow(model, effect, now)
}

fn resolve(
    workspace: &Workspace,
    appearance_settings: &[AppearanceSetting],
    request: SettingsRowRequest,
) -> SettingsCursorMessage {
    match request {
        SettingsRowRequest::Navigate(direction) => {
            navigate_target(workspace, appearance_settings, direction)
        }
        SettingsRowRequest::Adjust(direction) => {
            SettingsCursorMessage::Adjust(direction)
        }
        SettingsRowRequest::Activate => activate(workspace, appearance_settings),
    }
}

fn navigate_target(
    workspace: &Workspace,
    appearance_settings: &[AppearanceSetting],
    direction: Direction,
) -> SettingsCursorMessage {
    let Some(Overlay::Settings { selected }) = &workspace.overlay else {
        return SettingsCursorMessage::Noop;
    };
    let rows = SettingRow::all(appearance_settings);
    SettingsCursorMessage::Navigate(selected.moved(&rows, direction))
}

fn activate(
    workspace: &Workspace,
    appearance_settings: &[AppearanceSetting],
) -> SettingsCursorMessage {
    let Some(Overlay::Settings { selected }) = &workspace.overlay else {
        return SettingsCursorMessage::Noop;
    };
    if selected.activates(appearance_settings) {
        SettingsCursorMessage::Adjust(Direction::Next)
    } else {
        SettingsCursorMessage::Noop
    }
}
