use std::io::{self, Write};

use kernel::{
    WindowColorsCmd,
    domain::{ThemeName, appearance::Rgb},
};
use widgets::{Role, Theme};

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

fn set_sequence(background: Rgb, foreground: Rgb) -> String {
    let mut sequence = osc_set(OSC_SET_BACKGROUND, background);
    sequence.push_str(&osc_set(OSC_SET_FOREGROUND, foreground));
    sequence.push_str(&osc_set(OSC_SET_CURSOR, foreground));
    sequence
}

fn reset_sequence() -> String {
    let mut sequence = osc_reset(OSC_RESET_BACKGROUND);
    sequence.push_str(&osc_reset(OSC_RESET_FOREGROUND));
    sequence.push_str(&osc_reset(OSC_RESET_CURSOR));
    sequence
}

pub(crate) fn write_to_stdout(sequence: &str) {
    let mut stdout = io::stdout();
    let _ = stdout.write_all(sequence.as_bytes());
    let _ = stdout.flush();
}

pub(crate) fn reset_on_panic() {
    write_to_stdout(&reset_sequence());
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no such theme: {name}")]
pub struct UnknownThemeError {
    pub name: String,
}

fn sequence_for(name: &ThemeName, theme: &Theme) -> Result<String, UnknownThemeError> {
    if name.as_str() == theme.name {
        Ok(set_sequence(
            theme.colors.role(Role::WindowBackground),
            theme.colors.role(Role::Text),
        ))
    } else {
        Err(UnknownThemeError {
            name: name.as_str().to_string(),
        })
    }
}

pub(crate) fn window_colors_sequence(
    command: &WindowColorsCmd,
    theme: &Theme,
) -> Result<String, UnknownThemeError> {
    match command {
        WindowColorsCmd::Set(name) => sequence_for(name, theme),
        WindowColorsCmd::Reset => Ok(reset_sequence()),
    }
}

pub fn write_window_colors(
    command: &WindowColorsCmd,
    theme: &Theme,
) -> Result<(), UnknownThemeError> {
    write_to_stdout(&window_colors_sequence(command, theme)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use kernel::{
        WindowColorsCmd,
        domain::{ThemeName, appearance::Rgb},
    };
    use rstest::rstest;
    use widgets::{Colors, Role, Theme, ThemeSeed};

    use crate::window_colors::{
        UnknownThemeError,
        reset_sequence,
        set_sequence,
        window_colors_sequence,
    };

    const BACKGROUND: Rgb = Rgb([0x1a, 0x2b, 0x3c]);
    const FOREGROUND: Rgb = Rgb([0xff, 0x00, 0x99]);
    const KNOWN_THEME: &str = "noir";

    #[test]
    fn the_window_color_sequences_are_exact_bytes() {
        let written = [set_sequence(BACKGROUND, FOREGROUND), reset_sequence()]
            .map(|sequence| sequence.escape_debug().to_string())
            .join("\n");
        insta::assert_snapshot!(written);
    }

    fn theme() -> Theme {
        Theme {
            name: ThemeName::from_static(KNOWN_THEME),
            colors: Colors::derive(&ThemeSeed {
                background: Rgb([0x10, 0x10, 0x10]),
                foreground: Rgb([0xe0, 0xe0, 0xe0]),
                bright_foreground: Rgb([0xf0, 0xf0, 0xf0]),
                accent: Rgb([0x20, 0x60, 0xa0]),
                green: Rgb([0x00, 0xff, 0x00]),
                yellow: Rgb([0xff, 0xff, 0x00]),
                red: Rgb([0xff, 0x00, 0x00]),
                window_background: None,
            }),
            scanning_label: String::new(),
        }
    }

    #[rstest]
    #[case::set_to_the_same_theme(
        WindowColorsCmd::Set(ThemeName::from_static(KNOWN_THEME)),
        Ok(set_sequence(
            theme().colors.role(Role::WindowBackground),
            theme().colors.role(Role::Text),
        ))
    )]
    #[case::set_to_another_name(
        WindowColorsCmd::Set(ThemeName::from_static("no-such-theme")),
        Err(UnknownThemeError {
            name: "no-such-theme".to_string(),
        })
    )]
    #[case::reset_ignores_the_theme(WindowColorsCmd::Reset, Ok(reset_sequence()))]
    fn applying_a_known_theme_sets_its_colors_an_unknown_one_errors_and_reset_ignores_the_theme(
        #[case] command: WindowColorsCmd,
        #[case] expected: Result<String, UnknownThemeError>,
    ) {
        assert_eq!(window_colors_sequence(&command, &theme()), expected);
    }
}
