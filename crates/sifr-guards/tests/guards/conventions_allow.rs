// GUARD: allowlist rows `rule path name..` of the conventions guard.

use crate::guards::support::Allow;

pub(crate) fn allow_rows(rule: &str) -> Vec<Allow> {
    let row = |cells: &'static str| {
        let mut cells = cells.split_whitespace();
        let (id, path) = (cells.next(), cells.next().unwrap_or(""));
        let names = cells.filter(move |_| id == Some(rule));
        names.map(move |name| Allow::new(path, name, "migrates with its crate step"))
    };
    ALLOW.lines().flat_map(row).collect()
}

const ALLOW: &str = "\
type_words macos/src/controls.rs CommandOutcome
type_words runtime/src/library/cover.rs CoverOutcome CachedOutcome
type_words sifr/src/startup.rs Look
fn_words macos/src/controls.rs outcome
fn_words macos/src/cover.rs apply
fn_words macos/src/hardware_state.rs apply
fn_words macos/src/macos_loop.rs perform apply_cover_effect
fn_words runtime/src/config/machine.rs apply_save_result
fn_words runtime/src/config/session.rs watch_failure
fn_words runtime/src/event_loop.rs dispatch_arrival
fn_words runtime/src/library/cover.rs from_outcome into_outcome
fn_words runtime/src/library/machine.rs watch_failure
fn_words sifr/src/startup.rs with_look
machine_state runtime/src/config/machine.rs ConfigState
machine_state runtime/src/library/machine.rs LibraryState
loop kernel/src/domain/player.rs AbLoop
loop macos/src/macos_loop.rs MacosLoop
loop runtime/src/config/session.rs ConfigLoop
loop runtime/src/library/driver.rs LibraryLoop
loop runtime/src/library/worker.rs CoverLoop
colors config/src/theme_file.rs ThemeColors
colors widgets/src/milkdrop/mod.rs MilkdropColors
colors widgets/src/overlay/help/columns.rs HelpColors
colors widgets/src/overlay/modal/metrics.rs ModalRowColors
colors widgets/src/primitive/bar.rs HudProgressColors
colors widgets/src/primitive/chip.rs ChipColors
colors widgets/src/primitive/track_row.rs RowColors
colors widgets/src/status_line.rs StatusLineColors
toml_shape config/src/appearance_file.rs CoverConfig CardConfig ProgressConfig WindowConfig LayoutConfig
toml_shape config/src/appearance_file.rs AppearanceFile
toml_shape config/src/config_file.rs AudioConfig
toml_shape config/src/keymap.rs KeymapFile
toml_shape config/src/theme_file.rs ThemeFile
request runtime/src/library/cover.rs CoverRequest
single_field runtime/src/config/machine.rs ConfigMessage::SaveDue
single_field runtime/src/library/machine.rs LibraryMessage::EventsOverflowed
single_field runtime/src/trace.rs TraceEntry::JoinFailed TraceEntry::RestartFailed TraceEntry::TimerOverflow
single_field widgets/src/geometry.rs CoverSizing::Auto
single_field widgets/src/overlay/modal/placement.rs OverlayContainer::Modal
result_alias runtime/src/config/write.rs SaveResult
result_alias runtime/src/driver.rs Exit
render widgets/src/braille.rs render_meter
render widgets/src/card/mod.rs render_in
render widgets/src/overlay/help/mod.rs render_help_columns render_in
render widgets/src/overlay/history.rs render_in render_rows
render widgets/src/overlay/layer.rs render_in
render widgets/src/overlay/mod.rs rendered_canvas
render widgets/src/overlay/modal/prompt.rs render_in
render widgets/src/overlay/search/matches.rs render_match_pane render_match_rows
render widgets/src/overlay/search/mod.rs render_in render_pane render_modal render_query
render widgets/src/overlay/settings/mod.rs render_in render_rows
render widgets/src/overlay/track_details.rs render_in
render widgets/src/playlist/pane.rs render_in
render widgets/src/primitive/list_chrome.rs render_scrollbar
render widgets/src/screen/root.rs render_lists render_layers
render widgets/src/toast.rs render_in
let_underscore macos/src/core_audio.rs discard
let_underscore runtime/src/config/machine.rs discard
let_underscore runtime/src/config/session.rs discard
let_underscore runtime/src/driver.rs discard
let_underscore runtime/src/event_loop.rs discard
let_underscore runtime/src/latest.rs discard
let_underscore runtime/src/library/machine.rs discard
let_underscore runtime/src/runtime.rs discard
let_underscore runtime/src/spawn.rs discard
let_underscore sifr/src/shell/frame_due.rs discard
let_underscore sifr/src/shell/motion.rs discard
let_underscore terminal/src/session.rs discard
let_underscore terminal/src/window_colors.rs discard
let_underscore terminal/tests/unit/cover.rs discard
let_underscore widgets/src/overlay/confirm_delete.rs discard
let_underscore widgets/src/overlay/help/mod.rs discard
let_underscore widgets/src/overlay/history.rs discard
let_underscore widgets/src/overlay/jump_to_time.rs discard
let_underscore widgets/src/overlay/layer.rs discard
let_underscore widgets/src/overlay/music_dir.rs discard
let_underscore widgets/src/overlay/search/mod.rs discard
let_underscore widgets/src/overlay/settings/mod.rs discard
let_underscore widgets/src/overlay/track_details.rs discard
let_underscore widgets/src/playlist/pane.rs discard
let_underscore widgets/src/spectrum.rs discard
let_underscore widgets/tests/unit/animation_actions.rs discard
let_underscore widgets/tests/unit/animation_catalogue.rs discard
let_underscore widgets/tests/unit/animation_stage.rs discard
let_underscore widgets/tests/unit/animation_volume.rs discard";
