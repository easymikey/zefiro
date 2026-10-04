use std::{path::PathBuf, sync::Arc, time::Duration};

use image::{Rgba, RgbaImage};
use kernel::domain::{
    appearance::{Animations, CoverMode},
    geometry::Cells,
};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use ratatui_image::picker::Picker;
use rstest::rstest;
use terminal::pixels::CoverPainter;
use widgets::{
    animation::timings::AnimationTimings,
    card::CardCover,
    pixels::cover::{
        CoverImage,
        CoverMotion,
        CoverRefresh,
        CoverWash,
        CrossfadePermit,
    },
    screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
    toast::ToastAreas,
};

use crate::unit::support::{Scenery, playing_model};

fn cover(path: &str, pixel: Rgba<u8>) -> CoverImage {
    CoverImage {
        path: PathBuf::from(path),
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

fn parts(layout: FrameLayout, crossfade: CrossfadePermit) -> CoverRefresh {
    CoverRefresh {
        cover: layout.cover,
        crossfade,
        wash: CoverWash::Idle,
    }
}

fn crossfade_duration() -> Duration {
    Duration::from_millis(u64::from(AnimationTimings::default().cover_crossfade.0))
}

fn painted(buffer: &Buffer, rect: Rect) -> bool {
    (rect.left()..rect.right())
        .flat_map(|x| (rect.top()..rect.bottom()).map(move |y| (x, y)))
        .filter_map(|(x, y)| buffer.cell((x, y)))
        .any(|cell| cell.symbol() != " " || cell.bg != Color::Reset)
}

#[test]
fn no_decoded_cover_is_missing_art() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    let art =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(art, CardCover::Missing));
}

#[test]
fn a_decoded_cover_is_placed_as_an_image() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    let art =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(art, CardCover::Image));

    let mut buffer = Buffer::empty(cover_rect());
    pixels.paint(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[test]
fn an_off_style_never_shows_the_cover() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Off;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    let art =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(art, CardCover::Missing));
}

#[test]
fn a_vinyl_style_paints_an_image_over_the_cover_rect_even_without_art() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    let art =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(art, CardCover::Image));

    let mut buffer = Buffer::empty(cover_rect());
    pixels.paint(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[test]
fn a_vinyl_style_with_no_cover_rect_is_missing() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    let art = pixels.refresh(
        &sources.scene(),
        parts(layout_with_cover(None), CrossfadePermit::Withheld),
    );
    assert!(matches!(art, CardCover::Missing));
}

#[test]
fn a_milkdrop_style_returns_text_sized_to_the_cover_rect() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Milkdrop;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let rect = cover_rect();
    let layout = layout_with_cover(Some(rect));

    let art =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    let CardCover::Text(lines) = art else {
        panic!("milkdrop must hand back text lines, got {art:?}");
    };
    assert_eq!(lines.len(), usize::from(rect.height));
    for line in lines.iter() {
        assert_eq!(line.spans.len(), usize::from(rect.width));
    }
}

#[test]
fn a_milkdrop_style_with_no_cover_rect_is_missing() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Milkdrop;
    let mut pixels = CoverPainter::new(Picker::halfblocks());

    let art = pixels.refresh(
        &sources.scene(),
        parts(layout_with_cover(None), CrossfadePermit::Withheld),
    );
    assert!(matches!(art, CardCover::Missing));
}

#[test]
fn switching_from_plain_to_vinyl_and_back_keeps_showing_the_plain_image() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    let first =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(first, CardCover::Image));

    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));

    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    let back =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(
        matches!(back, CardCover::Image),
        "a style detour must not lose the plain cover"
    );

    let mut buffer = Buffer::empty(cover_rect());
    pixels.paint(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[test]
fn switching_between_milkdrop_and_vinyl_changes_the_cover_art_kind_immediately() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    sources.model.settings.appearance.cover_mode = CoverMode::Milkdrop;
    let text =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(text, CardCover::Text(_)));

    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    let image =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(image, CardCover::Image));

    sources.model.settings.appearance.cover_mode = CoverMode::Milkdrop;
    let text_again =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(text_again, CardCover::Text(_)));
}

