use ratatui_image::{
    FontSize,
    picker::{Capability, Picker, ProtocolType},
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
    pub fn detect(environment: &TerminalEnvironment) -> Self {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PixelProtocol {
    Kitty,
    Iterm2,
    Sixel,
    Query,
}

#[must_use]
pub(crate) fn protocols(brand: TerminalApp) -> &'static [PixelProtocol] {
    match brand {
        TerminalApp::Kitty | TerminalApp::Ghostty => &[PixelProtocol::Kitty],
        TerminalApp::Iterm2 | TerminalApp::WezTerm => {
            &[PixelProtocol::Iterm2, PixelProtocol::Sixel]
        }
        TerminalApp::Apple => &[],
        TerminalApp::Unknown => &[PixelProtocol::Query],
    }
}

#[derive(Debug)]
pub struct Capabilities {
    pub picker: Picker,
    pub pixel_path: PixelPath,
    pub color_depth: ColorDepth,
}

fn select_protocol_type(
    choices: &[PixelProtocol],
    protocol_type: ProtocolType,
    capabilities: &[Capability],
) -> Option<ProtocolType> {
    choices.iter().find_map(|protocol| match protocol {
        PixelProtocol::Kitty if protocol_type == ProtocolType::Kitty => {
            Some(ProtocolType::Kitty)
        }
        PixelProtocol::Iterm2 if protocol_type == ProtocolType::Iterm2 => {
            Some(ProtocolType::Iterm2)
        }
        PixelProtocol::Sixel
            if capabilities.contains(&Capability::Sixel)
                || protocol_type == ProtocolType::Sixel =>
        {
            Some(ProtocolType::Sixel)
        }
        PixelProtocol::Query if protocol_type != ProtocolType::Halfblocks => {
            Some(protocol_type)
        }
        PixelProtocol::Kitty
        | PixelProtocol::Iterm2
        | PixelProtocol::Sixel
        | PixelProtocol::Query => None,
    })
}

impl Capabilities {
    #[must_use]
    pub fn from_environment(environment: &TerminalEnvironment) -> Self {
        Capabilities {
            picker: Picker::halfblocks(),
            pixel_path: PixelPath::Halfblocks,
            color_depth: ColorDepth::detect(environment.term_program.as_deref()),
        }
    }
}

#[derive(Debug)]
pub struct ProbeAnswer {
    pub picker: Picker,
}

pub fn probe(brand: TerminalApp) -> Result<Option<ProbeAnswer>, Error> {
    if protocols(brand).is_empty() {
        return Ok(None);
    }
    let mut picker = Picker::from_query_stdio().map_err(Error::Probe)?;
    let confirmed = select_protocol_type(
        protocols(brand),
        picker.protocol_type(),
        picker.capabilities(),
    )
    .unwrap_or(ProtocolType::Halfblocks);
    if confirmed == ProtocolType::Halfblocks {
        return Ok(None);
    }
    picker.set_protocol_type(confirmed);
    Ok(Some(ProbeAnswer { picker }))
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
        picker::{Capability, ProtocolType},
    };
    use rstest::rstest;
    use widgets::{
        geometry::DEFAULT_CELL_ASPECT,
        scene::PixelPath,
        theme::rgb::ColorDepth,
    };

    use crate::capabilities::{
        Capabilities,
        PixelProtocol,
        TerminalApp,
        TerminalEnvironment,
        cell_aspect,
        probe,
        protocols,
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
    fn detect_names_the_terminal(
        #[case] environment: TerminalEnvironment,
        #[case] expected: TerminalApp,
    ) {
        assert_eq!(TerminalApp::detect(&environment), expected);
    }

    #[rstest]
    #[case::kitty(TerminalApp::Kitty, &[PixelProtocol::Kitty])]
    #[case::ghostty(TerminalApp::Ghostty, &[PixelProtocol::Kitty])]
    #[case::iterm2(TerminalApp::Iterm2, &[PixelProtocol::Iterm2, PixelProtocol::Sixel])]
    #[case::wezterm(TerminalApp::WezTerm, &[PixelProtocol::Iterm2, PixelProtocol::Sixel])]
    #[case::apple(TerminalApp::Apple, &[])]
    #[case::unknown(TerminalApp::Unknown, &[PixelProtocol::Query])]
    fn each_brand_names_its_protocols(
        #[case] brand: TerminalApp,
        #[case] expected: &[PixelProtocol],
    ) {
        assert_eq!(protocols(brand), expected);
    }

    struct ProtocolPick {
        brand: TerminalApp,
        best_guess: ProtocolType,
        sixel: &'static [Capability],
        expected: Option<ProtocolType>,
    }

    #[rstest]
    #[case::kitty_confirmed(ProtocolPick {
        brand: TerminalApp::Kitty,
        best_guess: ProtocolType::Kitty,
        sixel: &[],
        expected: Some(ProtocolType::Kitty),
    })]
    #[case::ghostty_confirmed(ProtocolPick {
        brand: TerminalApp::Ghostty,
        best_guess: ProtocolType::Kitty,
        sixel: &[],
        expected: Some(ProtocolType::Kitty),
    })]
    #[case::iterm2_confirmed(ProtocolPick {
        brand: TerminalApp::Iterm2,
        best_guess: ProtocolType::Iterm2,
        sixel: &[],
        expected: Some(ProtocolType::Iterm2),
    })]
    #[case::iterm2_falls_back_to_sixel(ProtocolPick {
        brand: TerminalApp::Iterm2,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::sixel_outranks_the_best_guess(ProtocolPick {
        brand: TerminalApp::WezTerm,
        best_guess: ProtocolType::Kitty,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::iterm2_unconfirmed(ProtocolPick {
        brand: TerminalApp::Iterm2,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[],
        expected: None,
    })]
    #[case::kitty_unconfirmed(ProtocolPick {
        brand: TerminalApp::Kitty,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[],
        expected: None,
    })]
    #[case::probe_takes_what_it_got(ProtocolPick {
        brand: TerminalApp::Unknown,
        best_guess: ProtocolType::Kitty,
        sixel: &[],
        expected: Some(ProtocolType::Kitty),
    })]
    #[case::probe_takes_sixel(ProtocolPick {
        brand: TerminalApp::Unknown,
        best_guess: ProtocolType::Sixel,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::probe_confirmed_nothing(ProtocolPick {
        brand: TerminalApp::Unknown,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[],
        expected: None,
    })]
    #[case::apple_never_gets_one(ProtocolPick {
        brand: TerminalApp::Apple,
        best_guess: ProtocolType::Kitty,
        sixel: &[Capability::Sixel],
        expected: None,
    })]
    fn select_protocol_type_picks_only_what_was_confirmed(#[case] row: ProtocolPick) {
        assert_eq!(
            select_protocol_type(protocols(row.brand), row.best_guess, row.sixel),
            row.expected
        );
    }

    #[test]
    fn before_probe_is_always_halfblocks_regardless_of_brand() {
        let environment = kitty_term();
        let capabilities = Capabilities::from_environment(&environment);
        assert_eq!(
            capabilities.picker.protocol_type(),
            ProtocolType::Halfblocks
        );
        assert_eq!(capabilities.pixel_path, PixelPath::Halfblocks);
        assert_eq!(
            capabilities.color_depth,
            ColorDepth::detect(environment.term_program.as_deref())
        );
    }

    #[test]
    fn probe_is_none_for_apple_terminal() {
        assert!(matches!(probe(TerminalApp::Apple), Ok(None)));
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
