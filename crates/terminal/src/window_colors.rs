use std::io::{self, Write};

use config::Hex;
use kernel::WindowColorsCmd;
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

pub struct WindowColorsWriter {
    write: fn(&str),
    commands: crossbeam_channel::Receiver<WindowColorsCmd>,
}

impl std::fmt::Debug for WindowColorsWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowColorsWriter")
            .finish_non_exhaustive()
    }
}

impl WindowColorsWriter {
    #[must_use]
    pub fn new(commands: crossbeam_channel::Receiver<WindowColorsCmd>) -> Self {
        Self {
            write: emit,
            commands,
        }
    }

    #[must_use]
    pub fn disconnected() -> Self {
        Self::new(crossbeam_channel::never())
    }

    pub fn obey(
        &self,
        resolve: impl Fn(&str) -> Option<Theme>,
    ) -> Option<UnknownThemeError> {
        let mut unresolved = None;
        for command in self.commands.try_iter() {
            match command {
                WindowColorsCmd::Apply(name) => match resolve(&name) {
                    Some(theme) => {
                        (self.write)(&apply_sequence(
                            theme.colors.role(Role::WindowBg),
                            theme.colors.role(Role::Text),
                        ));
                    }
                    None => unresolved = Some(UnknownThemeError { name }),
                },
                WindowColorsCmd::Reset => (self.write)(&reset_sequence()),
            }
        }
        unresolved
    }
}

impl Default for WindowColorsWriter {
    fn default() -> Self {
        Self::disconnected()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use config::{Hex, ThemeColors};
    use kernel::WindowColorsCmd;
    use rstest::rstest;
    use widgets::{Colors, Theme};

    use crate::window_colors::{
        WindowColorsWriter,
        apply_sequence,
        emit,
        reset_sequence,
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

    thread_local! {
        static WRITTEN: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    }

    fn capture(sequence: &str) {
        WRITTEN
            .with_borrow_mut(|written| written.extend_from_slice(sequence.as_bytes()));
    }

    fn escapes_written() -> usize {
        WRITTEN
            .with_borrow(|written| written.iter().filter(|byte| **byte == 0x1b).count())
    }

    fn resolve(name: &str) -> Option<Theme> {
        (name == KNOWN_THEME).then(|| Theme {
            name: name.to_string(),
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
        })
    }

    #[rstest]
    #[case::a_known_theme_paints_three_sequences(
        &[WindowColorsCmd::Apply(KNOWN_THEME.to_string())],
        3,
        None
    )]
    #[case::a_reset_paints_three_sequences(&[WindowColorsCmd::Reset], 3, None)]
    #[case::an_unknown_theme_paints_nothing_and_reports(
        &[WindowColorsCmd::Apply("no-such-theme".to_string())],
        0,
        Some("no-such-theme")
    )]
    #[case::nothing_commanded_writes_nothing(&[], 0, None)]
    fn the_driver_obeys_each_command_and_remembers_none(
        #[case] commands: &[WindowColorsCmd],
        #[case] escapes: usize,
        #[case] unresolvable: Option<&str>,
    ) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        for command in commands {
            let _ = sender.send(command.clone());
        }
        let colors = WindowColorsWriter {
            write: capture,
            commands: receiver,
        };

        let unresolved = colors.obey(resolve);

        assert_eq!(escapes_written(), escapes);
        match unresolvable {
            Some(name) => {
                assert!(
                    unresolved.is_some_and(|error| error.to_string().contains(name))
                );
            }
            None => assert!(unresolved.is_none()),
        }
    }

    #[test]
    fn a_disconnected_driver_writes_nothing() {
        let colors = WindowColorsWriter {
            write: emit,
            commands: crossbeam_channel::never(),
        };
        assert!(colors.obey(|_| None).is_none());
    }
}
