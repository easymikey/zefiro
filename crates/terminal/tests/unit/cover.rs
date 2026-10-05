use std::{mem::discriminant, path::PathBuf, sync::Arc, time::Duration};

use image::{Rgba, RgbaImage};
use kernel::domain::{
    appearance::{Animations, CoverMode, Rgb},
    geometry::{Cells, Pixels},
};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use ratatui_image::picker::Picker;
use rstest::{fixture, rstest};
use terminal::pixels::CoverPainter;
use widgets::{
    animation::timings::TIMINGS,
    card::CardCover,
    pixels::cover::{
        CoverImage,
        CoverMotion,
        CoverRefresh,
        CoverWash,
        CrossfadePermit,
        pixmap::CellPixels,
    },
    screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
    theme::{
        Theme,
        colors::{Colors, ThemeBase},
    },
};

use crate::support::{Scenery, noir_theme, playing_model};

fn recolored_theme() -> Theme {
    Theme {
        colors: Colors::derive(&ThemeBase {
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

fn layout_with_cover(cover: Option<Rect>) -> FrameLayout {
    FrameLayout {
        screen: Rect::default(),
        breakpoint: Breakpoint::Full,
        content: Rect::default(),
        header: Rect::default(),
        card: None,
        cover,
        playlist_pane: Rect::default(),
        playlist: None,
        key_hints: None,
        search_bounds: Rect::default(),
        overlay: None,
        toast: None,
    }
}

fn cover_refresh(layout: FrameLayout, crossfade: CrossfadePermit) -> CoverRefresh {
    CoverRefresh {
        cover: layout.cover,
        crossfade,
        wash: CoverWash::Idle,
    }
}

fn crossfade_duration() -> Duration {
    Duration::from_millis(u64::from(TIMINGS.cover_crossfade.0))
}

fn painted(buffer: &Buffer, rect: Rect) -> bool {
    (rect.left()..rect.right())
        .flat_map(|x| (rect.top()..rect.bottom()).map(move |y| (x, y)))
        .filter_map(|(x, y)| buffer.cell((x, y)))
        .any(|cell| cell.symbol() != " " || cell.bg != Color::Reset)
}

#[fixture]
fn painter() -> CoverPainter {
    let picker = Picker::halfblocks();
    let font_size = picker.font_size();
    let cell = CellPixels {
        width: Pixels(u32::from(font_size.width)),
        height: Pixels(u32::from(font_size.height)),
    };
    CoverPainter::new(picker, cell)
}

#[fixture]
fn scenery(#[default(CoverMode::Plain)] mode: CoverMode) -> Scenery {
    let mut scenery = Scenery::new(playing_model("moon-river", 200, 50));
    scenery.model.settings.appearance.cover_mode = mode;
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
    #[case] mode: CoverMode,
    #[case] art: bool,
    #[case] expected: CardCover,
) {
    let scenery = scenery::get(mode);
    let mut painter = painter::get();
    if art {
        painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));
    }

    let card_cover = painter.refresh(
        &scenery.scene(),
        cover_refresh(
            layout_with_cover(Some(cover_rect())),
            CrossfadePermit::Withheld,
        ),
    );
    assert_eq!(discriminant(&card_cover), discriminant(&expected));
}

#[rstest]
#[case::vinyl(CoverMode::Vinyl)]
#[case::milkdrop(CoverMode::Milkdrop)]
fn a_cover_mode_without_a_cover_rect_is_missing(
    #[case] mode: CoverMode,
    mut painter: CoverPainter,
) {
    let scenery = scenery::get(mode);
    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));

    let card_cover = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout_with_cover(None), CrossfadePermit::Withheld),
    );
    assert!(matches!(card_cover, CardCover::Missing));
}

