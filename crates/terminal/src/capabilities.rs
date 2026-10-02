use ratatui_image::{
    FontSize,
    picker::{Capability, Picker, ProtocolType},
};
use widgets::{ColorDepth, DEFAULT_CELL_ASPECT, PixelPath};

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

    #[cfg(test)]
    fn with_term_program(mut self, term_program: &str) -> Self {
        self.term_program = Some(term_program.into());
        self
    }
    #[cfg(test)]
    fn with_kitty_window_id(mut self, kitty_window_id: &str) -> Self {
        self.kitty_window_id = Some(kitty_window_id.into());
        self
    }
    #[cfg(test)]
    fn with_ghostty_resources_dir(mut self, ghostty_resources_dir: &str) -> Self {
        self.ghostty_resources_dir = Some(ghostty_resources_dir.into());
        self
    }
    #[cfg(test)]
    fn with_wezterm_executable(mut self, wezterm_executable: &str) -> Self {
        self.wezterm_executable = Some(wezterm_executable.into());
        self
    }
    #[cfg(test)]
    fn with_iterm_session_id(mut self, iterm_session_id: &str) -> Self {
        self.iterm_session_id = Some(iterm_session_id.into());
        self
    }
    #[cfg(test)]
    fn with_term(mut self, term: &str) -> Self {
        self.term = Some(term.into());
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Brand {
    Kitty,
    Ghostty,
    Iterm2,
    WezTerm,
    Apple,
    Unknown,
}

impl Brand {
    #[must_use]
    pub fn detect(environment: &TerminalEnvironment) -> Self {
        let program = environment.term_program.as_deref().unwrap_or_default();
        let term = environment.term.as_deref().unwrap_or_default();
        if environment.kitty_window_id.is_some() || term.contains("kitty") {
            Brand::Kitty
        } else if environment.ghostty_resources_dir.is_some()
            || program.eq_ignore_ascii_case("ghostty")
            || term.contains("ghostty")
        {
            Brand::Ghostty
        } else if environment.iterm_session_id.is_some() || program == "iTerm.app" {
            Brand::Iterm2
        } else if environment.wezterm_executable.is_some() || program == "WezTerm" {
            Brand::WezTerm
        } else if program == "Apple_Terminal" {
            Brand::Apple
        } else {
            Brand::Unknown
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    Kgp,
    Iip,
    Sixel,
    Probe,
}

#[must_use]
pub(crate) fn protocols(brand: Brand) -> &'static [Protocol] {
    match brand {
        Brand::Kitty | Brand::Ghostty => &[Protocol::Kgp],
        Brand::Iterm2 | Brand::WezTerm => &[Protocol::Iip, Protocol::Sixel],
        Brand::Apple => &[],
        Brand::Unknown => &[Protocol::Probe],
    }
}

#[derive(Debug)]
pub struct Capabilities {
    pub picker: Picker,
    pub pixel_path: PixelPath,
    pub color_depth: ColorDepth,
}

fn select_protocol_type(
    choices: &[Protocol],
    protocol_type: ProtocolType,
    capabilities: &[Capability],
) -> Option<ProtocolType> {
    choices.iter().find_map(|protocol| match protocol {
        Protocol::Kgp if protocol_type == ProtocolType::Kitty => {
            Some(ProtocolType::Kitty)
        }
        Protocol::Iip if protocol_type == ProtocolType::Iterm2 => {
            Some(ProtocolType::Iterm2)
        }
        Protocol::Sixel
            if capabilities.contains(&Capability::Sixel)
                || protocol_type == ProtocolType::Sixel =>
        {
            Some(ProtocolType::Sixel)
        }
        Protocol::Probe if protocol_type != ProtocolType::Halfblocks => {
            Some(protocol_type)
        }
        Protocol::Kgp | Protocol::Iip | Protocol::Sixel | Protocol::Probe => None,
    })
}

impl Capabilities {
    #[must_use]
    pub fn before_probe(environment: &TerminalEnvironment) -> Self {
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

#[must_use]
pub fn probe(brand: Brand) -> Option<ProbeAnswer> {
    if protocols(brand).is_empty() {
        return None;
    }
    let mut picker = Picker::from_query_stdio().ok()?;
    let confirmed = select_protocol_type(
        protocols(brand),
        picker.protocol_type(),
        picker.capabilities(),
    )
    .unwrap_or(ProtocolType::Halfblocks);
    if confirmed == ProtocolType::Halfblocks {
        return None;
    }
    picker.set_protocol_type(confirmed);
    Some(ProbeAnswer { picker })
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
    use widgets::{ColorDepth, DEFAULT_CELL_ASPECT, PixelPath};

    use crate::capabilities::{
        Brand,
        Capabilities,
        Protocol,
        TerminalEnvironment,
        cell_aspect,
        probe,
        protocols,
        select_protocol_type,
    };

    #[rstest]
    #[case::kitty_by_term(TerminalEnvironment::default().with_term("xterm-kitty"), Brand::Kitty)]
    #[case::kitty_by_window_id(TerminalEnvironment::default().with_kitty_window_id("1"), Brand::Kitty)]
    #[case::ghostty_by_program(TerminalEnvironment::default().with_term_program("ghostty"), Brand::Ghostty)]
    #[case::ghostty_by_resources_dir(
        TerminalEnvironment::default().with_ghostty_resources_dir("/tmp"),
        Brand::Ghostty
    )]
    #[case::iterm2_by_program(TerminalEnvironment::default().with_term_program("iTerm.app"), Brand::Iterm2)]
    #[case::iterm2_by_session_id(TerminalEnvironment::default().with_iterm_session_id("id"), Brand::Iterm2)]
    #[case::wezterm_by_program(TerminalEnvironment::default().with_term_program("WezTerm"), Brand::WezTerm)]
    #[case::wezterm_by_executable(
        TerminalEnvironment::default().with_wezterm_executable("wezterm"),
        Brand::WezTerm
    )]
    #[case::apple_by_program(
        TerminalEnvironment::default().with_term_program("Apple_Terminal"),
        Brand::Apple
    )]
    #[case::nothing_named(TerminalEnvironment::default(), Brand::Unknown)]
    fn detect_names_the_terminal(
        #[case] environment: TerminalEnvironment,
        #[case] expected: Brand,
    ) {
        assert_eq!(Brand::detect(&environment), expected);
    }

    #[rstest]
    #[case::kitty(Brand::Kitty, &[Protocol::Kgp])]
    #[case::ghostty(Brand::Ghostty, &[Protocol::Kgp])]
    #[case::iterm2(Brand::Iterm2, &[Protocol::Iip, Protocol::Sixel])]
    #[case::wezterm(Brand::WezTerm, &[Protocol::Iip, Protocol::Sixel])]
    #[case::apple(Brand::Apple, &[])]
    #[case::unknown(Brand::Unknown, &[Protocol::Probe])]
    fn each_brand_names_its_protocols(
        #[case] brand: Brand,
        #[case] expected: &[Protocol],
    ) {
        assert_eq!(protocols(brand), expected);
    }

    struct ProtocolPick {
        brand: Brand,
        best_guess: ProtocolType,
        sixel: &'static [Capability],
        expected: Option<ProtocolType>,
    }

    #[rstest]
    #[case::kitty_confirmed(ProtocolPick {
        brand: Brand::Kitty,
        best_guess: ProtocolType::Kitty,
        sixel: &[],
        expected: Some(ProtocolType::Kitty),
    })]
    #[case::ghostty_confirmed(ProtocolPick {
        brand: Brand::Ghostty,
        best_guess: ProtocolType::Kitty,
        sixel: &[],
        expected: Some(ProtocolType::Kitty),
    })]
    #[case::iterm2_confirmed(ProtocolPick {
        brand: Brand::Iterm2,
        best_guess: ProtocolType::Iterm2,
        sixel: &[],
        expected: Some(ProtocolType::Iterm2),
    })]
    #[case::iterm2_falls_back_to_sixel(ProtocolPick {
        brand: Brand::Iterm2,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::sixel_outranks_the_best_guess(ProtocolPick {
        brand: Brand::WezTerm,
        best_guess: ProtocolType::Kitty,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::iterm2_unconfirmed(ProtocolPick {
        brand: Brand::Iterm2,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[],
        expected: None,
    })]
    #[case::kitty_unconfirmed(ProtocolPick {
        brand: Brand::Kitty,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[],
        expected: None,
    })]
    #[case::probe_takes_what_it_got(ProtocolPick {
        brand: Brand::Unknown,
        best_guess: ProtocolType::Kitty,
        sixel: &[],
        expected: Some(ProtocolType::Kitty),
    })]
    #[case::probe_takes_sixel(ProtocolPick {
        brand: Brand::Unknown,
        best_guess: ProtocolType::Sixel,
        sixel: &[Capability::Sixel],
        expected: Some(ProtocolType::Sixel),
    })]
    #[case::probe_confirmed_nothing(ProtocolPick {
        brand: Brand::Unknown,
        best_guess: ProtocolType::Halfblocks,
        sixel: &[],
        expected: None,
    })]
    #[case::apple_never_gets_one(ProtocolPick {
        brand: Brand::Apple,
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
        let environment = TerminalEnvironment::default().with_term("xterm-kitty");
        let capabilities = Capabilities::before_probe(&environment);
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
        assert!(probe(Brand::Apple).is_none());
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
