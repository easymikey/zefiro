use kernel::{
    domain::{
        keymap::Action,
        revision::Revision,
        server::{Listing, Server},
    },
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
        truncate::truncate_line,
    },
    theme::active_theme::ActiveTheme,
};

const KEY_HINTS: &[(Action, &str, Scope)] = &[
    (Action::PlayPause, "Play", Scope::Everywhere),
    (Action::PlaySelected, "Open", Scope::Everywhere),
    (Action::Enqueue, "Queue", Scope::Tracks),
    (Action::NextCatalog, "Sources", Scope::Servers),
    (Action::CycleView, "View", Scope::ServerTab),
    (Action::CycleSort, "Order", Scope::Albums),
    (Action::LevelUp, "Back", Scope::Opened),
    (Action::Search, "Find", Scope::Everywhere),
    (Action::Help, "Help", Scope::Everywhere),
    (Action::Quit, "Quit", Scope::Everywhere),
];

const KEY_HINTS_COMPACT: &[Action] = &[
    Action::PlayPause,
    Action::PlaySelected,
    Action::NextCatalog,
    Action::CycleView,
    Action::CycleSort,
    Action::LevelUp,
    Action::Search,
    Action::Help,
    Action::Quit,
];

const CHORDS_PER_ACTION: usize = 2;

pub(crate) fn chords_for_action(
    bindings: &[KeyBinding],
    action: Action,
) -> impl Iterator<Item = String> {
    chords(bindings, move |binding| binding.action == Some(action))
}

pub(crate) fn chords(
    bindings: &[KeyBinding],
    wanted: impl Fn(&KeyBinding) -> bool,
) -> impl Iterator<Item = String> {
    let key_context = bindings
        .iter()
        .find(|binding| wanted(binding))
        .map(|binding| binding.key_context);
    bindings
        .iter()
        .filter(move |binding| {
            wanted(binding) && Some(binding.key_context) == key_context
        })
        .map(|binding| binding.pattern.to_string())
}

#[derive(Debug, Clone, Copy)]
enum Scope {
    Everywhere,
    Tracks,
    Servers,
    ServerTab,
    Albums,
    Opened,
}

#[derive(Debug, Clone)]
pub(crate) struct Chip {
    key: String,
    label: &'static str,
    scope: Scope,
}

