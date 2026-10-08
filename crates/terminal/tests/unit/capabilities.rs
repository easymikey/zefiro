use rstest::rstest;
use terminal::capabilities::{TerminalApp, TerminalEnvironment};

use crate::support::{child_stdout, in_child};

const CURRENT_APP: &str =
    "unit::capabilities::the_current_environment_names_its_terminal_app";

#[test]
fn the_current_environment_names_its_terminal_app() {
    if in_child() {
        println!(
            "{:?}",
            TerminalApp::from_environment(&TerminalEnvironment::current())
        );
    }
}

#[rstest]
#[case::term_program(("TERM_PROGRAM", "Apple_Terminal"), TerminalApp::Apple)]
#[case::kitty_window_id(("KITTY_WINDOW_ID", "1"), TerminalApp::Kitty)]
#[case::ghostty_resources_dir(("GHOSTTY_RESOURCES_DIR", "/Applications/Ghostty.app"), TerminalApp::Ghostty)]
#[case::iterm_session_id(("ITERM_SESSION_ID", "w0t0p0"), TerminalApp::Iterm2)]
#[case::wezterm_executable(("WEZTERM_EXECUTABLE", "/usr/local/bin/wezterm"), TerminalApp::WezTerm)]
#[case::term(("TERM", "xterm-kitty"), TerminalApp::Kitty)]
fn current_reads_each_terminal_variable_of_the_process(
    #[case] variable: (&str, &str),
    #[case] app: TerminalApp,
) {
    let written = child_stdout(CURRENT_APP, &[variable]).unwrap();
    assert!(written.contains(&format!("{app:?}\n")), "{written:?}");
}
