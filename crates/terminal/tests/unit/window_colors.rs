use terminal::window_colors::reset_window_colors;

use crate::support::{child_stdout, in_child};

const RESET: &str =
    "unit::window_colors::reset_window_colors_writes_the_three_resets_to_stdout";

pub(crate) const RESET_SEQUENCE: &str = "\x1b]111\x07\x1b]110\x07\x1b]112\x07";

#[test]
fn reset_window_colors_writes_the_three_resets_to_stdout() {
    if in_child() {
        reset_window_colors().unwrap();
    } else {
        let written = child_stdout(RESET, &[]).unwrap();
        assert!(written.contains(RESET_SEQUENCE), "{written:?}");
    }
}