#[rstest]
fn a_milkdrop_style_returns_text_sized_to_the_cover_rect(
    #[with(CoverMode::Milkdrop)] scenery: Scenery,
    mut painter: CoverPainter,
) {
    let card_cover = painter.refresh(
        &scenery.scene(),
        cover_refresh(
            layout_with_cover(Some(cover_rect())),
            CrossfadePermit::Withheld,
        ),
    );
    let CardCover::Text(lines) = card_cover else {
        panic!("milkdrop must hand back text lines, got {card_cover:?}");
    };
    assert_eq!(lines.len(), 4);
    for line in lines.iter() {
        assert_eq!(line.spans.len(), 8);
    }
}

#[rstest]
fn switching_from_plain_to_vinyl_and_back_keeps_showing_the_plain_image(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    let layout = layout_with_cover(Some(cover_rect()));
    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));

    scenery.model.settings.appearance.cover_mode = CoverMode::Plain;
    let first = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
    assert!(matches!(first, CardCover::Image));

    scenery.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    scenery.model.settings.appearance.cover_mode = CoverMode::Plain;
    let back = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
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

    scenery.model.settings.appearance.cover_mode = CoverMode::Milkdrop;
    let text = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
    assert!(matches!(text, CardCover::Text(_)));

    scenery.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    let image = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
    assert!(matches!(image, CardCover::Image));

    scenery.model.settings.appearance.cover_mode = CoverMode::Milkdrop;
    let text_again = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
    assert!(matches!(text_again, CardCover::Text(_)));
}

#[rstest]
fn a_reused_plan_returns_the_same_lines_allocation(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    let layout = layout_with_cover(Some(cover_rect()));
    scenery.model.settings.appearance.cover_mode = CoverMode::Milkdrop;
    let refresh = cover_refresh(layout, CrossfadePermit::Withheld);

    let first = painter.refresh(&scenery.scene(), refresh);
    let second = painter.refresh(&scenery.scene(), refresh);

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

    painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
    let art = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
    assert!(matches!(art, CardCover::Image));
}

