use std::{path::PathBuf, time::Duration};

use runtime::Frame;
use widgets::{
    ColorDepth,
    FrameLayout,
    PixelPath,
    Scene,
    ScenePresentation,
    Spectrum,
    Theme,
};

use crate::shell::motion::Motion;

pub(crate) struct ShellPresentation {
    pub(in crate::shell) theme: Theme,
    pub(in crate::shell) pixel_path: PixelPath,
    pub(in crate::shell) color_depth: ColorDepth,
    pub(in crate::shell) cell_aspect: f32,
    pub(in crate::shell) home: Option<PathBuf>,
    pub(in crate::shell) spectrum: Spectrum,
}

pub(crate) struct LaidOutScene<'a> {
    pub(crate) scene: Scene<'a>,
    pub(crate) layout: FrameLayout,
}

pub(crate) fn view<'a>(
    frame: &Frame<'a>,
    presentation: &'a ShellPresentation,
    motion: &Motion,
) -> LaidOutScene<'a> {
    let scene = scene(frame, presentation, motion);
    let layout = FrameLayout::from_scene(&scene, motion.area);
    LaidOutScene { scene, layout }
}

pub(crate) fn scene<'a>(
    frame: &Frame<'a>,
    presentation: &'a ShellPresentation,
    motion: &Motion,
) -> Scene<'a> {
    Scene::from_model(
        frame.model,
        ScenePresentation {
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
            sleep_left: frame
                .sleep_deadline
                .map(|deadline| deadline.elapsed_since(frame.now)),
        },
    )
}

#[cfg(test)]
pub(in crate::shell) fn test_presentation() -> ShellPresentation {
    ShellPresentation {
        theme: crate::startup::theme(crate::startup::fallback_theme()),
        pixel_path: PixelPath::Halfblocks,
        color_depth: ColorDepth::TrueColor,
        cell_aspect: widgets::DEFAULT_CELL_ASPECT,
        home: None,
        spectrum: [0.0; widgets::SPECTRUM_BANDS],
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use audio::tap::SpectrumTap;
    use kernel::{Moment, domain::Model};
    use ratatui::layout::Rect;
    use runtime::Frame;

    use crate::shell::{
        motion::Motion,
        view::{LaidOutScene, test_presentation, view},
    };

    #[test]
    fn a_view_of_a_stock_model_lays_out_the_whole_frame() {
        let model = Model::default();
        let (_senders, latest, _doorbell) = runtime::latest_channels();
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
            sleep_deadline: None,
            now: Moment::new(Duration::from_secs(5)),
        };

        let LaidOutScene { layout, .. } = view(&frame, &presentation, &motion);

        assert_eq!(layout.screen, area);
        insta::assert_debug_snapshot!(layout);
    }
}
