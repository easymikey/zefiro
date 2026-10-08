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
            since_first_paint: motion.paint_clock.elapsed(frame.now),
            now: frame.now,
            home_dir: presentation.home_dir.as_deref(),
            key_hint_chords: &presentation.key_hint_chords,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use audio::tap::SpectrumTap;
    use config::{
        embedded_theme::{STOCK_THEME, STOCK_THEME_TEXT},
        theme_file::parse_theme,
    };
    use kernel::domain::{model::Model, time::Moment};
    use ratatui::layout::Rect;
    use runtime::shell::Frame;
    use widgets::{
        scene::PixelPath,
        screen::frame_layout::FrameLayout,
        theme::rgb::ColorDepth,
    };

    use crate::shell::{
        motion::Motion,
        presentation::{ShellPresentation, theme},
        view::scene,
    };

    #[test]
    fn a_view_of_a_stock_model_lays_out_the_whole_frame() {
        let model = Model::default();
        let (_senders, latest_receivers, _doorbell) =
            runtime::latest::latest_channels();
        let spectrum_tap = SpectrumTap::silent();
        let area = Rect::new(0, 0, 80, 24);
        let presentation = ShellPresentation::new(
            theme(parse_theme(STOCK_THEME_TEXT, STOCK_THEME).unwrap()),
            PixelPath::Halfblocks,
            ColorDepth::TrueColor,
        );
        let motion = Motion {
            area,
            ..Motion::default()
        };
        let frame = Frame {
            model: &model,
            spectrum_tap: &spectrum_tap,
            latest_receivers: &latest_receivers,
            now: Moment::new(Duration::from_secs(5)),
        };

        let layout =
            FrameLayout::from_scene(&scene(&frame, &presentation, &motion), area);

        assert_eq!(layout.screen, area);
        insta::assert_debug_snapshot!(layout);
    }
}
