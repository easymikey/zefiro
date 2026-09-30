use raster::{BarColorOverrides, BarColors};
use ratatui::style::Color;

use crate::theme::{Role, Theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillColors {
    pub accent: Color,
    pub dim: Color,
}

#[must_use]
pub fn bar_colors(overrides: BarColorOverrides, theme: &Theme) -> BarColors {
    BarColors {
        fill: overrides.fill.unwrap_or(theme.colors.role(Role::Accent)),
        trough: overrides
            .track
            .unwrap_or(theme.colors.role(Role::BarGroove)),
    }
}

#[cfg(test)]
mod tests {
    use config::Rgb;
    use raster::{BarColorOverrides, BarColors, color_overrides};

    use crate::theme::{Role, Theme, bars::bar_colors};

    fn theme() -> Theme {
        let file =
            config::parse_theme(include_str!("../../../../themes/noir.toml"), "noir")
                .unwrap();
        Theme::from(file)
    }

    #[test]
    fn an_unset_override_is_the_themes_own_accent_and_derived_track() {
        let theme = theme();
        assert_eq!(
            bar_colors(BarColorOverrides::default(), &theme),
            BarColors {
                fill: theme.colors.role(Role::Accent),
                trough: theme.colors.role(Role::BarGroove),
            }
        );
    }

    #[test]
    fn a_set_override_wins_over_the_theme() {
        let theme = theme();
        let overrides = BarColorOverrides {
            fill: Some(Rgb([255, 0, 0])),
            track: Some(Rgb([0, 255, 0])),
        };
        assert_eq!(
            bar_colors(overrides, &theme),
            BarColors {
                fill: Rgb([255, 0, 0]),
                trough: Rgb([0, 255, 0]),
            }
        );
    }

    #[test]
    fn the_bar_colours_follow_the_progress_configs_overrides() {
        let theme = theme();
        let overridden = config::ProgressConfig {
            fill: Some(Rgb([255, 0, 0])),
            track: Some(Rgb([0, 255, 0])),
            ..config::ProgressConfig::default()
        };
        assert_eq!(
            bar_colors(color_overrides(&overridden), &theme),
            BarColors {
                fill: Rgb([255, 0, 0]),
                trough: Rgb([0, 255, 0]),
            }
        );
    }
}
