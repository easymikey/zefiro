use std::io::{self, Write};

use config::Hex;
use kernel::{WindowColorsCmd, domain::ThemeName};
use widgets::{Role, Theme};

const OSC: &str = "\x1b]";
const BEL: &str = "\x07";

const OSC_SET_BACKGROUND: u16 = 11;
const OSC_SET_FOREGROUND: u16 = 10;
const OSC_SET_CURSOR: u16 = 12;
const OSC_RESET_BACKGROUND: u16 = 111;
const OSC_RESET_FOREGROUND: u16 = 110;
const OSC_RESET_CURSOR: u16 = 112;

fn osc_set(code: u16, hex: Hex) -> String {
    let [r, g, b] = hex.0;
    format!("{OSC}{code};#{r:02x}{g:02x}{b:02x}{BEL}")
}

fn osc_reset(code: u16) -> String {
    format!("{OSC}{code}{BEL}")
}

fn set_background(hex: Hex) -> String {
    osc_set(OSC_SET_BACKGROUND, hex)
}

fn set_foreground(hex: Hex) -> String {
    osc_set(OSC_SET_FOREGROUND, hex)
}

fn set_cursor(hex: Hex) -> String {
    osc_set(OSC_SET_CURSOR, hex)
}

fn reset_background() -> String {
    osc_reset(OSC_RESET_BACKGROUND)
}

fn reset_foreground() -> String {
    osc_reset(OSC_RESET_FOREGROUND)
}

fn reset_cursor() -> String {
    osc_reset(OSC_RESET_CURSOR)
}

fn apply_sequence(background: Hex, foreground: Hex) -> String {
    let mut sequence = set_background(background);
    sequence.push_str(&set_foreground(foreground));
    sequence.push_str(&set_cursor(foreground));
    sequence
}

fn reset_sequence() -> String {
    let mut sequence = reset_background();
    sequence.push_str(&reset_foreground());
    sequence.push_str(&reset_cursor());
    sequence
}

pub(crate) fn emit(sequence: &str) {
    let mut stdout = io::stdout();
    let _ = stdout.write_all(sequence.as_bytes());
    let _ = stdout.flush();
}

pub(crate) fn reset_on_panic() {
    emit(&reset_sequence());
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no such theme: {name}")]
pub struct UnknownThemeError {
    pub name: String,
}

fn apply_to(name: &ThemeName, theme: &Theme) -> Result<String, UnknownThemeError> {
    if name.as_str() == theme.name {
        Ok(apply_sequence(
            theme.colors.role(Role::WindowBg),
            theme.colors.role(Role::Text),
        ))
    } else {
        Err(UnknownThemeError {
            name: name.as_str().to_string(),
        })
    }
}

pub fn window_colors_sequence(
    command: &WindowColorsCmd,
    theme: &Theme,
) -> Result<String, UnknownThemeError> {
    match command {
        WindowColorsCmd::Apply(name) => apply_to(name, theme),
        WindowColorsCmd::Reset => Ok(reset_sequence()),
    }
}

pub fn write_window_colors(
    command: &WindowColorsCmd,
    theme: &Theme,
) -> Result<(), UnknownThemeError> {
    emit(&window_colors_sequence(command, theme)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use config::{Hex, ThemeColors};
    use kernel::{WindowColorsCmd, domain::ThemeName};
    use rstest::rstest;
    use widgets::{Colors, Role, Theme};

    use crate::window_colors::{
        UnknownThemeError,
        apply_sequence,
        reset_sequence,
        window_colors_sequence,
    };

    const BACKGROUND: Hex = Hex([0x1a, 0x2b, 0x3c]);
    const FOREGROUND: Hex = Hex([0xff, 0x00, 0x99]);
    const KNOWN_THEME: &str = "noir";

    #[test]
    fn the_window_color_sequences_are_exact_bytes() {
        let written = [apply_sequence(BACKGROUND, FOREGROUND), reset_sequence()]
            .map(|sequence| sequence.escape_debug().to_string())
            .join("\n");
        insta::assert_snapshot!(written);
    }

    fn theme() -> Theme {
        Theme {
            name: KNOWN_THEME.to_string(),
            colors: Colors::derive(&ThemeColors {
                background: Hex([0x10, 0x10, 0x10]),
                foreground: Hex([0xe0, 0xe0, 0xe0]),
                bright_foreground: Hex([0xf0, 0xf0, 0xf0]),
                accent: Hex([0x20, 0x60, 0xa0]),
                green: Hex([0x00, 0xff, 0x00]),
                yellow: Hex([0xff, 0xff, 0x00]),
                red: Hex([0xff, 0x00, 0x00]),
                window_background: None,
            }),
            scanning_label: String::new(),
        }
    }

    #[rstest]
    #[case::apply_to_the_same_theme(
        WindowColorsCmd::Apply(ThemeName::from_static(KNOWN_THEME)),
        Ok(apply_sequence(
            theme().colors.role(Role::WindowBg),
            theme().colors.role(Role::Text),
        ))
    )]
    #[case::apply_to_another_name(
        WindowColorsCmd::Apply(ThemeName::from_static("no-such-theme")),
        Err(UnknownThemeError {
            name: "no-such-theme".to_string(),
        })
    )]
    #[case::reset_ignores_the_theme(WindowColorsCmd::Reset, Ok(reset_sequence()))]
    fn window_colors_sequence_rows(
        #[case] command: WindowColorsCmd,
        #[case] expected: Result<String, UnknownThemeError>,
    ) {
        assert_eq!(window_colors_sequence(&command, &theme()), expected);
    }
}
