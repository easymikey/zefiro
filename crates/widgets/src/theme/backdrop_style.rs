use ratatui::style::Color;

use crate::{
    animation::timings::TIMINGS,
    theme::{active_theme::ActiveTheme, rgb::lerp_rgb},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackdropStyle {
    pub background: Color,
    pub accent: Color,
    pub volume_lifted: Color,
}

impl BackdropStyle {
    #[must_use]
    pub fn from_theme(theme: &ActiveTheme<'_>) -> Self {
        let colors = theme.colors();
        Self {
            background: colors.window_background,
            accent: colors.accent,
            volume_lifted: theme.color(lerp_rgb(
                theme.colors.accent,
                theme.colors.foreground,
                TIMINGS.volume_pulse_mix,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        animation::timings::TIMINGS,
        test_support::noir,
        theme::{
            active_theme::ActiveTheme,
            backdrop_style::BackdropStyle,
            rgb::{ColorDepth, color_at_depth, lerp_rgb},
        },
    };

    #[test]
    fn from_theme_lifts_the_accent_toward_the_text_by_the_volume_pulse_mix() {
        let theme = noir();
        let active_theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let colors = active_theme.colors();
        let style = BackdropStyle::from_theme(&active_theme);
        assert_eq!(style.background, colors.window_background);
        assert_eq!(style.accent, colors.accent);
        assert_eq!(
            style.volume_lifted,
            color_at_depth(
                lerp_rgb(
                    theme.colors.accent,
                    theme.colors.foreground,
                    TIMINGS.volume_pulse_mix,
                ),
                ColorDepth::TrueColor,
            )
        );
        assert_ne!(style.volume_lifted, style.accent);
    }
}
