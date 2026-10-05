use kernel::{
    domain::{keymap::Action, revision::Revision},
    update::keymap::chord::KeyBinding,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};

use crate::{
    primitive::{
        glyphs,
        span::{line, text},
        text::truncate_line_to_width,
    },
    theme::active_theme::ActiveTheme,
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

#[derive(Debug, Clone)]
pub(crate) struct Chip {
    key: String,
    label: &'static str,
}

impl Chip {
    fn new(chord: &str, label: &'static str) -> Self {
        Self {
            key: format!(" {chord} "),
            label,
        }
    }
}

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
        primary: Action::SettingsStepDown,
        secondary: Some(Action::SettingsStepUp),
        label: "step",
    },
    SettingsHint {
        primary: Action::SettingsActivate,
        secondary: None,
        label: "select",
    },
    SettingsHint {
        primary: Action::SettingsClose,
        secondary: None,
        label: "close",
    },
];

#[derive(Debug, Clone, Default)]
pub struct KeyHintChords {
    revision: Option<Revision>,
    pub(crate) keys: Vec<Chip>,
    pub(crate) compact: Vec<Chip>,
    pub(crate) settings: Vec<Chip>,
}

impl KeyHintChords {
    #[must_use]
    pub fn from_bindings(bindings: &[KeyBinding]) -> Self {
        Self {
            revision: None,
            keys: key_chips(bindings, |_| true),
            compact: key_chips(bindings, |action| KEY_HINTS_COMPACT.contains(&action)),
            settings: SETTINGS_HINTS
                .iter()
                .filter_map(|hint| settings_chip(bindings, *hint))
                .collect(),
        }
    }

    pub fn follow(&mut self, bindings: &[KeyBinding], revision: Revision) {
        if self.revision == Some(revision) {
            return;
        }
        *self = Self {
            revision: Some(revision),
            ..Self::from_bindings(bindings)
        };
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct KeyHintsView<'a> {
    pub(crate) full: &'a [Chip],
    pub(crate) compact: &'a [Chip],
}

#[derive(Debug)]
pub(crate) struct KeyHintsWidget<'a> {
    pub(crate) theme: ActiveTheme<'a>,
    pub(crate) view: KeyHintsView<'a>,
}

impl Widget for &KeyHintsWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let line = key_hints_line(&self.theme, self.view, area.width);
        Paragraph::new(line).render(Rect { height: 1, ..area }, buffer);
    }
}

fn settings_chip(bindings: &[KeyBinding], hint: SettingsHint) -> Option<Chip> {
    let key = [Some(hint.primary), hint.secondary]
        .into_iter()
        .flatten()
        .map(|action| chord_for_action(bindings, action))
        .filter(|chord| !chord.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    (!key.is_empty()).then(|| Chip::new(&key, hint.label))
}

fn key_chips(bindings: &[KeyBinding], keep: impl Fn(Action) -> bool) -> Vec<Chip> {
    KEY_HINTS
        .iter()
        .filter(|(action, _)| keep(*action))
        .map(|(action, label)| (chord_for_action(bindings, *action), *label))
        .filter(|(chord, _)| !chord.is_empty())
        .map(|(chord, label)| Chip::new(&chord, label))
        .collect()
}

fn chips_line<'a>(theme: &ActiveTheme<'_>, chips: &'a [Chip]) -> Line<'a> {
    let colors = theme.colors();
    let chip_background = theme.muted_accent();
    line(chips.iter().enumerate().flat_map(move |(position, chip)| {
        let separator = (position > 0)
            .then(|| text(glyphs::key_hints::SEPARATOR).fg(colors.muted_foreground));
        separator.into_iter().chain([
            text(chip.key.as_str())
                .fg(colors.window_background)
                .bg(chip_background),
            text(glyphs::key_hints::LABEL_GAP).fg(colors.text),
            text(chip.label).fg(colors.text),
        ])
    }))
}

fn key_hints_line<'a>(
    theme: &ActiveTheme<'_>,
    view: KeyHintsView<'a>,
    width: u16,
) -> Line<'a> {
    let full = chips_line(theme, view.full);
    let line = if full.width() <= usize::from(width) {
        full
    } else {
        chips_line(theme, view.compact)
    };
    truncate_line_to_width(line, usize::from(width))
}

