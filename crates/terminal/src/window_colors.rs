use std::io::{self, Write};

use kernel::domain::appearance::Rgb;
use widgets::theme::{Theme, rgb::lerp_rgb};

use crate::error::Error;

const OSC: &str = "\x1b]";
const BEL: &str = "\x07";

const OSC_SET_BACKGROUND: u16 = 11;
const OSC_SET_FOREGROUND: u16 = 10;
const OSC_SET_CURSOR: u16 = 12;
const OSC_RESET_BACKGROUND: u16 = 111;
const OSC_RESET_FOREGROUND: u16 = 110;
const OSC_RESET_CURSOR: u16 = 112;

fn osc_set(code: u16, rgb: Rgb) -> String {
    let [r, g, b] = rgb.0;
    format!("{OSC}{code};#{r:02x}{g:02x}{b:02x}{BEL}")
}

fn osc_reset(code: u16) -> String {
    format!("{OSC}{code}{BEL}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shade {
    pub background: Rgb,
    pub foreground: Rgb,
}

impl From<&Theme> for Shade {
    fn from(theme: &Theme) -> Self {
        Self {
            background: theme.colors.window_background,
            foreground: theme.colors.foreground,
        }
    }
}

impl Shade {
    #[must_use]
    pub fn lerp(self, incoming: Self, fraction: f32) -> Self {
        Self {
            background: lerp_rgb(self.background, incoming.background, fraction),
            foreground: lerp_rgb(self.foreground, incoming.foreground, fraction),
        }
    }
}

fn set_sequence(shade: Shade) -> String {
    [
        osc_set(OSC_SET_BACKGROUND, shade.background),
        osc_set(OSC_SET_FOREGROUND, shade.foreground),
        osc_set(OSC_SET_CURSOR, shade.foreground),
    ]
    .concat()
}

fn reset_sequence() -> String {
    [
        osc_reset(OSC_RESET_BACKGROUND),
        osc_reset(OSC_RESET_FOREGROUND),
        osc_reset(OSC_RESET_CURSOR),
    ]
    .concat()
}

fn write_to_stdout(sequence: &str) -> Result<(), io::Error> {
    let mut stdout = io::stdout();
    stdout.write_all(sequence.as_bytes())?;
    stdout.flush()
}

pub(crate) fn reset_on_panic() -> Result<(), io::Error> {
    write_to_stdout(&reset_sequence())
}

pub fn write_window_colors(shade: Shade) -> Result<(), Error> {
    write_to_stdout(&set_sequence(shade)).map_err(Error::WriteWindowColors)
}

pub fn reset_window_colors() -> Result<(), Error> {
    write_to_stdout(&reset_sequence()).map_err(Error::WriteWindowColors)
}

#[cfg(test)]
mod tests {
    use kernel::domain::{appearance::Rgb, theme::ThemeName};
    use widgets::theme::{
        Theme,
        colors::{Colors, ThemeBase},
    };

    use crate::window_colors::{Shade, reset_sequence, set_sequence};

    const BACKGROUND: Rgb = Rgb([0x1a, 0x2b, 0x3c]);
    const FOREGROUND: Rgb = Rgb([0xff, 0x00, 0x99]);

    #[test]
    fn the_window_color_sequences_are_exact_bytes() {
        let shade = Shade {
            background: BACKGROUND,
            foreground: FOREGROUND,
        };
        let written = [set_sequence(shade), reset_sequence()]
            .map(|sequence| sequence.escape_debug().to_string())
            .join("\n");
        insta::assert_snapshot!(written);
    }

    fn theme(window_background: Rgb, foreground: Rgb) -> Theme {
        Theme {
            name: ThemeName::from_static("wash"),
            colors: Colors::from_theme_base(&ThemeBase {
                background: window_background,
                muted_foreground: foreground,
                foreground,
                accent: foreground,
                green: foreground,
                yellow: foreground,
                red: foreground,
                window_background: Some(window_background),
            }),
            scanning_label: String::new(),
        }
    }

    #[test]
    fn the_shade_of_a_theme_is_its_window_background_and_foreground() {
        let shade = Shade::from(&theme(BACKGROUND, FOREGROUND));

        assert_eq!(
            shade,
            Shade {
                background: BACKGROUND,
                foreground: FOREGROUND,
            }
        );
    }

    #[test]
    fn the_lerp_runs_from_the_outgoing_to_the_incoming_shade() {
        let outgoing = Shade::from(&theme(BACKGROUND, FOREGROUND));
        let incoming = Shade::from(&theme(FOREGROUND, BACKGROUND));

        assert_eq!(outgoing.lerp(incoming, 0.0), outgoing);
        assert_eq!(outgoing.lerp(incoming, 1.0), incoming);
    }
}
