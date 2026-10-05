use ratatui::style::Color;

use crate::theme::active_theme::ActiveTheme;

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
        let colors = theme.colors();
        let accent = colors.accent;
        Self {
            background: colors.window_background,
            accent,
            volume_fill: accent,
            volume_lifted: theme.lifted(theme.colors.accent, theme.volume_pulse_mix),
        }
    }
}
