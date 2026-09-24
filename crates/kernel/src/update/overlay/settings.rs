use crate::{
    Cmd,
    domain::{
        CursorOver,
        ListMotion,
        Model,
        Nudge,
        SettingControl,
        SettingRow,
        SettingsRows,
    },
    message::SettingsRowRequest,
    update::{
        machine::{Machine, Rejected},
        overlay::{
            FollowUp,
            InnerMessage,
            OverlayEffect,
            OverlayMessage,
            follow,
            selected_setting_row,
        },
        rejection::Rejection,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRejection {
    NothingSelected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsMessage {
    Navigate {
        nudge: Nudge,
        len: usize,
    },
    Adjust {
        row: Option<SettingRow>,
        nudge: Nudge,
    },
    Noop,
}

type Transition = Result<
    (CursorOver<SettingsRows>, OverlayEffect),
    Rejected<CursorOver<SettingsRows>>,
>;

impl Machine for CursorOver<SettingsRows> {
    type Message = SettingsMessage;
    type Rejection = SettingsRejection;
    type Effect = OverlayEffect;

    fn transition(mut self, message: SettingsMessage) -> Transition {
        match message {
            SettingsMessage::Navigate { nudge, len } => {
                self.resize(len);
                self.navigate(ListMotion::from(nudge));
                Ok((self, OverlayEffect::default()))
            }
            SettingsMessage::Adjust {
                row: Some(row),
                nudge,
            } => Ok((self, OverlayEffect::from(FollowUp::Adjust { row, nudge }))),
            SettingsMessage::Adjust { row: None, .. } => Err(Rejected {
                state: self,
                reason: SettingsRejection::NothingSelected,
            }),
            SettingsMessage::Noop => Ok((self, OverlayEffect::default())),
        }
    }
}

pub(super) fn request(
    model: &mut Model,
    request: SettingsRowRequest,
) -> Result<Cmd, Rejection> {
    let message = resolve(model, request);
    let effect = model
        .workspace
        .overlay
        .update(OverlayMessage::Inner(InnerMessage::Settings(message)))?;
    follow(model, effect)
}

fn resolve(model: &Model, request: SettingsRowRequest) -> SettingsMessage {
    match request {
        SettingsRowRequest::Navigate(nudge) => SettingsMessage::Navigate {
            nudge,
            len: SettingRow::all(&model.custom_rows).len(),
        },
        SettingsRowRequest::Adjust(nudge) => SettingsMessage::Adjust {
            row: selected_setting_row(model),
            nudge,
        },
        SettingsRowRequest::Activate => activate(model),
    }
}

fn activate(model: &Model) -> SettingsMessage {
    match selected_setting_row(model) {
        Some(row) if activates(row, model) => SettingsMessage::Adjust {
            row: Some(row),
            nudge: Nudge::Up,
        },
        Some(_) | None => SettingsMessage::Noop,
    }
}

fn activates(row: SettingRow, model: &Model) -> bool {
    matches!(
        row.control(&model.custom_rows),
        SettingControl::Toggle | SettingControl::Cycle(_)
    )
}
