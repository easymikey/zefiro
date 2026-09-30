use ratatui::style::Color;
use terminal::OwnedCoverArt;
use widgets::{ActiveTheme, Backdrop, FrameLayout};

use crate::shell::view::Presentation;

pub(crate) struct BackdropSources<'a> {
    pub(crate) presentation: &'a Presentation,
    pub(crate) mix: f32,
    pub(crate) outgoing_background: Option<Color>,
}

pub(crate) fn animation_backdrop(
    sources: &BackdropSources<'_>,
    layout: FrameLayout,
    cover_art: &OwnedCoverArt,
) -> Backdrop {
    let theme = ActiveTheme::new(
        &sources.presentation.theme,
        sources.presentation.color_depth,
    );
    let fill = theme.volume_bar_colors().fill;
    let background = theme.window_background();
    Backdrop {
        animations: sources.presentation.appearance.window.animations,
        layout: protected_layout(layout, cover_art),
        background,
        accent: theme.accent(),
        volume_fill: theme.color(fill),
        volume_lifted: theme.lifted(fill, sources.mix),
        wash_from: sources.outgoing_background.unwrap_or(background),
    }
}

fn protected_layout(mut layout: FrameLayout, cover_art: &OwnedCoverArt) -> FrameLayout {
    if !matches!(cover_art, OwnedCoverArt::Image) {
        layout.cover = None;
    }
    layout
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use config::AppearanceFile;
    use kernel::{Cue, Moment};
    use ratatui::{buffer::Buffer, layout::Rect};
    use runtime::FrameDue;
    use terminal::OwnedCoverArt;
    use widgets::{
        AnimationStage,
        Breakpoint,
        CellAspect,
        ColorDepth,
        FrameLayout,
        PixelPath,
        ToastAreas,
    };

    use crate::shell::{
        backdrop::{Backdrop, BackdropSources, animation_backdrop},
        frame_clock::{FrameEffect, animation_frame_due},
        view::{Presentation, fallback_theme_file},
    };

    fn test_backdrop(cover_art: &OwnedCoverArt) -> Backdrop {
        test_backdrop_with_outgoing(cover_art, None)
    }

    fn test_backdrop_with_outgoing(
        cover_art: &OwnedCoverArt,
        outgoing_background: Option<ratatui::style::Color>,
    ) -> Backdrop {
        let presentation = Presentation {
            theme: widgets::Theme::from(fallback_theme_file()),
            appearance: AppearanceFile::default(),
            pixel_path: PixelPath::Halfblocks,
            color_depth: ColorDepth::TrueColor,
            cell_aspect: CellAspect::default(),
            home: None,
            music_dir: std::path::PathBuf::new(),
            music_dir_label: String::new(),
        };
        let layout = FrameLayout {
            screen: Rect::new(0, 0, 40, 10),
            breakpoint: Breakpoint::Full,
            content: Rect::default(),
            header: Rect::default(),
            card: None,
            cover: Some(Rect::new(0, 0, 4, 4)),
            playlist_pane: Rect::default(),
            playlist: None,
            key_hints: None,
            search_bounds: Rect::default(),
            overlay: None,
            toast: Some(ToastAreas {
                outer: Rect::new(0, 0, 10, 1),
                painted: Rect::new(0, 0, 10, 1),
            }),
        };
        animation_backdrop(
            &BackdropSources {
                presentation: &presentation,
                mix: 0.2,
                outgoing_background,
            },
            layout,
            cover_art,
        )
    }

    #[test]
    fn with_no_wash_staged_the_wash_starts_from_the_current_background() {
        let backdrop = test_backdrop(&OwnedCoverArt::Missing);

        assert_eq!(backdrop.wash_from, backdrop.background);
    }

    #[test]
    fn a_staged_outgoing_background_becomes_the_wash_from() {
        let outgoing = ratatui::style::Color::Rgb(0x11, 0x22, 0x33);

        let backdrop =
            test_backdrop_with_outgoing(&OwnedCoverArt::Missing, Some(outgoing));

        assert_eq!(backdrop.wash_from, outgoing);
        assert_ne!(backdrop.wash_from, backdrop.background);
    }

    #[test]
    fn a_pixel_image_cover_stays_protected_from_effects() {
        let backdrop = test_backdrop(&OwnedCoverArt::Image);

        assert_eq!(backdrop.layout.cover, Some(Rect::new(0, 0, 4, 4)));
    }

    #[test]
    fn a_text_cover_takes_part_in_effects() {
        let backdrop = test_backdrop(&OwnedCoverArt::Text(Arc::default()));

        assert_eq!(backdrop.layout.cover, None);
    }

    #[test]
    fn a_missing_cover_takes_part_in_effects() {
        let backdrop = test_backdrop(&OwnedCoverArt::Missing);

        assert_eq!(backdrop.layout.cover, None);
    }

    #[test]
    fn an_ended_effect_settles_to_no_deadline_after_the_next_paint() {
        let backdrop = test_backdrop(&OwnedCoverArt::Missing);
        let mut stage = AnimationStage::default();
        stage.play(vec![Cue::ToastRaised], &backdrop);
        assert!(stage.wants_frame(), "sanity: the toast is animating");
        let mut buffer = Buffer::empty(backdrop.layout.screen);

        stage.advance(&mut buffer, Duration::from_secs(10));
        assert!(
            stage.wants_frame(),
            "sanity: the settling frame is still owed once the effect ends"
        );

        stage.play(Vec::new(), &backdrop);
        stage.advance(&mut buffer, Duration::ZERO);

        assert!(!stage.wants_frame());
        assert_eq!(
            animation_frame_due(FrameEffect::Settled, Moment::default()),
            FrameDue::Settled
        );
    }
}