#[test]
fn a_reused_plan_returns_the_same_lines_allocation() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    sources.model.settings.appearance.cover_mode = CoverMode::Milkdrop;
    let parts = parts(layout, CrossfadePermit::Withheld);

    let first = pixels.refresh(&sources.scene(), parts);
    let second = pixels.refresh(&sources.scene(), parts);

    let (CardCover::Text(first), CardCover::Text(second)) = (first, second) else {
        panic!("milkdrop must hand back text lines");
    };
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn reusing_the_same_path_and_rect_stays_an_image_across_frames() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    let art =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(art, CardCover::Image));
}

#[test]
fn an_allowed_track_change_crossfades_over_time() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    sources.model.settings.appearance.animations = Animations::On;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.set_cover(cover("first.mp3", Rgba([200, 10, 10, 255])));
    pixels.refresh(
        &sources.scene_at(Duration::ZERO),
        parts(layout, CrossfadePermit::Withheld),
    );

    pixels.set_cover(cover("second.mp3", Rgba([10, 10, 200, 255])));
    let mid = pixels.refresh(
        &sources.scene_at(Duration::from_millis(50)),
        parts(layout, CrossfadePermit::Allowed),
    );
    assert!(matches!(mid, CardCover::Image));

    let settled = pixels.refresh(
        &sources.scene_at(Duration::from_secs(5)),
        parts(layout, CrossfadePermit::Allowed),
    );
    assert!(matches!(settled, CardCover::Image));

    let mut buffer = Buffer::empty(cover_rect());
    pixels.paint(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[test]
fn no_cover_reports_a_still_motion() {
    let pixels = CoverPainter::new(Picker::halfblocks());

    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[test]
fn an_allowed_new_path_reports_crossfading() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    sources.model.settings.appearance.animations = Animations::On;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.set_cover(cover("first.mp3", Rgba([200, 10, 10, 255])));
    pixels.refresh(
        &sources.scene_at(Duration::ZERO),
        parts(layout, CrossfadePermit::Withheld),
    );

    pixels.set_cover(cover("second.mp3", Rgba([10, 10, 200, 255])));
    pixels.refresh(
        &sources.scene_at(Duration::from_millis(1)),
        parts(layout, CrossfadePermit::Allowed),
    );

    assert_eq!(
        pixels.cover_motion(Duration::from_millis(1)),
        CoverMotion::Animating
    );
}

#[test]
fn a_withheld_new_path_stays_still() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    sources.model.settings.appearance.animations = Animations::On;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.set_cover(cover("first.mp3", Rgba([200, 10, 10, 255])));
    pixels.refresh(
        &sources.scene_at(Duration::ZERO),
        parts(layout, CrossfadePermit::Withheld),
    );

    pixels.set_cover(cover("second.mp3", Rgba([10, 10, 200, 255])));
    pixels.refresh(
        &sources.scene_at(Duration::from_millis(1)),
        parts(layout, CrossfadePermit::Withheld),
    );

    assert_eq!(
        pixels.cover_motion(Duration::from_millis(1)),
        CoverMotion::Still
    );
}

#[test]
fn a_crossfade_reports_crossfading_until_a_paint_settles_it() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    sources.model.settings.appearance.animations = Animations::On;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.set_cover(cover("first.mp3", Rgba([200, 10, 10, 255])));
    pixels.refresh(
        &sources.scene_at(Duration::ZERO),
        parts(layout, CrossfadePermit::Withheld),
    );

    pixels.set_cover(cover("second.mp3", Rgba([10, 10, 200, 255])));
    pixels.refresh(
        &sources.scene_at(Duration::from_millis(1)),
        parts(layout, CrossfadePermit::Allowed),
    );

    let elapsed = Duration::from_millis(1) + crossfade_duration();
    assert_eq!(pixels.cover_motion(elapsed), CoverMotion::Animating);

    pixels.refresh(
        &sources.scene_at(elapsed),
        parts(layout, CrossfadePermit::Allowed),
    );
    assert_eq!(pixels.cover_motion(elapsed), CoverMotion::Still);
}

