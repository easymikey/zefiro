use std::{path::PathBuf, time::Duration};

use config::{AppearanceFile, Hex, ThemeColors, ThemeFile};
use kernel::Moment;
use runtime::View;
use widgets::{CellAspect, ColorDepth, FrameLayout, PixelPath, Scene, Theme};

use crate::shell::motion::Motion;

pub(crate) struct Presentation {
    pub(in crate::shell) theme: Theme,
    pub(in crate::shell) appearance: AppearanceFile,
    pub(in crate::shell) pixel_path: PixelPath,
    pub(in crate::shell) color_depth: ColorDepth,
    pub(in crate::shell) cell_aspect: CellAspect,
    pub(in crate::shell) home: Option<PathBuf>,
    pub(in crate::shell) music_dir: PathBuf,
    pub(in crate::shell) music_dir_display: String,
}

pub(in crate::shell) enum Update {
    Theme(ThemeFile),
    Appearance(AppearanceFile),
}

pub(in crate::shell) fn install(
    theme: &mut Theme,
    appearance: &mut AppearanceFile,
    update: Update,
) {
    match update {
        Update::Theme(file) => *theme = Theme::from(file),
        Update::Appearance(file) => *appearance = file,
    }
}

pub(crate) struct Frame<'a> {
    pub(crate) scene: Scene<'a>,
    pub(crate) layout: FrameLayout,
}

pub(crate) fn view<'a>(
    view: &View<'a>,
    presentation: &'a Presentation,
    motion: &'a Motion,
) -> Frame<'a> {
    let scene = Scene {
        model: view.model,
        theme: &presentation.theme,
        color_depth: presentation.color_depth,
        appearance: &presentation.appearance,
        bindings: view.model.workspace.bindings.as_slice(),
        spectrum: motion.spectrum.current_bands(),
        pixel_path: presentation.pixel_path,
        cell_aspect: presentation.cell_aspect,
        clock: animation_clock(motion.started, view.now),
        now: view.now,
        music_dir: &presentation.music_dir_display,
        sleep_left: sleep_left(view.sleep_deadline, view.now),
    };
    let layout = FrameLayout::new(&scene.layout_inputs(), motion.area);
    Frame { scene, layout }
}

fn animation_clock(started: Moment, now: Moment) -> Duration {
    now.elapsed_since(started)
}

fn sleep_left(sleep_deadline: Option<Moment>, now: Moment) -> Option<Duration> {
    sleep_deadline.map(|deadline| deadline.elapsed_since(now))
}

