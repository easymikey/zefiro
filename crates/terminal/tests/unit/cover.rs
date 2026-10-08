use std::{mem::discriminant, path::PathBuf, sync::Arc, time::Duration};

use image::{Rgba, RgbaImage};
use kernel::domain::{
    appearance::{Breakpoints, CoverMode, Rgb},
    geometry::{Cells, Pixels},
    time::Moment,
    toast::Toast,
};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use ratatui_image::picker::{Picker, ProtocolType};
use rstest::{fixture, rstest};
use terminal::{capabilities::Capabilities, pixels::CoverPainter};
use widgets::{
    animation::stage::{AnimationStage, animation_frame_due},
    card::CardCover,
    overlay::modal::placement::OverlayAreas,
    pixels::cover::{CoverImage, pixmap::CellPixels},
    screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
    theme::{
        Theme,
        colors::{Colors, ThemeBase},
        rgb::ColorDepth,
    },
};

use crate::support::{Scenery, noir_theme, playing_model};

fn recolored_theme() -> Theme {
    Theme {
        colors: Colors::from_theme_base(&ThemeBase {
            background: Rgb([0xf4, 0xf1, 0xea]),
            muted_foreground: Rgb([0x8a, 0x84, 0x78]),
            foreground: Rgb([0x22, 0x20, 0x1c]),
            accent: Rgb([0x3d, 0x9b, 0xff]),
            green: Rgb([0x4f, 0x8a, 0x3c]),
            yellow: Rgb([0xc4, 0x9a, 0x2a]),
            red: Rgb([0xc2, 0x41, 0x3b]),
            window_background: None,
        }),
        ..noir_theme()
    }
}

fn cover(title: &str, pixel: Rgba<u8>) -> CoverImage {
    CoverImage {
        path: PathBuf::from(format!("/music/{title}.mp3")),
        image: Arc::new(RgbaImage::from_pixel(4, 4, pixel)),
    }
}

fn cover_rect() -> Rect {
    Rect::new(0, 0, 8, 4)
}

fn layout_with_cover(cover_area: Option<Rect>) -> FrameLayout<'static> {
    FrameLayout {
        screen: Rect::default(),
        breakpoint: Breakpoint::Full,
        content: Rect::default(),
        header: Rect::default(),
        card_metrics: None,
        progress_bar_width: Cells(0),
        remaining_label: String::new(),
        cover_area,
        playlist_pane: Rect::default(),
        playlist_areas: None,
        key_hints: None,
        search_bounds: Rect::default(),
        overlay_areas: None,
        overlay_content: None,
        toast_placement: None,
    }
}

fn painted(buffer: &Buffer, rect: Rect) -> bool {
    (rect.left()..rect.right())
        .flat_map(|x| (rect.top()..rect.bottom()).map(move |y| (x, y)))
        .filter_map(|(x, y)| buffer.cell((x, y)))
        .any(|cell| cell.symbol() != " " || cell.bg != Color::Reset)
}

fn halfblocks_capabilities() -> Capabilities {
    Capabilities {
        picker: Picker::halfblocks(),
        color_depth: ColorDepth::TrueColor,
    }
}

#[fixture]
fn capabilities() -> Capabilities {
    let mut picker = Picker::halfblocks();
    picker.set_protocol_type(ProtocolType::Kitty);
    Capabilities {
        picker,
        color_depth: ColorDepth::TrueColor,
    }
}

#[fixture]
fn painter(capabilities: Capabilities) -> CoverPainter {
    let Capabilities {
        picker,
        color_depth: _color_depth,
    } = capabilities;
    let font_size = picker.font_size();
    let cell_pixels = CellPixels {
        width: Pixels(u32::from(font_size.width)),
        height: Pixels(u32::from(font_size.height)),
    };
    CoverPainter::new(picker, cell_pixels)
}

#[fixture]
fn scenery(
    #[default(CoverMode::Plain)] cover_mode: CoverMode,
    capabilities: Capabilities,
) -> Scenery {
    let mut scenery = Scenery::new(
        playing_model("moon-river", 200, 50),
        capabilities.pixel_path(),
    );
    scenery.model.settings.appearance_settings.cover_mode = cover_mode;
    scenery
}

#[rstest]
#[case::plain_without_art(CoverMode::Plain, false, CardCover::Missing)]
#[case::plain_with_art(CoverMode::Plain, true, CardCover::Image)]
#[case::off_with_art(CoverMode::Off, true, CardCover::Missing)]
#[case::vinyl_without_art(CoverMode::Vinyl, false, CardCover::Image)]
#[case::milkdrop_without_art(
    CoverMode::Milkdrop,
    false,
    CardCover::Text(Arc::from(Vec::new()))
)]
fn a_cover_mode_picks_the_card_cover_kind(
    #[case] cover_mode: CoverMode,
    #[case] art: bool,
    #[case] expected: CardCover,
) {
    let scenery = scenery::get(cover_mode, capabilities());
    let mut painter = painter::get(capabilities());
    if art {
        painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));
    }

    let card_cover = painter.refresh(&scenery.scene(), Some(cover_rect()));
    assert_eq!(discriminant(&card_cover), discriminant(&expected));
}