#[cfg(test)]
mod tests {
    use kernel::{
        domain::{keymap::Action, revision::Revision},
        update::keymap::{bindings::Keymap, chord::KeyBinding},
    };
    use rstest::rstest;

    use crate::{
        key_hints::{KeyHintChords, KeyHintsView, KeyHintsWidget, chord_for_action},
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn stock_chords() -> KeyHintChords {
        KeyHintChords::from_bindings(Keymap::default().bindings())
    }

    fn keys_view(chords: &KeyHintChords) -> KeyHintsView<'_> {
        KeyHintsView {
            full: &chords.keys,
            compact: &chords.compact,
        }
    }

    fn settings_view(chords: &KeyHintChords) -> KeyHintsView<'_> {
        KeyHintsView {
            full: &chords.settings,
            compact: &chords.settings,
        }
    }

    fn hints_text_at(view: KeyHintsView<'_>, width: u16) -> String {
        let theme = noir();
        let widget = KeyHintsWidget {
            theme: ActiveTheme::new(&theme, ColorDepth::TrueColor),
            view,
        };
        rendered(width, 1, |frame| frame.render_widget(&widget, frame.area()))
            .to_string()
    }

    fn hints_text(view: KeyHintsView<'_>) -> String {
        hints_text_at(view, 80)
    }

    #[rstest]
    #[case(80)]
    #[case(60)]
    #[case(40)]
    fn key_hints_chip_row_by_width(#[case] width: u16) {
        let chords = stock_chords();
        insta::assert_snapshot!(
            format!("key_hints_drop_chips_as_the_width_shrinks_{width}"),
            hints_text_at(keys_view(&chords), width)
        );
    }

    #[test]
    fn an_action_bound_to_two_chords_joins_them_with_a_slash() {
        let keymap = Keymap::default();
        let bindings = keymap.bindings();
        assert_eq!(chord_for_action(bindings, Action::Help), "?/Ctrl+K");
    }

    #[test]
    fn an_action_bound_to_one_chord_shows_it_bare() {
        let keymap = Keymap::default();
        let bindings = keymap.bindings();
        assert_eq!(chord_for_action(bindings, Action::Quit), "q");
    }

    #[test]
    fn an_unbound_action_shows_an_empty_chord() {
        let bindings: Vec<KeyBinding> = Vec::new();
        assert_eq!(chord_for_action(&bindings, Action::Help), "");
    }

    #[test]
    fn the_settings_select_hint_shows_both_bound_chords() {
        let chords = stock_chords();
        let text = hints_text(settings_view(&chords));
        assert!(text.contains("Enter/Space"), "got {text:?}");
    }

    #[test]
    fn chords_rebuild_only_when_the_config_revision_moves() {
        let stock = Keymap::default();
        let mut chords = KeyHintChords::default();
        chords.follow(stock.bindings(), Revision::default());
        chords.follow(&without(Action::Search), Revision::default());
        assert!(hints_text(keys_view(&chords)).contains("Find"));
        chords.follow(&without(Action::Search), Revision::default().next());
        assert!(!hints_text(keys_view(&chords)).contains("Find"));
    }

    fn without(action: Action) -> Vec<KeyBinding> {
        Keymap::default()
            .bindings()
            .iter()
            .filter(|binding| binding.action != Some(action))
            .cloned()
            .collect()
    }

    #[test]
    fn an_unbound_action_shows_no_hint() {
        let chords = KeyHintChords::from_bindings(&without(Action::Search));
        let text = hints_text(keys_view(&chords));
        assert!(!text.contains("Find"), "got {text:?}");
        assert!(text.contains("Help"), "got {text:?}");
    }

    #[test]
    fn a_half_bound_settings_pair_shows_only_its_bound_chord() {
        let bindings = without(Action::SettingsNavigateUp);
        let down = chord_for_action(&bindings, Action::SettingsNavigateDown);
        let chords = KeyHintChords::from_bindings(&bindings);
        let text = hints_text(settings_view(&chords));
        assert!(text.contains(&format!(" {down}  move")), "got {text:?}");
    }

    #[test]
    fn a_fully_unbound_settings_pair_shows_no_hint() {
        let chords = KeyHintChords::from_bindings(&without(Action::SettingsClose));
        let text = hints_text(settings_view(&chords));
        assert!(!text.contains("close"), "got {text:?}");
    }
}