impl Chip {
    fn new(chord: &str, label: &'static str) -> Self {
        Self {
            key: format!(" {chord} "),
            label,
            scope: Scope::Everywhere,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SettingsHint {
    action: Action,
    fallback_action: Option<Action>,
    label: &'static str,
}

const SETTINGS_HINTS: [SettingsHint; 4] = [
    SettingsHint {
        action: Action::SettingsNavigateDown,
        fallback_action: Some(Action::SettingsNavigateUp),
        label: "move",
    },
    SettingsHint {
        action: Action::SettingsStepDown,
        fallback_action: Some(Action::SettingsStepUp),
        label: "step",
    },
    SettingsHint {
        action: Action::SettingsActivate,
        fallback_action: None,
        label: "select",
    },
    SettingsHint {
        action: Action::SettingsClose,
        fallback_action: None,
        label: "close",
    },
];

#[derive(Debug, Clone, Default)]
pub struct KeyHintChords {
    revision: Option<Revision>,
    pub(crate) chips: Vec<Chip>,
    pub(crate) compact_chips: Vec<Chip>,
    pub(crate) settings_chips: Vec<Chip>,
}

impl KeyHintChords {
    #[must_use]
    pub fn from_bindings(bindings: &[KeyBinding]) -> Self {
        Self {
            revision: None,
            chips: key_chips(bindings, |_| true),
            compact_chips: key_chips(bindings, |action| {
                KEY_HINTS_COMPACT.contains(&action)
            }),
            settings_chips: SETTINGS_HINTS
                .iter()
                .filter_map(|hint| {
                    chip(
                        bindings,
                        [Some(hint.action), hint.fallback_action]
                            .into_iter()
                            .flatten(),
                        hint.label,
                    )
                })
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
    pub(crate) full_chips: &'a [Chip],
    pub(crate) compact_chips: &'a [Chip],
    pub(crate) listing: Option<&'a Listing>,
    pub(crate) servers: &'a [Server],
}

impl KeyHintsView<'_> {
    fn shows(self, scope: Scope) -> bool {
        match scope {
            Scope::Everywhere => true,
            Scope::Tracks => {
                !matches!(self.listing, Some(Listing::Albums(_) | Listing::Playlists))
            }
            Scope::Servers => !self.servers.is_empty(),
            Scope::ServerTab => matches!(
                self.listing,
                Some(Listing::Songs | Listing::Albums(_) | Listing::Playlists)
            ),
            Scope::Albums => matches!(self.listing, Some(Listing::Albums(_))),
            Scope::Opened => {
                matches!(self.listing, Some(Listing::Album(_) | Listing::Playlist(_)))
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct KeyHintsWidget<'a> {
    theme: ActiveTheme<'a>,
    view: KeyHintsView<'a>,
}

impl<'a> KeyHintsWidget<'a> {
    #[must_use]
    pub(crate) fn new(view: KeyHintsView<'a>, theme: ActiveTheme<'a>) -> Self {
        Self { theme, view }
    }
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

fn chip(
    bindings: &[KeyBinding],
    actions: impl IntoIterator<Item = Action>,
    label: &'static str,
) -> Option<Chip> {
    let key = actions
        .into_iter()
        .flat_map(|action| chords_for_action(bindings, action).take(CHORDS_PER_ACTION))
        .collect::<Vec<_>>()
        .join("/");
    (!key.is_empty()).then(|| Chip::new(&key, label))
}

fn key_chips(bindings: &[KeyBinding], keep: impl Fn(Action) -> bool) -> Vec<Chip> {
    KEY_HINTS
        .iter()
        .filter(|(action, _, _)| keep(*action))
        .filter_map(|(action, label, scope)| {
            chip(bindings, [*action], label).map(|found| Chip {
                scope: *scope,
                ..found
            })
        })
        .collect()
}

fn chips_line<'a>(
    theme: &ActiveTheme<'_>,
    view: KeyHintsView<'a>,
    chips: &'a [Chip],
) -> Line<'a> {
    let colors = theme.colors();
    let chip_background = theme.muted_accent();
    let chips = chips.iter().filter(move |chip| view.shows(chip.scope));
    line(chips.enumerate().flat_map(move |(position, chip)| {
        let separator = (position > 0)
            .then(|| text(glyphs::key_hints::SEPARATOR).fg(colors.muted_foreground));
        separator.into_iter().chain([
            text(chip.key.as_str())
                .fg(colors.window_background)
                .bg(chip_background),
            text(glyphs::key_hints::LABEL_GAP).fg(colors.foreground),
            text(chip.label).fg(colors.foreground),
        ])
    }))
}

fn key_hints_line<'a>(
    theme: &ActiveTheme<'_>,
    view: KeyHintsView<'a>,
    width: u16,
) -> Line<'a> {
    let full = chips_line(theme, view, view.full_chips);
    let line = if full.width() <= usize::from(width) {
        full
    } else {
        chips_line(theme, view, view.compact_chips)
    };
    truncate_line(line, usize::from(width))
}

#[cfg(test)]
mod tests {
    use kernel::{
        domain::{keymap::Action, revision::Revision},
        update::keymap::{bindings::Keymap, chord::KeyBinding},
    };
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
    use rstest::rstest;

    use crate::{
        key_hints::{KeyHintChords, KeyHintsView, KeyHintsWidget, chords_for_action},
        repaint::Presence,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    fn stock_chords() -> KeyHintChords {
        KeyHintChords::from_bindings(Keymap::default().bindings())
    }

    fn keys_view(chords: &KeyHintChords) -> KeyHintsView<'_> {
        KeyHintsView {
            full_chips: &chords.chips,
            compact_chips: &chords.compact_chips,
            listing: None,
            servers: &[],
        }
    }

    fn settings_view(chords: &KeyHintChords) -> KeyHintsView<'_> {
        KeyHintsView {
            full_chips: &chords.settings_chips,
            compact_chips: &chords.settings_chips,
            listing: None,
            servers: &[],
        }
    }

    fn hints_text_at(view: KeyHintsView<'_>, width: u16) -> String {
        let theme = noir();
        let widget =
            KeyHintsWidget::new(view, ActiveTheme::new(&theme, ColorDepth::TrueColor));
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
    fn an_area_without_rows_paints_nothing() {
        let theme = noir();
        let chords = stock_chords();
        let widget = KeyHintsWidget::new(
            keys_view(&chords),
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        let blank = Buffer::empty(Rect::new(0, 0, 40, 2));
        let mut buffer = blank.clone();
        (&widget).render(Rect::new(0, 0, 40, 0), &mut buffer);
        assert_eq!(buffer, blank);
    }

    #[test]
    fn an_action_lists_only_the_chords_of_its_first_context() {
        let keymap = Keymap::default();
        assert_eq!(
            chords_for_action(keymap.bindings(), Action::Delete).collect::<Vec<_>>(),
            ["d"]
        );
    }

    #[test]
    fn chords_rebuild_only_when_the_config_revision_moves() {
        let stock_keymap = Keymap::default();
        let mut chords = KeyHintChords::default();
        chords.follow(stock_keymap.bindings(), Revision::default());
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

    #[rstest]
    #[case::the_select_hint_shows_both_bound_chords(
        Keymap::default().bindings().to_vec(),
        "Enter/Space",
        Presence::Shown
    )]
    #[case::a_half_bound_pair_shows_only_its_bound_chord(
        without(Action::SettingsNavigateUp),
        " j/↓  move",
        Presence::Shown
    )]
    #[case::a_fully_unbound_pair_shows_no_hint(
        without(Action::SettingsClose),
        "close",
        Presence::Hidden
    )]
    fn settings_hints_show_only_bound_chords(
        #[case] bindings: Vec<KeyBinding>,
        #[case] hint: &str,
        #[case] presence: Presence,
    ) {
        let chords = KeyHintChords::from_bindings(&bindings);
        let text = hints_text(settings_view(&chords));
        assert_eq!(
            Presence::from(text.contains(hint)),
            presence,
            "got {text:?}"
        );
    }
}
