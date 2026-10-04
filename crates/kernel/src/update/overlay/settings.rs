use crate::{
    Cmd,
    domain::{AppearanceSetting, Direction, Overlay, SettingRow, Workspace},
    message::{Message, SettingsRowRequest},
    update::{
        machine::{Machine, Unhandled},
        overlay::{InnerMessage, OverlayMessage},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRowMessage {
    Navigate(SettingRow),
    Step(Direction),
}

impl Machine for SettingRow {
    type Message = SettingRowMessage;
    type Effect = Cmd;

    fn transition(&mut self, message: SettingRowMessage) -> Result<Cmd, Unhandled> {
        match message {
            SettingRowMessage::Navigate(row) => {
                *self = row;
                Ok(Cmd::none())
            }
            SettingRowMessage::Step(direction) => Ok(Cmd::message(Message::Step {
                row: *self,
                direction,
            })),
        }
    }
}

pub(crate) fn request(
    workspace: &mut Workspace,
    appearance_settings: &[AppearanceSetting],
    request: SettingsRowRequest,
) -> Result<Cmd, Unhandled> {
    let message = resolve(workspace, appearance_settings, request)?;
    workspace
        .overlay
        .transition(OverlayMessage::Inner(InnerMessage::Settings(message)))
}

fn resolve(
    workspace: &Workspace,
    appearance_settings: &[AppearanceSetting],
    request: SettingsRowRequest,
) -> Result<SettingRowMessage, Unhandled> {
    match request {
        SettingsRowRequest::Navigate(direction) => {
            navigate_target(workspace, appearance_settings, direction)
        }
        SettingsRowRequest::Step(direction) => Ok(SettingRowMessage::Step(direction)),
        SettingsRowRequest::Activate => activate(workspace, appearance_settings),
    }
}

fn navigate_target(
    workspace: &Workspace,
    appearance_settings: &[AppearanceSetting],
    direction: Direction,
) -> Result<SettingRowMessage, Unhandled> {
    let Some(Overlay::Settings(selected)) = &workspace.overlay else {
        return Err(Unhandled);
    };
    let rows = SettingRow::all(appearance_settings);
    Ok(SettingRowMessage::Navigate(
        selected.moved(&rows, direction),
    ))
}

fn activate(
    workspace: &Workspace,
    appearance_settings: &[AppearanceSetting],
) -> Result<SettingRowMessage, Unhandled> {
    let Some(Overlay::Settings(selected)) = &workspace.overlay else {
        return Err(Unhandled);
    };
    if selected.activates(appearance_settings) {
        Ok(SettingRowMessage::Step(Direction::Next))
    } else {
        Err(Unhandled)
    }
}
