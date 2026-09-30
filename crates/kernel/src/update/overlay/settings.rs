use crate::{
    Cmd,
    domain::{Direction, Model, Moment, Overlay, SettingControl, SettingRow},
    message::SettingsRowRequest,
    update::{
        error::UpdateError,
        machine::Machine,
        overlay::{FollowUp, InnerMessage, OverlayEffect, OverlayMessage, follow},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCursorMessage {
    Navigate(SettingRow),
    Adjust(Direction),
    Noop,
}

pub(crate) fn transition(
    current: SettingRow,
    message: SettingsCursorMessage,
) -> (Option<Overlay>, OverlayEffect) {
    match message {
        SettingsCursorMessage::Navigate(selected) => (
            Some(Overlay::Settings { selected }),
            OverlayEffect::default(),
        ),
        SettingsCursorMessage::Adjust(direction) => (
            Some(Overlay::Settings { selected: current }),
            OverlayEffect::from(FollowUp::Adjust {
                row: current,
                direction,
            }),
        ),
        SettingsCursorMessage::Noop => (
            Some(Overlay::Settings { selected: current }),
            OverlayEffect::default(),
        ),
    }
}

pub(crate) fn request(
    model: &mut Model,
    request: SettingsRowRequest,
    now: Moment,
) -> Result<Cmd, UpdateError> {
    let message = resolve(model, request);
    let effect = model
        .workspace
        .overlay
        .update(OverlayMessage::Inner(InnerMessage::Settings(message)))?;
    follow(model, effect, now)
}

fn resolve(model: &Model, request: SettingsRowRequest) -> SettingsCursorMessage {
    match request {
        SettingsRowRequest::Navigate(direction) => navigate_target(model, direction),
        SettingsRowRequest::Adjust(direction) => {
            SettingsCursorMessage::Adjust(direction)
        }
        SettingsRowRequest::Activate => activate(model),
    }
}

fn navigate_target(model: &Model, direction: Direction) -> SettingsCursorMessage {
    let Some(Overlay::Settings { selected }) = &model.workspace.overlay else {
        return SettingsCursorMessage::Noop;
    };
    let rows = SettingRow::all(&model.custom_settings);
    SettingsCursorMessage::Navigate(selected.moved(&rows, direction))
}

fn activate(model: &Model) -> SettingsCursorMessage {
    let Some(Overlay::Settings { selected }) = &model.workspace.overlay else {
        return SettingsCursorMessage::Noop;
    };
    if activates(*selected, model) {
        SettingsCursorMessage::Adjust(Direction::Next)
    } else {
        SettingsCursorMessage::Noop
    }
}

fn activates(row: SettingRow, model: &Model) -> bool {
    matches!(
        row.control(&model.custom_settings),
        Some(SettingControl::Toggle | SettingControl::Ring | SettingControl::Custom(_))
    )
}
