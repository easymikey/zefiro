use std::time::Duration;

use runtime::shell::Frame;
use widgets::scene::{Scene, ScenePresentation};

use crate::shell::{motion::Motion, presentation::ShellPresentation};

pub(crate) fn scene<'a>(
    frame: &Frame<'a>,
    presentation: &'a ShellPresentation,
    motion: &Motion,
) -> Scene<'a> {
    Scene::from_model(
        frame.model,
        ScenePresentation {
            appearance: &presentation.appearance,
            theme: &presentation.theme,
            color_depth: presentation.color_depth,
            spectrum: &presentation.spectrum,
            pixel_path: presentation.pixel_path,
            cell_aspect: presentation.cell_aspect,
            clock: motion
                .first_paint
                .map_or(Duration::ZERO, |first| frame.now.elapsed_since(first)),
            now: frame.now,
            home: presentation.home.as_deref(),
            key_hint_chords: &presentation.key_hint_chords,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use audio::tap::SpectrumTap;
    use kernel::domain::{model::Model, time::Moment};
    use ratatui::layout::Rect;
    use runtime::shell::Frame;
    use widgets::screen::frame_layout::FrameLayout;

    use crate::shell::{motion::Motion, presentation::test_presentation, view::scene};

    #[test]
    fn a_view_of_a_stock_model_lays_out_the_whole_frame() {
        let model = Model::default();
        let (_senders, latest, _doorbell) = runtime::latest::latest_channels();
        let spectrum = SpectrumTap::silent();
        let area = Rect::new(0, 0, 80, 24);
        let presentation = test_presentation();
        let motion = Motion {
            area,
            ..Motion::default()
        };
        let frame = Frame {
            model: &model,
            spectrum: &spectrum,
            latest: &latest,
            now: Moment::new(Duration::from_secs(5)),
        };

        let layout =
            FrameLayout::from_scene(&scene(&frame, &presentation, &motion), area);

        assert_eq!(layout.screen, area);
        insta::assert_debug_snapshot!(layout);
    }
}
