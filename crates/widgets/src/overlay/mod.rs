pub(crate) mod confirm_delete;
pub(crate) mod help;
pub(crate) mod history;
pub(crate) mod jump_to_time;
pub(crate) mod layer;
pub(crate) mod modal;
pub(crate) mod music_dir;
pub(crate) mod search;
pub(crate) mod settings;
pub(crate) mod track_details;

#[cfg(test)]
pub(crate) fn rendered_canvas(
    width: u16,
    height: u16,
    paint: impl FnOnce(crate::primitive::canvas::Canvas<'_>),
) -> ratatui::backend::TestBackend {
    crate::test_support::rendered(width, height, |frame| {
        let area = frame.area();
        paint(crate::primitive::canvas::Canvas {
            area,
            buffer: frame.buffer_mut(),
        });
    })
}