#[test]
fn a_wider_rect_forces_a_rebuild_without_a_crossfade() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Plain;
    sources.model.settings.appearance.animations = Animations::On;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    pixels.refresh(
        &sources.scene(),
        parts(
            layout_with_cover(Some(cover_rect())),
            CrossfadePermit::Allowed,
        ),
    );
    let wider = Rect::new(0, 0, 16, 4);
    let art = pixels.refresh(
        &sources.scene(),
        parts(layout_with_cover(Some(wider)), CrossfadePermit::Allowed),
    );
    assert!(matches!(art, CardCover::Image));
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[test]
fn a_settled_theme_wash_ends_on_the_new_image_and_goes_still() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));

    sources.model.revisions.theme.advance();
    let mid = pixels.refresh(
        &sources.scene(),
        CoverRefresh {
            wash: CoverWash::Running {
                progress: 0.2,
                screen_width: Cells(80),
            },
            ..parts(layout, CrossfadePermit::Withheld)
        },
    );
    assert!(matches!(mid, CardCover::Image));
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Animating);

    let settled =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert!(matches!(settled, CardCover::Image));
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[test]
fn a_theme_change_with_no_wash_staged_installs_the_new_image_at_once() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    sources.model.settings.appearance.animations = Animations::Off;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));

    sources.model.revisions.theme.advance();
    let art =
        pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));

    assert!(matches!(art, CardCover::Image));
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[test]
fn a_vinyl_rebuild_with_permission_on_a_new_path_crossfades_then_settles() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    sources.model.settings.appearance.animations = Animations::On;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(Rect::new(0, 0, 10, 10)));
    let allowed = parts(layout, CrossfadePermit::Allowed);

    pixels.set_cover(cover("a.jpg", Rgba([200, 100, 50, 255])));
    pixels.refresh(&sources.scene(), allowed);
    pixels.set_cover(cover("b.jpg", Rgba([200, 100, 50, 255])));
    pixels.refresh(&sources.scene(), allowed);
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Animating);
    assert_eq!(
        pixels.cover_motion(crossfade_duration()),
        CoverMotion::Animating
    );

    pixels.refresh(&sources.scene_at(crossfade_duration()), allowed);
    assert_eq!(
        pixels.cover_motion(crossfade_duration()),
        CoverMotion::Still
    );
}

#[test]
fn a_vinyl_rebuild_without_permission_never_crossfades() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    sources.model.settings.appearance.animations = Animations::On;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(Rect::new(0, 0, 10, 10)));

    pixels.set_cover(cover("a.jpg", Rgba([200, 100, 50, 255])));
    pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Allowed));
    pixels.set_cover(cover("b.jpg", Rgba([200, 100, 50, 255])));
    pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[test]
fn a_vinyl_rebuild_for_a_new_rect_on_the_same_path_never_crossfades() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    sources.model.settings.appearance.animations = Animations::On;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    pixels.set_cover(cover("a.jpg", Rgba([200, 100, 50, 255])));

    let narrow = layout_with_cover(Some(Rect::new(0, 0, 10, 10)));
    pixels.refresh(&sources.scene(), parts(narrow, CrossfadePermit::Allowed));
    let wide = layout_with_cover(Some(Rect::new(0, 0, 12, 10)));
    pixels.refresh(&sources.scene(), parts(wide, CrossfadePermit::Allowed));
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[rstest]
#[case::no_toast(None, true)]
#[case::a_toast_over_the_rect(Some(Rect::new(4, 0, 4, 1)), false)]
#[case::a_toast_away_from_the_rect(Some(Rect::new(0, 10, 8, 1)), true)]
fn a_toast_overlapping_the_cover_rect_hides_it(
    #[case] toast: Option<Rect>,
    #[case] visible: bool,
) {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.model.settings.appearance.cover_mode = CoverMode::Vinyl;
    let mut pixels = CoverPainter::new(Picker::halfblocks());
    let mut layout = layout_with_cover(Some(cover_rect()));
    layout.toast = toast.map(|rect| ToastAreas {
        outer: rect,
        painted: rect,
    });
    pixels.refresh(&sources.scene(), parts(layout, CrossfadePermit::Withheld));

    let mut buffer = Buffer::empty(cover_rect());
    pixels.paint(&mut buffer, &layout);
    assert_eq!(painted(&buffer, cover_rect()), visible);
}