#[rstest]
#[case::plain(CoverMode::Plain)]
#[case::vinyl(CoverMode::Vinyl)]
fn a_halfblocks_painter_in_an_image_mode_answers_missing_and_stays_still(
    #[case] cover_mode: CoverMode,
) {
    let scenery = scenery::get(cover_mode, halfblocks_capabilities());
    let mut painter = painter::get(halfblocks_capabilities());
    let layout = layout_with_cover(Some(cover_rect()));
    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));

    let first = painter.refresh(&scenery.scene(), layout.cover_area);
    let second = painter.refresh(&scenery.scene(), layout.cover_area);
    assert!(matches!(first, CardCover::Missing));
    assert!(matches!(second, CardCover::Missing));

    let mut buffer = Buffer::empty(cover_rect());
    painter.paint(&mut buffer, &layout);
    assert!(!painted(&buffer, cover_rect()));
}

#[rstest]
#[case::vinyl(CoverMode::Vinyl)]
#[case::milkdrop(CoverMode::Milkdrop)]
fn a_cover_mode_without_a_cover_rect_is_missing(
    #[case] cover_mode: CoverMode,
    mut painter: CoverPainter,
) {
    let scenery = scenery::get(cover_mode, capabilities());
    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));

    let card_cover = painter.refresh(&scenery.scene(), None);
    assert!(matches!(card_cover, CardCover::Missing));
}

#[rstest]
fn a_milkdrop_style_returns_text_sized_to_the_cover_rect(
    #[with(CoverMode::Milkdrop)] scenery: Scenery,
    mut painter: CoverPainter,
) {
    let card_cover = painter.refresh(&scenery.scene(), Some(cover_rect()));
    let CardCover::Text(lines) = card_cover else {
        panic!("milkdrop must hand back text lines, got {card_cover:?}");
    };
    assert_eq!(lines.len(), 4);
    for line in lines.iter() {
        assert_eq!(line.spans.len(), 8);
    }
}

#[rstest]
fn a_cover_fade_keeps_the_next_frame_due_until_its_last_step(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    let animation_stage = AnimationStage::default();
    let next_frame_at = Moment::new(Duration::from_millis(1_033));
    let frame_due = |cover_painter: &CoverPainter| {
        animation_frame_due(&animation_stage, cover_painter.motion(), next_frame_at)
    };
    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));
    painter.refresh(&scenery.scene(), Some(cover_rect()));
    assert_eq!(frame_due(&painter), None);

    scenery.model.player = playing_model("blue-moon", 200, 50).player;
    painter.set_cover(cover("blue-moon", Rgba([10, 10, 200, 255])));
    painter.refresh(&scenery.scene(), Some(cover_rect()));
    assert_eq!(frame_due(&painter), Some(next_frame_at));

    scenery.since_first_paint = Duration::from_millis(300);
    painter.refresh(&scenery.scene(), Some(cover_rect()));
    assert_eq!(frame_due(&painter), Some(next_frame_at));

    scenery.since_first_paint = Duration::from_millis(600);
    painter.refresh(&scenery.scene(), Some(cover_rect()));
    assert_eq!(frame_due(&painter), None);
}

#[rstest]
fn switching_from_plain_to_vinyl_and_back_keeps_showing_the_plain_image(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    let layout = layout_with_cover(Some(cover_rect()));
    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));

    scenery.model.settings.appearance_settings.cover_mode = CoverMode::Plain;
    let first = painter.refresh(&scenery.scene(), layout.cover_area);
    assert!(matches!(first, CardCover::Image));

    scenery.model.settings.appearance_settings.cover_mode = CoverMode::Vinyl;
    painter.refresh(&scenery.scene(), layout.cover_area);

    scenery.model.settings.appearance_settings.cover_mode = CoverMode::Plain;
    let back = painter.refresh(&scenery.scene(), layout.cover_area);
    assert!(
        matches!(back, CardCover::Image),
        "a style detour must not lose the plain cover"
    );

    let mut buffer = Buffer::empty(cover_rect());
    painter.paint(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[rstest]
fn switching_between_milkdrop_and_vinyl_changes_the_cover_art_kind_immediately(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    let layout = layout_with_cover(Some(cover_rect()));

    scenery.model.settings.appearance_settings.cover_mode = CoverMode::Milkdrop;
    let text = painter.refresh(&scenery.scene(), layout.cover_area);
    assert!(matches!(text, CardCover::Text(_)));

    scenery.model.settings.appearance_settings.cover_mode = CoverMode::Vinyl;
    let image = painter.refresh(&scenery.scene(), layout.cover_area);
    assert!(matches!(image, CardCover::Image));

    scenery.model.settings.appearance_settings.cover_mode = CoverMode::Milkdrop;
    let text_again = painter.refresh(&scenery.scene(), layout.cover_area);
    assert!(matches!(text_again, CardCover::Text(_)));
}

#[rstest]
fn a_reused_plan_returns_the_same_lines_allocation(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    let layout = layout_with_cover(Some(cover_rect()));
    scenery.model.settings.appearance_settings.cover_mode = CoverMode::Milkdrop;

    let first = painter.refresh(&scenery.scene(), layout.cover_area);
    let second = painter.refresh(&scenery.scene(), layout.cover_area);

    let (CardCover::Text(first), CardCover::Text(second)) = (first, second) else {
        panic!("milkdrop must hand back text lines");
    };
    assert!(Arc::ptr_eq(&first, &second));
}

#[rstest]
fn reusing_the_same_path_and_rect_stays_an_image_across_frames(
    #[with(CoverMode::Plain)] scenery: Scenery,
    mut painter: CoverPainter,
) {
    let layout = layout_with_cover(Some(cover_rect()));
    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));

    painter.refresh(&scenery.scene(), layout.cover_area);
    let card_cover = painter.refresh(&scenery.scene(), layout.cover_area);
    assert!(matches!(card_cover, CardCover::Image));
}

