use kernel::{
    domain::{Action, Overlay},
    update::keymap::KeyBinding,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    text::Line,
    widgets::{Paragraph, Widget},
};

use crate::{
    primitive::{
        glyphs::{KeyHintsGlyphs, TruncateGlyphs},
        span::{row, text},
        text::truncate_line_to_width,
    },
    theme::ActiveTheme,
};

const KEY_HINTS: &[(Action, &str)] = &[
    (Action::PlayPause, "Play"),
    (Action::PlaySelected, "Open"),
    (Action::Enqueue, "Queue"),
    (Action::Search, "Find"),
    (Action::Help, "Help"),
    (Action::Quit, "Quit"),
];

const KEY_HINTS_COMPACT: &[Action] = &[
    Action::PlayPause,
    Action::Search,
    Action::Help,
    Action::Quit,
];

const CHORDS_PER_ACTION: usize = 2;

fn chord_for_action(bindings: &[KeyBinding], action: Action) -> String {
    bindings
        .iter()
        .filter(|binding| binding.action == Some(action))
        .take(CHORDS_PER_ACTION)
        .map(|binding| binding.pattern.to_string())
        .collect::<Vec<_>>()
        .join("/")
}

type ChipPair = (String, &'static str);

#[derive(Debug, Clone, Copy)]
struct SettingsHint {
    primary: Action,
    secondary: Option<Action>,
    label: &'static str,
}

const SETTINGS_HINTS: [SettingsHint; 4] = [
    SettingsHint {
        primary: Action::SettingsNavigateDown,
        secondary: Some(Action::SettingsNavigateUp),
        label: "move",
    },
    SettingsHint {
        primary: Action::SettingsAdjustDown,
        secondary: Some(Action::SettingsAdjustUp),
        label: "adjust",
    },
    SettingsHint {
        primary: Action::SettingsActivate,
        secondary: None,
        label: "apply",
    },
    SettingsHint {
        primary: Action::SettingsClose,
        secondary: None,
        label: "close",
    },
];

#[derive(Debug, Clone, Copy)]
pub enum KeyHintsContent<'a> {
    Keys(&'a [KeyBinding]),
    SettingsHints(&'a [KeyBinding]),
}

impl<'a> KeyHintsContent<'a> {
    #[must_use]
    pub fn new(overlay: Option<&Overlay>, bindings: &'a [KeyBinding]) -> Self {
        if matches!(overlay, Some(Overlay::Settings(_))) {
            Self::SettingsHints(bindings)
        } else {
            Self::Keys(bindings)
        }
    }
}

#[derive(Debug)]
pub struct KeyHintsLine<'a> {
    pub theme: ActiveTheme<'a>,
    pub content: KeyHintsContent<'a>,
}

impl Widget for &KeyHintsLine<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let line = key_hints_line(self.theme, self.content, area.width);
        Paragraph::new(line).render(Rect { height: 1, ..area }, buffer);
    }
}

fn settings_chip(bindings: &[KeyBinding], hint: SettingsHint) -> ChipPair {
    let primary_chord = chord_for_action(bindings, hint.primary);
    let key = match hint.secondary {
        Some(secondary) => {
            format!("{primary_chord}/{}", chord_for_action(bindings, secondary))
        }
        None => primary_chord,
    };
    (key, hint.label)
}

fn pairs_for(content: KeyHintsContent<'_>) -> Vec<ChipPair> {
    match content {
        KeyHintsContent::Keys(bindings) => KEY_HINTS
            .iter()
            .map(|(action, label)| (chord_for_action(bindings, *action), *label))
            .collect(),
        KeyHintsContent::SettingsHints(bindings) => SETTINGS_HINTS
            .iter()
            .map(|hint| settings_chip(bindings, *hint))
            .collect(),
    }
}

fn key_hints_line(
    theme: ActiveTheme<'_>,
    content: KeyHintsContent<'_>,
    width: u16,
) -> Line<'static> {
    let chip_text: Color = theme.window_bg();
    let chip_background: Color = theme.muted_accent();
    let label: Color = theme.text();
    let separator_color: Color = theme.dim();
    let glyphs = KeyHintsGlyphs::default();
    let truncation = TruncateGlyphs::default();

    let pairs = pairs_for(content);
    let compact: Vec<ChipPair> = match content {
        KeyHintsContent::Keys(_) => KEY_HINTS
            .iter()
            .zip(pairs.iter())
            .filter(|((action, _), _)| KEY_HINTS_COMPACT.contains(action))
            .map(|(_, pair)| pair.clone())
            .collect(),
        KeyHintsContent::SettingsHints(_) => pairs.clone(),
    };

    let render = |chips: &[ChipPair]| -> Line<'static> {
        row(chips
            .iter()
            .enumerate()
            .flat_map(|(position, (key, name))| {
                let separator_piece =
                    (position > 0).then(|| text(glyphs.separator).fg(separator_color));
                separator_piece.into_iter().chain([
                    text(format!(" {key} ")).fg(chip_text).bg(chip_background),
                    text(format!(" {name}")).fg(label),
                ])
            }))
    };

    let full = render(&pairs);
    let line = if full.width() <= usize::from(width) {
        full
    } else {
        render(&compact)
    };

    truncate_line_to_width(line, usize::from(width), truncation)
}

#[cfg(test)]
mod tests {
    use kernel::{
        domain::{Action, KeymapOverrides},
        update::keymap::{Bindings, KeyBinding},
    };
    use ratatui::{Terminal, backend::TestBackend, widgets::Widget};
    use rstest::rstest;

    use crate::{
        key_hints::{KeyHintsContent, KeyHintsLine, chord_for_action},
        scene::fixtures::noir,
        theme::{ActiveTheme, ColorDepth},
    };

    fn bindings() -> Vec<KeyBinding> {
        Bindings::new(&KeymapOverrides::default())
            .as_slice()
            .to_vec()
    }

    fn widget_snapshot<W: Widget>(widget: W, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(widget, frame.area()))
            .unwrap();
        format!("{}", terminal.backend())
    }

    #[rstest]
    #[case(80)]
    #[case(60)]
    #[case(40)]
    fn key_hints_chip_row_by_width(#[case] width: u16) {
        let theme = noir();
        let bindings = bindings();
        let widget = KeyHintsLine {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            content: KeyHintsContent::Keys(&bindings),
        };
        insta::assert_snapshot!(
            format!("key_hints_chips_{width}"),
            widget_snapshot(&widget, width, 1)
        );
    }

    #[test]
    fn an_action_bound_to_two_chords_joins_them_with_a_slash() {
        let bindings = bindings();
        assert_eq!(chord_for_action(&bindings, Action::Help), "?/Ctrl+K");
    }

    #[test]
    fn an_action_bound_to_one_chord_shows_it_bare() {
        let bindings = bindings();
        assert_eq!(chord_for_action(&bindings, Action::Quit), "q");
    }

    #[test]
    fn an_unbound_action_shows_an_empty_chord() {
        let bindings: Vec<KeyBinding> = Vec::new();
        assert_eq!(chord_for_action(&bindings, Action::Help), "");
    }

    #[test]
    fn the_settings_apply_hint_shows_both_bound_chords() {
        let theme = noir();
        let bindings = bindings();
        let widget = KeyHintsLine {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            content: KeyHintsContent::SettingsHints(&bindings),
        };
        let text = widget_snapshot(&widget, 80, 1);
        assert!(text.contains("Enter/Space"), "got {text:?}");
    }
}