#[rstest]
fn an_allowed_track_change_crossfades_over_time(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    scenery.model.settings.appearance.animations = Animations::On;
    let layout = layout_with_cover(Some(cover_rect()));

    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));
    painter.refresh(
        &scenery.scene_at(Duration::ZERO),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    scenery.model.player = playing_model("second", 200, 50).player;
    painter.set_cover(cover("second", Rgba([10, 10, 200, 255])));
    let mid = painter.refresh(
        &scenery.scene_at(Duration::from_millis(50)),
        cover_refresh(layout, CrossfadePermit::Allowed),
    );
    assert!(matches!(mid, CardCover::Image));

    let settled = painter.refresh(
        &scenery.scene_at(Duration::from_secs(5)),
        cover_refresh(layout, CrossfadePermit::Allowed),
    );
    assert!(matches!(settled, CardCover::Image));

    let mut buffer = Buffer::empty(cover_rect());
    painter.paint(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[rstest]
fn no_cover_reports_a_still_motion(painter: CoverPainter) {
    assert_eq!(painter.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[rstest]
fn an_allowed_new_path_reports_crossfading(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    scenery.model.settings.appearance.animations = Animations::On;
    let layout = layout_with_cover(Some(cover_rect()));

    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));
    painter.refresh(
        &scenery.scene_at(Duration::ZERO),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    scenery.model.player = playing_model("second", 200, 50).player;
    painter.set_cover(cover("second", Rgba([10, 10, 200, 255])));
    painter.refresh(
        &scenery.scene_at(Duration::from_millis(1)),
        cover_refresh(layout, CrossfadePermit::Allowed),
    );

    assert_eq!(
        painter.cover_motion(Duration::from_millis(1)),
        CoverMotion::Animating
    );
}

#[rstest]
fn a_withheld_new_path_stays_still(mut scenery: Scenery, mut painter: CoverPainter) {
    scenery.model.settings.appearance.animations = Animations::On;
    let layout = layout_with_cover(Some(cover_rect()));

    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));
    painter.refresh(
        &scenery.scene_at(Duration::ZERO),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    scenery.model.player = playing_model("second", 200, 50).player;
    painter.set_cover(cover("second", Rgba([10, 10, 200, 255])));
    painter.refresh(
        &scenery.scene_at(Duration::from_millis(1)),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    assert_eq!(
        painter.cover_motion(Duration::from_millis(1)),
        CoverMotion::Still
    );
}

#[rstest]
fn a_crossfade_reports_crossfading_until_a_paint_settles_it(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    scenery.model.settings.appearance.animations = Animations::On;
    let layout = layout_with_cover(Some(cover_rect()));

    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));
    painter.refresh(
        &scenery.scene_at(Duration::ZERO),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    scenery.model.player = playing_model("second", 200, 50).player;
    painter.set_cover(cover("second", Rgba([10, 10, 200, 255])));
    painter.refresh(
        &scenery.scene_at(Duration::from_millis(1)),
        cover_refresh(layout, CrossfadePermit::Allowed),
    );

    let elapsed = Duration::from_millis(1) + crossfade_duration();
    assert_eq!(painter.cover_motion(elapsed), CoverMotion::Animating);

    painter.refresh(
        &scenery.scene_at(elapsed),
        cover_refresh(layout, CrossfadePermit::Allowed),
    );
    assert_eq!(painter.cover_motion(elapsed), CoverMotion::Still);
}

#[rstest]
#[case::off(CoverMode::Off)]
#[case::vinyl(CoverMode::Vinyl)]
#[case::milkdrop(CoverMode::Milkdrop)]
fn a_mode_switch_mid_crossfade_ends_still(
    #[case] next: CoverMode,
    mut painter: CoverPainter,
) {
    let mut scenery = Scenery::new(playing_model("first", 200, 50));
    scenery.model.settings.appearance.cover_mode = CoverMode::Plain;
    scenery.model.settings.appearance.animations = Animations::On;
    let layout = layout_with_cover(Some(cover_rect()));

    painter.set_cover(cover("first", Rgba([200, 10, 10, 255])));
    painter.refresh(
        &scenery.scene_at(Duration::ZERO),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
    scenery.model.player = playing_model("second", 200, 50).player;
    painter.set_cover(cover("second", Rgba([10, 10, 200, 255])));
    painter.refresh(
        &scenery.scene_at(Duration::from_millis(1)),
        cover_refresh(layout, CrossfadePermit::Allowed),
    );
    let mid_crossfade = Duration::from_millis(2);
    assert_eq!(painter.cover_motion(mid_crossfade), CoverMotion::Animating);

    scenery.model.settings.appearance.cover_mode = next;
    painter.refresh(
        &scenery.scene_at(mid_crossfade),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    assert_eq!(painter.cover_motion(mid_crossfade), CoverMotion::Still);
}

#[rstest]
fn a_wider_rect_forces_a_rebuild_without_a_crossfade(
    mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    scenery.model.settings.appearance.animations = Animations::On;
    painter.set_cover(cover("moon-river", Rgba([200, 10, 10, 255])));

    painter.refresh(
        &scenery.scene(),
        cover_refresh(
            layout_with_cover(Some(cover_rect())),
            CrossfadePermit::Allowed,
        ),
    );
    let wider = Rect::new(0, 0, 16, 4);
    let art = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout_with_cover(Some(wider)), CrossfadePermit::Allowed),
    );
    assert!(matches!(art, CardCover::Image));
    assert_eq!(painter.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[rstest]
fn a_settled_theme_wash_ends_on_the_new_image_and_goes_still(
    #[with(CoverMode::Vinyl)] mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    let layout = layout_with_cover(Some(cover_rect()));

    painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    scenery.model.revisions.theme.advance();
    scenery.theme = recolored_theme();
    let mid = painter.refresh(
        &scenery.scene(),
        CoverRefresh {
            wash: CoverWash::Running {
                progress: 0.2,
                screen_width: Cells(80),
            },
            ..cover_refresh(layout, CrossfadePermit::Withheld)
        },
    );
    assert!(matches!(mid, CardCover::Image));
    assert_eq!(painter.cover_motion(Duration::ZERO), CoverMotion::Animating);

    let settled = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
    assert!(matches!(settled, CardCover::Image));
    assert_eq!(painter.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[rstest]
fn a_theme_change_with_no_wash_staged_installs_the_new_image_at_once(
    #[with(CoverMode::Vinyl)] mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    scenery.model.settings.appearance.animations = Animations::Off;
    let layout = layout_with_cover(Some(cover_rect()));

    painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    scenery.model.revisions.theme.advance();
    scenery.theme = recolored_theme();
    let art = painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    assert!(matches!(art, CardCover::Image));
    assert_eq!(painter.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[rstest]
fn a_vinyl_rebuild_with_permission_on_a_new_path_crossfades_then_settles(
    #[with(CoverMode::Vinyl)] mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    scenery.model.settings.appearance.animations = Animations::On;
    let layout = layout_with_cover(Some(Rect::new(0, 0, 10, 10)));
    let allowed = cover_refresh(layout, CrossfadePermit::Allowed);

    painter.set_cover(cover("moon-river", Rgba([200, 100, 50, 255])));
    painter.refresh(&scenery.scene(), allowed);
    scenery.model.player = playing_model("second", 200, 50).player;
    painter.set_cover(cover("second", Rgba([200, 100, 50, 255])));
    painter.refresh(&scenery.scene(), allowed);
    assert_eq!(painter.cover_motion(Duration::ZERO), CoverMotion::Animating);
    assert_eq!(
        painter.cover_motion(crossfade_duration()),
        CoverMotion::Animating
    );

    painter.refresh(&scenery.scene_at(crossfade_duration()), allowed);
    assert_eq!(
        painter.cover_motion(crossfade_duration()),
        CoverMotion::Still
    );
}

#[rstest]
fn a_vinyl_rebuild_without_permission_never_crossfades(
    #[with(CoverMode::Vinyl)] mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    scenery.model.settings.appearance.animations = Animations::On;
    let layout = layout_with_cover(Some(Rect::new(0, 0, 10, 10)));

    painter.set_cover(cover("moon-river", Rgba([200, 100, 50, 255])));
    painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Allowed),
    );
    scenery.model.player = playing_model("second", 200, 50).player;
    painter.set_cover(cover("second", Rgba([200, 100, 50, 255])));
    painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );
    assert_eq!(painter.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[rstest]
fn a_vinyl_rebuild_for_a_new_rect_on_the_same_path_never_crossfades(
    #[with(CoverMode::Vinyl)] mut scenery: Scenery,
    mut painter: CoverPainter,
) {
    scenery.model.settings.appearance.animations = Animations::On;
    painter.set_cover(cover("moon-river", Rgba([200, 100, 50, 255])));

    let narrow = layout_with_cover(Some(Rect::new(0, 0, 10, 10)));
    painter.refresh(
        &scenery.scene(),
        cover_refresh(narrow, CrossfadePermit::Allowed),
    );
    let wide = layout_with_cover(Some(Rect::new(0, 0, 12, 10)));
    painter.refresh(
        &scenery.scene(),
        cover_refresh(wide, CrossfadePermit::Allowed),
    );
    assert_eq!(painter.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[rstest]
#[case::no_toast(None, true)]
#[case::a_toast_over_the_rect(Some(Rect::new(4, 0, 4, 1)), false)]
#[case::a_toast_away_from_the_rect(Some(Rect::new(0, 10, 8, 1)), true)]
fn a_toast_overlapping_the_cover_rect_hides_it(
    #[case] toast: Option<Rect>,
    #[case] visible: bool,
    mut painter: CoverPainter,
) {
    let scenery = scenery::get(CoverMode::Vinyl);
    let mut layout = layout_with_cover(Some(cover_rect()));
    layout.toast = toast;
    painter.refresh(
        &scenery.scene(),
        cover_refresh(layout, CrossfadePermit::Withheld),
    );

    let mut buffer = Buffer::empty(cover_rect());
    painter.paint(&mut buffer, &layout);
    assert_eq!(painted(&buffer, cover_rect()), visible);
}