pub(crate) fn fallback_theme_file() -> ThemeFile {
    ThemeFile {
        name: "fallback".to_string(),
        colors: ThemeColors {
            background: Hex([0, 0, 0]),
            foreground: Hex([0xff, 0xff, 0xff]),
            bright_foreground: Hex([0xff, 0xff, 0xff]),
            accent: Hex([0xff, 0xff, 0xff]),
            green: Hex([0, 0xff, 0]),
            yellow: Hex([0xff, 0xff, 0]),
            red: Hex([0xff, 0, 0]),
            window_background: None,
        },
        scanning_label: "scanning…".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use audio::SpectrumTap;
    use config::{AppearanceFile, Hex, ThemeColors, ThemeFile};
    use kernel::{Moment, domain::Model};
    use ratatui::layout::Rect;
    use rstest::rstest;
    use runtime::View;
    use widgets::{
        CellAspect,
        ColorDepth,
        FrameLayout,
        PixelPath,
        SPECTRUM_BANDS,
        Scene,
        Theme,
    };

    use crate::shell::{
        motion::Motion,
        view::{
            Frame,
            Presentation,
            Update,
            animation_clock,
            fallback_theme_file,
            install,
            sleep_left,
            view,
        },
    };

    #[test]
    fn a_view_of_a_stock_model_lays_out_the_whole_frame() {
        let model = Model::default();
        let (_writers, cells, _doorbell) = runtime::cells();
        let spectrum = SpectrumTap::silent();
        let area = Rect::new(0, 0, 80, 24);
        let presentation = Presentation {
            theme: Theme::from(fallback_theme_file()),
            appearance: AppearanceFile::default(),
            pixel_path: PixelPath::Halfblocks,
            color_depth: ColorDepth::TrueColor,
            cell_aspect: CellAspect::default(),
            home: None,
            music_dir: PathBuf::new(),
            music_dir_display: String::new(),
        };
        let motion = Motion {
            area,
            ..Motion::default()
        };
        let stock = View {
            model: &model,
            spectrum: &spectrum,
            cells: &cells,
            sleep_deadline: None,
            now: Moment::new(Duration::from_secs(5)),
        };

        let Frame { layout, .. } = view(&stock, &presentation, &motion);

        assert_eq!(layout.screen, area);
        insta::assert_debug_snapshot!(layout);
    }

    fn theme_file(name: &str) -> ThemeFile {
        ThemeFile {
            name: name.to_string(),
            colors: ThemeColors {
                background: Hex([0, 0, 0]),
                foreground: Hex([1, 1, 1]),
                bright_foreground: Hex([2, 2, 2]),
                accent: Hex([3, 3, 3]),
                green: Hex([0, 0xff, 0]),
                yellow: Hex([0xff, 0xff, 0]),
                red: Hex([0xff, 0, 0]),
                window_background: None,
            },
            scanning_label: "scanning…".to_string(),
        }
    }

    #[rstest]
    fn a_theme_reload_installs_the_parsed_theme() {
        let mut theme = Theme::from(theme_file("before"));
        let mut appearance = AppearanceFile::default();

        install(
            &mut theme,
            &mut appearance,
            Update::Theme(theme_file("after")),
        );

        assert_eq!(theme.name, "after");
    }

    #[rstest]
    fn an_appearance_reload_replaces_the_appearance_file() {
        let mut theme = Theme::from(theme_file("noir"));
        let mut appearance = AppearanceFile::default();
        let mut replacement = AppearanceFile::default();
        replacement.cover.size_px = 512;

        install(&mut theme, &mut appearance, Update::Appearance(replacement));

        assert_eq!(appearance.cover.size_px, 512);
    }

    #[test]
    fn animation_clock_is_view_time_since_first_paint() {
        let started = Moment::new(Duration::from_secs(10));
        let now = Moment::new(Duration::from_secs(13));

        assert_eq!(animation_clock(started, now), Duration::from_secs(3));
    }

    #[test]
    fn no_deadline_has_no_sleep_left() {
        let now = Moment::new(Duration::from_secs(10));

        assert_eq!(sleep_left(None, now), None);
    }

    #[test]
    fn a_future_deadline_counts_down_to_it() {
        let now = Moment::new(Duration::from_secs(10));
        let deadline = Moment::new(now.since_epoch() + Duration::from_secs(90));

        assert_eq!(
            sleep_left(Some(deadline), now),
            Some(Duration::from_secs(90))
        );
    }

    #[test]
    fn a_past_deadline_has_no_time_left() {
        let now = Moment::new(Duration::from_secs(10));
        let deadline = Moment::new(now.since_epoch() - Duration::from_secs(1));

        assert_eq!(sleep_left(Some(deadline), now), Some(Duration::ZERO));
    }

    #[test]
    fn view_is_pure() {
        let model = Model::default();
        let theme = Theme::from(fallback_theme_file());
        let appearance = AppearanceFile::default();
        let bands: widgets::Spectrum = [0.0; SPECTRUM_BANDS];
        let now = Moment::new(Duration::from_secs(5));
        let area = Rect::new(0, 0, 80, 24);
        let scene = |at: Moment| Scene {
            model: &model,
            theme: &theme,
            color_depth: ColorDepth::TrueColor,
            appearance: &appearance,
            bindings: &[],
            spectrum: &bands,
            pixel_path: PixelPath::Halfblocks,
            cell_aspect: CellAspect::default(),
            clock: Duration::ZERO,
            now: at,
            music_dir: "",
            sleep_left: None,
        };

        let first = scene(now);
        let second = scene(now);

        assert_eq!(
            FrameLayout::new(&first.layout_inputs(), area),
            FrameLayout::new(&second.layout_inputs(), area)
        );
        assert!(std::ptr::eq(first.model, second.model));
        assert_eq!(first.theme, second.theme);
        assert_eq!(first.color_depth, second.color_depth);
        assert_eq!(first.bindings.len(), second.bindings.len());
        assert_eq!(first.spectrum, second.spectrum);
        assert_eq!(first.pixel_path, second.pixel_path);
        assert_eq!(first.cell_aspect, second.cell_aspect);
        assert_eq!(first.clock, second.clock);
        assert_eq!(first.now, second.now);
        assert_eq!(first.music_dir, second.music_dir);
        assert_eq!(first.sleep_left, second.sleep_left);
    }
}
