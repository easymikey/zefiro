use crate::{
    cmd::Cmd,
    domain::{
        direction::Direction,
        overlay::Overlay,
        setting_row::{AppearanceSetting, SettingRow},
        workspace::Workspace,
    },
    message::{Message, SettingsRowRequest},
    update::machine::{Machine, Unhandled},
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

pub(crate) fn resolve(
    workspace: &Workspace,
    appearance_rows: &[AppearanceSetting],
    request: SettingsRowRequest,
) -> Result<SettingRowMessage, Unhandled> {
    match request {
        SettingsRowRequest::Navigate(direction) => {
            navigate_target(workspace, appearance_rows, direction)
        }
        SettingsRowRequest::Step(direction) => Ok(SettingRowMessage::Step(direction)),
        SettingsRowRequest::Activate => activate(workspace, appearance_rows),
    }
}

fn navigate_target(
    workspace: &Workspace,
    appearance_rows: &[AppearanceSetting],
    direction: Direction,
) -> Result<SettingRowMessage, Unhandled> {
    let Some(Overlay::Settings(selected)) = &workspace.overlay else {
        return Err(Unhandled);
    };
    let rows = SettingRow::all(appearance_rows);
    Ok(SettingRowMessage::Navigate(
        selected.moved(&rows, direction),
    ))
}

fn activate(
    workspace: &Workspace,
    appearance_rows: &[AppearanceSetting],
) -> Result<SettingRowMessage, Unhandled> {
    let Some(Overlay::Settings(selected)) = &workspace.overlay else {
        return Err(Unhandled);
    };
    if selected.activates(appearance_rows) {
        Ok(SettingRowMessage::Step(Direction::Next))
    } else {
        Err(Unhandled)
    }
}
