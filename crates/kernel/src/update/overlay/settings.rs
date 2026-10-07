use crate::{
    cmd::Cmd,
    domain::{
        direction::Direction,
        overlay::Overlay,
        setting_row::SettingRow,
        workspace::Workspace,
    },
    message::{Message, SettingRowRequest},
    update::machine::{Machine, Unhandled, replace},
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
            SettingRowMessage::Set(row) => replace(self, row).map(|()| Cmd::none()),
            SettingRowMessage::Step(direction) => Ok(Cmd::message(Message::Step {
                row: *self,
                direction,
            })),
        }
    }
}

pub(crate) fn setting_row_message(
    workspace: &Workspace,
    request: SettingRowRequest,
) -> Result<SettingRowMessage, Unhandled> {
    match request {
        SettingRowRequest::Navigate(direction) => navigate_target(workspace, direction),
        SettingRowRequest::Step(direction) => Ok(SettingRowMessage::Step(direction)),
        SettingRowRequest::Activate => activate(workspace),
    }
}

fn navigate_target(
    workspace: &Workspace,
    direction: Direction,
) -> Result<SettingRowMessage, Unhandled> {
    let Some(Overlay::Settings(selected)) = &workspace.overlay else {
        return Err(Unhandled);
    };
    Ok(SettingRowMessage::Set(
        selected.moved(&SettingRow::ALL, direction),
    ))
}

fn activate(workspace: &Workspace) -> Result<SettingRowMessage, Unhandled> {
    let Some(Overlay::Settings(selected)) = &workspace.overlay else {
        return Err(Unhandled);
    };
    if selected.activates() {
        Ok(SettingRowMessage::Step(Direction::Next))
    } else {
        Err(Unhandled)
    }
}
