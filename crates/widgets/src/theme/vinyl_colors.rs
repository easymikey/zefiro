use raster::VinylColors;

use crate::theme::{Role, Theme, shade};

const RECORD_SHADE_FACTOR: f32 = 0.35;

impl From<&Theme> for VinylColors {
    fn from(theme: &Theme) -> Self {
        let colors = &theme.colors;
        Self {
            paper: colors.role(Role::Text),
            border: colors.role(Role::Frame),
            record: shade(colors.role(Role::Dim), RECORD_SHADE_FACTOR),
            accent: colors.role(Role::Accent),
            blank_paper: colors.role(Role::WindowBg),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use raster::VinylColors;

    use crate::theme::{Role, Theme};

    fn theme() -> Theme {
        let file =
            config::parse_theme(include_str!("../../../../themes/noir.toml"), "noir")
                .unwrap();
        Theme::from(file)
    }

    #[test]
    fn from_theme_blank_paper_matches_window_bg_not_background() {
        let theme = theme();
        assert_ne!(
            theme.colors.role(Role::Background),
            theme.colors.role(Role::WindowBg)
        );
        let colors = VinylColors::from(&theme);
        assert_eq!(colors.blank_paper, theme.colors.role(Role::WindowBg));
    }
}
