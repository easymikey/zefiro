use std::time::Duration;

use ratatui_image::{
    FontSize,
    picker::{Capability, Picker, ProtocolType, cap_parser::QueryStdioOptions},
};
use widgets::{
    geometry::DEFAULT_CELL_ASPECT,
    scene::PixelPath,
    theme::rgb::ColorDepth,
};

use crate::error::Error;

#[derive(Debug, Default)]
pub struct TerminalEnvironment {
    term_program: Option<String>,
    kitty_window_id: Option<String>,
    ghostty_resources_dir: Option<String>,
    wezterm_executable: Option<String>,
    iterm_session_id: Option<String>,
    term: Option<String>,
}

impl TerminalEnvironment {
    #[must_use]
    pub fn current() -> Self {
        Self {
            term_program: std::env::var("TERM_PROGRAM").ok(),
            kitty_window_id: std::env::var("KITTY_WINDOW_ID").ok(),
            ghostty_resources_dir: std::env::var("GHOSTTY_RESOURCES_DIR").ok(),
            wezterm_executable: std::env::var("WEZTERM_EXECUTABLE").ok(),
            iterm_session_id: std::env::var("ITERM_SESSION_ID").ok(),
            term: std::env::var("TERM").ok(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalApp {
    Kitty,
    Ghostty,
    Iterm2,
    WezTerm,
    Apple,
    Unknown,
}

impl TerminalApp {
    #[must_use]
    pub fn from_environment(environment: &TerminalEnvironment) -> Self {
        let program = environment.term_program.as_deref().unwrap_or("");
        let term = environment.term.as_deref().unwrap_or("");
        if environment.kitty_window_id.is_some() || term.contains("kitty") {
            TerminalApp::Kitty
        } else if environment.ghostty_resources_dir.is_some()
            || program.eq_ignore_ascii_case("ghostty")
            || term.contains("ghostty")
        {
            TerminalApp::Ghostty
        } else if environment.iterm_session_id.is_some() || program == "iTerm.app" {
            TerminalApp::Iterm2
        } else if environment.wezterm_executable.is_some() || program == "WezTerm" {
            TerminalApp::WezTerm
        } else if program == "Apple_Terminal" {
            TerminalApp::Apple
        } else {
            TerminalApp::Unknown
        }
    }
}

#[must_use]
pub(crate) fn protocols(app: TerminalApp) -> &'static [ProtocolType] {
    match app {
        TerminalApp::Kitty | TerminalApp::Ghostty => &[ProtocolType::Kitty],
        TerminalApp::Iterm2 | TerminalApp::WezTerm => {
            &[ProtocolType::Iterm2, ProtocolType::Sixel]
        }
        TerminalApp::Apple => &[],
        TerminalApp::Unknown => &[
            ProtocolType::Kitty,
            ProtocolType::Iterm2,
            ProtocolType::Sixel,
        ],
    }
}

#[derive(Debug)]
pub struct Capabilities {
    pub picker: Picker,
    pub color_depth: ColorDepth,
}

fn select_protocol_type(
    choices: &[ProtocolType],
    protocol_type: ProtocolType,
    capabilities: &[Capability],
) -> Option<ProtocolType> {
    choices.iter().copied().find(|&choice| {
        choice == protocol_type
            || (choice == ProtocolType::Sixel
                && capabilities.contains(&Capability::Sixel))
    })
}

impl Capabilities {
    #[must_use]
    pub fn from_environment(environment: &TerminalEnvironment) -> Self {
        Capabilities {
            picker: Picker::halfblocks(),
            color_depth: ColorDepth::from_term_program(
                environment.term_program.as_deref(),
            ),
        }
    }

    #[must_use]
    pub fn pixel_path(&self) -> PixelPath {
        match self.picker.protocol_type() {
            ProtocolType::Halfblocks => PixelPath::Halfblocks,
            ProtocolType::Sixel | ProtocolType::Kitty | ProtocolType::Iterm2 => {
                PixelPath::Protocol
            }
        }
    }
}

const QUERY_TIMEOUT: Duration = Duration::from_millis(300);

pub fn query(app: TerminalApp) -> Result<Option<Picker>, Error> {
    if protocols(app).is_empty() {
        return Ok(None);
    }
    let mut picker = Picker::from_query_stdio_with_options(QueryStdioOptions {
        timeout: QUERY_TIMEOUT,
        ..QueryStdioOptions::default()
    })
    .map_err(Error::Query)?;
    let confirmed = select_protocol_type(
        protocols(app),
        picker.protocol_type(),
        picker.capabilities(),
    );
    Ok(confirmed.map(|protocol_type| {
        picker.set_protocol_type(protocol_type);
        picker
    }))
}

#[must_use]
pub fn cell_aspect(font_size: FontSize) -> f32 {
    if font_size.width > 0 {
        f32::from(font_size.height) / f32::from(font_size.width)
    } else {
        DEFAULT_CELL_ASPECT
    }
}

#[cfg(test)]
mod tests {
    use ratatui_image::{
        FontSize,
        picker::{Capability, Picker, ProtocolType},
    };
    use rstest::rstest;
    use widgets::{
        geometry::DEFAULT_CELL_ASPECT,
        scene::PixelPath,
        theme::rgb::ColorDepth,
    };

    use crate::capabilities::{
        Capabilities,
        TerminalApp,
        TerminalEnvironment,
        cell_aspect,
        protocols,
        query,
        select_protocol_type,
    };

    fn named(program: &str) -> Option<String> {
        Some(program.to_string())
    }

    fn kitty_term() -> TerminalEnvironment {
        TerminalEnvironment {
            term: named("xterm-kitty"),
            ..TerminalEnvironment::default()
        }
    }

    #[rstest]
    #[case::kitty_by_term(kitty_term(), TerminalApp::Kitty)]
    #[case::kitty_by_window_id(
        TerminalEnvironment { kitty_window_id: named("1"), ..TerminalEnvironment::default() },
        TerminalApp::Kitty
    )]
    #[case::ghostty_by_program(
        TerminalEnvironment { term_program: named("ghostty"), ..TerminalEnvironment::default() },
        TerminalApp::Ghostty
    )]
    #[case::ghostty_by_resources_dir(
        TerminalEnvironment { ghostty_resources_dir: named("/tmp"), ..TerminalEnvironment::default() },
        TerminalApp::Ghostty
    )]
    #[case::iterm2_by_program(
        TerminalEnvironment { term_program: named("iTerm.app"), ..TerminalEnvironment::default() },
        TerminalApp::Iterm2
    )]
    #[case::iterm2_by_session_id(
        TerminalEnvironment { iterm_session_id: named("id"), ..TerminalEnvironment::default() },
        TerminalApp::Iterm2
    )]
    #[case::wezterm_by_program(
        TerminalEnvironment { term_program: named("WezTerm"), ..TerminalEnvironment::default() },
        TerminalApp::WezTerm
    )]
    #[case::wezterm_by_executable(
        TerminalEnvironment { wezterm_executable: named("wezterm"), ..TerminalEnvironment::default() },
        TerminalApp::WezTerm
    )]
    #[case::apple_by_program(
        TerminalEnvironment { term_program: named("Apple_Terminal"), ..TerminalEnvironment::default() },
        TerminalApp::Apple
    )]
    #[case::nothing_named(TerminalEnvironment::default(), TerminalApp::Unknown)]
    fn from_environment_names_the_terminal(
        #[case] environment: TerminalEnvironment,
        #[case] expected: TerminalApp,
    ) {
        assert_eq!(TerminalApp::from_environment(&environment), expected);
    }

    #[rstest]
    #[case::kitty(TerminalApp::Kitty, &[ProtocolType::Kitty])]
    #[case::ghostty(TerminalApp::Ghostty, &[ProtocolType::Kitty])]
    #[case::iterm2(TerminalApp::Iterm2, &[ProtocolType::Iterm2, ProtocolType::Sixel])]
    #[case::wezterm(TerminalApp::WezTerm, &[ProtocolType::Iterm2, ProtocolType::Sixel])]
    #[case::apple(TerminalApp::Apple, &[])]
    #[case::unknown(TerminalApp::Unknown, &[ProtocolType::Kitty, ProtocolType::Iterm2, ProtocolType::Sixel])]
    fn each_terminal_app_names_its_protocols(
        #[case] app: TerminalApp,
        #[case] expected: &[ProtocolType],
    ) {
        assert_eq!(protocols(app), expected);
    }

    struct ProtocolRow {
        app: TerminalApp,
        best_guess: ProtocolType,
        sixel: &'static [Capability],
        expected: Option<ProtocolType>,
    }

    #[rstest]
    #[case::kitty_confirmed(ProtocolRow {
        app: TerminalApp::Kitty,
        best_guess: ProtocolType::Kitty,
        sixel: &[],
        expected: Some(ProtocolType::Kitty),
    })]
    #[case::ghostty_confirmed(ProtocolRow {
        app: TerminalApp::Ghostty,
        best_guess: ProtocolType::Kitty,
        sixel: &[],
        expected: Some(ProtocolType::Kitty),
    })]
    #[case::iterm2_confirmed(ProtocolRow {
        app: TerminalApp::Iterm2,
        best_guess: ProtocolType::Iterm2,
        sixel: &[],
        expected: Some(ProtocolType::Iterm2),
    })]
    #[case::iterm2_falls_back_to_sixel(ProtocolRow {
        app: TerminalApp::Iterm2,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::sixel_outranks_the_best_guess(ProtocolRow {
        app: TerminalApp::WezTerm,
        best_guess: ProtocolType::Kitty,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::iterm2_unconfirmed(ProtocolRow {
        app: TerminalApp::Iterm2,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[],
        expected: None,
    })]
    #[case::kitty_unconfirmed(ProtocolRow {
        app: TerminalApp::Kitty,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[],
        expected: None,
    })]
    #[case::probe_takes_what_it_got(ProtocolRow {
        app: TerminalApp::Unknown,
        best_guess: ProtocolType::Kitty,
        sixel: &[],
        expected: Some(ProtocolType::Kitty),
    })]
    #[case::probe_takes_sixel(ProtocolRow {
        app: TerminalApp::Unknown,
        best_guess: ProtocolType::Sixel,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::probe_takes_sixel_over_a_halfblocks_guess(ProtocolRow {
        app: TerminalApp::Unknown,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::probe_confirmed_nothing(ProtocolRow {
        app: TerminalApp::Unknown,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[],
        expected: None,
    })]
    #[case::apple_never_gets_one(ProtocolRow {
        app: TerminalApp::Apple,
        best_guess: ProtocolType::Kitty,
        sixel: &[Capability::Sixel],
        expected: None,
    })]
    fn select_protocol_type_picks_only_what_was_confirmed(#[case] row: ProtocolRow) {
        assert_eq!(
            select_protocol_type(protocols(row.app), row.best_guess, row.sixel),
            row.expected
        );
    }

    #[test]
    fn from_environment_is_always_halfblocks_for_every_app() {
        let environment = kitty_term();
        let capabilities = Capabilities::from_environment(&environment);
        assert_eq!(
            capabilities.picker.protocol_type(),
            ProtocolType::Halfblocks
        );
        assert_eq!(capabilities.pixel_path(), PixelPath::Halfblocks);
        assert_eq!(
            capabilities.color_depth,
            ColorDepth::from_term_program(environment.term_program.as_deref())
        );
    }

    #[rstest]
    #[case::halfblocks(ProtocolType::Halfblocks, PixelPath::Halfblocks)]
    #[case::kitty(ProtocolType::Kitty, PixelPath::Protocol)]
    #[case::iterm2(ProtocolType::Iterm2, PixelPath::Protocol)]
    #[case::sixel(ProtocolType::Sixel, PixelPath::Protocol)]
    fn pixel_path_follows_the_picker_protocol(
        #[case] protocol_type: ProtocolType,
        #[case] expected: PixelPath,
    ) {
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(protocol_type);
        let capabilities = Capabilities {
            picker,
            color_depth: ColorDepth::from_term_program(None),
        };
        assert_eq!(capabilities.pixel_path(), expected);
    }

    #[test]
    fn query_is_none_for_apple_terminal() {
        assert!(matches!(query(TerminalApp::Apple), Ok(None)));
    }

    #[test]
    fn cell_aspect_divides_cell_height_by_cell_width() {
        let font_size = FontSize::new(8, 16);
        assert_eq!(cell_aspect(font_size), 2.0);
    }

    #[test]
    fn cell_aspect_falls_back_to_default_when_the_cell_width_is_zero() {
        let font_size = FontSize::new(0, 16);
        assert_eq!(cell_aspect(font_size), DEFAULT_CELL_ASPECT);
    }
}
