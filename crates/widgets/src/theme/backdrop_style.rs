use ratatui::style::Color;

use crate::theme::{active_theme::ActiveTheme, colors::Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackdropStyle {
    pub background: Color,
    pub accent: Color,
    pub volume_fill: Color,
    pub volume_lifted: Color,
}

impl BackdropStyle {
    #[must_use]
    pub fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        let accent = theme.role(Role::Accent);
        Self {
            background: theme.role(Role::WindowBackground),
            accent,
            volume_fill: accent,
            volume_lifted: theme
                .lifted(theme.colors.role(Role::Accent), theme.volume_pulse_mix),
        }
    }
}