#[rstest]
fn a_track_change_repaints_the_new_cover_once(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    let layout = layout_with_cover(Some(cover_rect()));

    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));
    painter.refresh(&scenery.scene(), layout.cover_area);

    scenery.model.player = playing_model("second", 200, 50).player;
    painter.set_cover(cover("second", Rgba([10, 10, 200, 255])));
    let swapped = painter.refresh(&scenery.scene(), layout.cover_area);
    assert!(matches!(swapped, CardCover::Image));

    let mut buffer = Buffer::empty(cover_rect());
    painter.paint(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[rstest]
fn a_wider_rect_forces_a_rebuild(scenery: Scenery, mut painter: CoverPainter) {
    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));

    painter.refresh(&scenery.scene(), Some(cover_rect()));
    let wider = Rect::new(0, 0, 16, 4);
    let card_cover = painter.refresh(&scenery.scene(), Some(wider));
    assert!(matches!(card_cover, CardCover::Image));
}

#[rstest]
fn a_theme_change_installs_the_new_image_at_once(
    #[with(CoverMode::Vinyl)] mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    let layout = layout_with_cover(Some(cover_rect()));

    painter.refresh(&scenery.scene(), layout.cover_area);

    scenery.model.revisions.theme.advance();
    scenery.theme = recolored_theme();
    let card_cover = painter.refresh(&scenery.scene(), layout.cover_area);

    assert!(matches!(card_cover, CardCover::Image));
}

#[rstest]
#[case::no_toast(None, true)]
#[case::a_toast_over_the_rect(Some(Rect::new(0, 0, 8, 1)), false)]
#[case::a_toast_away_from_the_rect(Some(Rect::new(0, 10, 8, 1)), true)]
fn a_toast_overlapping_the_cover_rect_hides_it(
    #[case] toast_screen: Option<Rect>,
    #[case] visible: bool,
    mut painter: CoverPainter,
) {
    let mut scenery = scenery::get(CoverMode::Vinyl, capabilities());
    scenery.model.workspace.toasts = vec![Toast::info("Queued")];
    scenery.appearance.breakpoints = Breakpoints {
        full_min_width: Cells(u16::MAX),
        full_min_height: Cells(u16::MAX),
        compact_min_width: Cells(u16::MAX),
        compact_min_height: Cells(u16::MAX),
        min_width: Cells(0),
        min_height: Cells(0),
    };
    let scene = scenery.scene();
    let layout = FrameLayout {
        toast_placement: toast_screen
            .and_then(|screen| FrameLayout::from_scene(&scene, screen).toast_placement),
        ..layout_with_cover(Some(cover_rect()))
    };
    painter.refresh(&scene, layout.cover_area);

    let mut buffer = Buffer::empty(cover_rect());
    painter.paint(&mut buffer, &layout);
    assert_eq!(painted(&buffer, cover_rect()), visible);
}

#[rstest]
#[case::an_overlay_over_the_rect(Rect::new(2, 1, 4, 2), false)]
#[case::an_overlay_away_from_the_rect(Rect::new(0, 10, 8, 4), true)]
fn an_open_overlay_over_the_cover_rect_hides_it(
    #[case] overlay_outer: Rect,
    #[case] visible: bool,
    mut painter: CoverPainter,
) {
    let scenery = scenery::get(CoverMode::Vinyl, capabilities());
    let layout = FrameLayout {
        overlay_areas: Some(OverlayAreas::Banner(overlay_outer)),
        ..layout_with_cover(Some(cover_rect()))
    };
    painter.refresh(&scenery.scene(), layout.cover_area);

    let mut buffer = Buffer::empty(cover_rect());
    painter.paint(&mut buffer, &layout);
    assert_eq!(painted(&buffer, cover_rect()), visible);
}
