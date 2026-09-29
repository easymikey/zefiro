use crate::{
    Cmd,
    domain::{
        Model,
        Moment,
        Nudge,
        Overlay,
        SettingControl,
        SettingRow,
        SettingsCursor,
    },
    message::SettingsRowRequest,
    update::{
        machine::{Machine, Never, Rejected},
        overlay::{FollowUp, InnerMessage, OverlayEffect, OverlayMessage, follow},
        rejection::Rejection,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMessage {
    Navigate(SettingRow),
    Adjust(Nudge),
    Noop,
}

impl Machine for SettingsCursor {
    type Message = SettingsMessage;
    type Rejection = Never;
    type Effect = OverlayEffect;

    fn transition(
        self,
        message: SettingsMessage,
    ) -> Result<(Self, OverlayEffect), Rejected<Self>> {
        match message {
            SettingsMessage::Navigate(selected) => {
                Ok((SettingsCursor { selected }, OverlayEffect::default()))
            }
            SettingsMessage::Adjust(nudge) => {
                let row = self.selected;
                Ok((self, OverlayEffect::from(FollowUp::Adjust { row, nudge })))
            }
            SettingsMessage::Noop => Ok((self, OverlayEffect::default())),
        }
    }
}

pub(crate) fn request(
    model: &mut Model,
    request: SettingsRowRequest,
    now: Moment,
) -> Result<Cmd, Rejection> {
    let message = resolve(model, request);
    let effect = model
        .workspace
        .overlay
        .update(OverlayMessage::Inner(InnerMessage::Settings(message)))?;
    follow(model, effect, now)
}

fn resolve(model: &Model, request: SettingsRowRequest) -> SettingsMessage {
    match request {
        SettingsRowRequest::Navigate(nudge) => navigate_target(model, nudge),
        SettingsRowRequest::Adjust(nudge) => SettingsMessage::Adjust(nudge),
        SettingsRowRequest::Activate => activate(model),
    }
}

fn navigate_target(model: &Model, nudge: Nudge) -> SettingsMessage {
    let Some(Overlay::Settings(cursor)) = &model.workspace.overlay else {
        return SettingsMessage::Noop;
    };
    let rows = SettingRow::all(&model.custom_rows);
    SettingsMessage::Navigate(cursor.moved(&rows, nudge).selected)
}

fn activate(model: &Model) -> SettingsMessage {
    let Some(Overlay::Settings(cursor)) = &model.workspace.overlay else {
        return SettingsMessage::Noop;
    };
    if activates(cursor.selected, model) {
        SettingsMessage::Adjust(Nudge::Up)
    } else {
        SettingsMessage::Noop
    }
}

fn activates(row: SettingRow, model: &Model) -> bool {
    matches!(
        row.control(&model.custom_rows),
        Some(SettingControl::Toggle | SettingControl::Ring | SettingControl::Custom(_))
    )
}
