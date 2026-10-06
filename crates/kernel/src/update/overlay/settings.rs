use crate::{
    cmd::Cmd,
    domain::{
        direction::Direction,
        overlay::Overlay,
        setting_row::{AppearanceRowChoice, SettingRow},
        workspace::Workspace,
    },
    message::{Message, SettingRowRequest},
    update::machine::{Machine, Unhandled},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingRowMessage {
    Set(SettingRow),
    Step(Direction),
}

impl Machine for SettingRow {
    type Message = SettingRowMessage;
    type Effect = Cmd;

    fn transition(
        &mut self,
        setting_row_message: SettingRowMessage,
    ) -> Result<Cmd, Unhandled> {
        match setting_row_message {
            SettingRowMessage::Set(row) => {
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

pub(crate) fn setting_row_message(
    workspace: &Workspace,
    appearance_row_choices: &[AppearanceRowChoice],
    request: SettingRowRequest,
) -> Result<SettingRowMessage, Unhandled> {
    match request {
        SettingRowRequest::Navigate(direction) => {
            navigate_target(workspace, appearance_row_choices, direction)
        }
        SettingRowRequest::Step(direction) => Ok(SettingRowMessage::Step(direction)),
        SettingRowRequest::Activate => activate(workspace, appearance_row_choices),
    }
}

fn navigate_target(
    workspace: &Workspace,
    appearance_row_choices: &[AppearanceRowChoice],
    direction: Direction,
) -> Result<SettingRowMessage, Unhandled> {
    let Some(Overlay::Settings(selected)) = &workspace.overlay else {
        return Err(Unhandled);
    };
    let setting_row = SettingRow::all(appearance_row_choices);
    Ok(SettingRowMessage::Set(
        selected.moved(&setting_row, direction),
    ))
}

fn activate(
    workspace: &Workspace,
    appearance_row_choices: &[AppearanceRowChoice],
) -> Result<SettingRowMessage, Unhandled> {
    let Some(Overlay::Settings(selected)) = &workspace.overlay else {
        return Err(Unhandled);
    };
    if selected.activates(appearance_row_choices) {
        Ok(SettingRowMessage::Step(Direction::Next))
    } else {
        Err(Unhandled)
    }
}
