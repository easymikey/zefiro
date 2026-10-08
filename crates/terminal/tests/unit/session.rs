use std::{
    io::{self, Write},
    panic,
    thread,
};

use terminal::session::install_panic_hook;

use crate::{
    support::{child_stdout, in_child},
    unit::window_colors::RESET_SEQUENCE,
};

const PANIC_HOOK: &str =
    "unit::session::the_panic_hook_restores_the_terminal_only_for_the_painting_thread";

const OTHER_THREAD_PANICKED: &str = "the other thread has panicked\n";

const LEAVE_ALTERNATE_SCREEN_AND_SHOW: &str = "\x1b[?1049l\x1b[?25h";

#[test]
fn the_panic_hook_restores_the_terminal_only_for_the_painting_thread() {
    if in_child() {
        install_panic_hook();
        let other = thread::spawn(|| panic!("a worker thread")).join();
        io::stdout()
            .write_all(OTHER_THREAD_PANICKED.as_bytes())
            .unwrap();
        let painting = panic::catch_unwind(|| panic!("the painting thread"));
        assert!(other.is_err() && painting.is_err());
    } else {
        let written = child_stdout(PANIC_HOOK, &[]).unwrap();
        let (before, after) = written.split_once(OTHER_THREAD_PANICKED).unwrap();
        assert!(
            !before.contains(LEAVE_ALTERNATE_SCREEN_AND_SHOW),
            "{before:?}"
        );
        assert!(
            after.contains(&format!(
                "{RESET_SEQUENCE}{LEAVE_ALTERNATE_SCREEN_AND_SHOW}"
            )),
            "{after:?}"
        );
    }
}
