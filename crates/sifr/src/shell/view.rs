use std::path::PathBuf;

use config::AppearanceFile;
use runtime::Frame;
use widgets::{ColorDepth, FrameLayout, PixelPath, Scene, Theme};

use crate::shell::motion::Motion;

pub(crate) struct Presentation {
    pub(in crate::shell) theme: Theme,
    pub(in crate::shell) appearance: AppearanceFile,
    pub(in crate::shell) pixel_path: PixelPath,
    pub(in crate::shell) color_depth: ColorDepth,
    pub(in crate::shell) cell_aspect: f32,
    pub(in crate::shell) home: Option<PathBuf>,
    pub(in crate::shell) music_dir: PathBuf,
    pub(in crate::shell) music_dir_label: String,
}

pub(crate) struct LaidOutScene<'a> {
    pub(crate) scene: Scene<'a>,
    pub(crate) layout: FrameLayout,
}

pub(crate) fn view<'a>(
    frame: &Frame<'a>,
    presentation: &'a Presentation,
    motion: &'a Motion,
) -> LaidOutScene<'a> {
    let scene = Scene {
        model: frame.model,
        theme: &presentation.theme,
        color_depth: presentation.color_depth,
        appearance: &presentation.appearance,
        bindings: frame.model.workspace.keymap.bindings(),
        spectrum: motion.spectrum_smoothing.bands(),
        pixel_path: presentation.pixel_path,
        cell_aspect: presentation.cell_aspect,
        clock: frame.now.elapsed_since(motion.first_paint),
        now: frame.now,
        music_dir: &presentation.music_dir_label,
        sleep_left: frame
            .sleep_deadline
            .map(|deadline| deadline.elapsed_since(frame.now)),
    };
    let layout = FrameLayout::new(&scene.layout_parts(), motion.area);
    LaidOutScene { scene, layout }
}

#[cfg(test)]
pub(in crate::shell) fn test_presentation() -> Presentation {
    Presentation {
        theme: Theme::from(crate::startup::fallback_theme_file()),
        appearance: AppearanceFile::default(),
        pixel_path: PixelPath::Halfblocks,
        color_depth: ColorDepth::TrueColor,
        cell_aspect: widgets::DEFAULT_CELL_ASPECT,
        home: None,
        music_dir: PathBuf::new(),
        music_dir_label: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use audio::SpectrumTap;
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
