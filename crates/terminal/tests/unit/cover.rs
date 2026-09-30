use std::{path::PathBuf, sync::Arc, time::Duration};

use config::{Animations, CoverStyle};
use image::{Rgba, RgbaImage};
use ratatui::{buffer::Buffer, layout::Rect, style::Color};
use ratatui_image::picker::Picker;
use terminal::{
    CoverFade,
    CoverMotion,
    CoverPlacement,
    CoverRenderer,
    CoverWash,
    DecodedCover,
    OwnedCoverArt,
};
use widgets::{AnimationTimings, Breakpoint, FrameLayout};

use crate::unit::support::{Scenery, playing_model};

fn cover(path: &str, pixel: Rgba<u8>) -> DecodedCover {
    DecodedCover {
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

fn painted(buffer: &Buffer, rect: Rect) -> bool {
    (rect.left()..rect.right())
        .flat_map(|x| (rect.top()..rect.bottom()).map(move |y| (x, y)))
        .filter_map(|(x, y)| buffer.cell((x, y)))
        .any(|cell| cell.symbol() != " " || cell.bg != Color::Reset)
}

#[test]
fn no_decoded_cover_is_missing_art() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Plain;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(art, OwnedCoverArt::Missing));
}

#[test]
fn a_decoded_cover_is_placed_as_an_image() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Plain;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(art, OwnedCoverArt::Image));

    let mut buffer = Buffer::empty(cover_rect());
    pixels.place(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[test]
fn an_off_style_never_shows_the_cover() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Off;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(art, OwnedCoverArt::Missing));
}

#[test]
fn a_vinyl_style_paints_an_image_over_the_cover_rect_even_without_art() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Vinyl;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(art, OwnedCoverArt::Image));

    let mut buffer = Buffer::empty(cover_rect());
    pixels.place(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[test]
fn a_vinyl_style_with_no_cover_rect_is_missing() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Vinyl;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout: layout_with_cover(None),
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(art, OwnedCoverArt::Missing));
}

#[test]
fn a_milkdrop_style_returns_text_sized_to_the_cover_rect() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Milkdrop;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let rect = cover_rect();
    let layout = layout_with_cover(Some(rect));

    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    let OwnedCoverArt::Text(lines) = art else {
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
    sources.appearance.cover.style = CoverStyle::Milkdrop;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());

    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout: layout_with_cover(None),
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(art, OwnedCoverArt::Missing));
}

#[test]
fn switching_from_plain_to_vinyl_and_back_keeps_showing_the_plain_image() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    sources.appearance.cover.style = CoverStyle::Plain;
    let first = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(first, OwnedCoverArt::Image));

    sources.appearance.cover.style = CoverStyle::Vinyl;
    pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));

    sources.appearance.cover.style = CoverStyle::Plain;
    let back = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(
        matches!(back, OwnedCoverArt::Image),
        "a style detour must not lose the plain cover"
    );

    let mut buffer = Buffer::empty(cover_rect());
    pixels.place(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

#[test]
fn switching_between_milkdrop_and_vinyl_changes_the_cover_art_kind_immediately() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    sources.appearance.cover.style = CoverStyle::Milkdrop;
    let text = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(text, OwnedCoverArt::Text(_)));

    sources.appearance.cover.style = CoverStyle::Vinyl;
    let image = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(image, OwnedCoverArt::Image));

    sources.appearance.cover.style = CoverStyle::Milkdrop;
    let text_again = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(text_again, OwnedCoverArt::Text(_)));
}

#[test]
fn a_reused_plan_returns_the_same_lines_allocation() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    sources.appearance.cover.style = CoverStyle::Milkdrop;
    let placement = CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    };

    let first = pixels.refresh(sources.sources(placement));
    let second = pixels.refresh(sources.sources(placement));

    let (OwnedCoverArt::Text(first), OwnedCoverArt::Text(second)) = (first, second)
    else {
        panic!("milkdrop must hand back text lines");
    };
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn reusing_the_same_path_and_rect_stays_an_image_across_frames() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Plain;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(art, OwnedCoverArt::Image));
}

#[test]
fn an_allowed_track_change_crossfades_over_time() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Plain;
    sources.appearance.window.animations = Animations::On;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.set_cover(cover("first.mp3", Rgba([200, 10, 10, 255])));
    pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Withheld,
            wash: CoverWash::Idle,
        },
        Duration::ZERO,
    ));

    pixels.set_cover(cover("second.mp3", Rgba([10, 10, 200, 255])));
    let mid = pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        },
        Duration::from_millis(50),
    ));
    assert!(matches!(mid, OwnedCoverArt::Image));

    let settled = pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        },
        Duration::from_secs(5),
    ));
    assert!(matches!(settled, OwnedCoverArt::Image));

    let mut buffer = Buffer::empty(cover_rect());
    pixels.place(&mut buffer, &layout);
    assert!(painted(&buffer, cover_rect()));
}

fn crossfade_duration() -> Duration {
    Duration::from_millis(u64::from(AnimationTimings::default().cover_crossfade.0))
}

#[test]
fn no_cover_reports_a_still_motion() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Plain;
    let pixels = CoverRenderer::new(Picker::halfblocks());

    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[test]
fn an_allowed_new_path_reports_crossfading() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Plain;
    sources.appearance.window.animations = Animations::On;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.set_cover(cover("first.mp3", Rgba([200, 10, 10, 255])));
    pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Withheld,
            wash: CoverWash::Idle,
        },
        Duration::ZERO,
    ));

    pixels.set_cover(cover("second.mp3", Rgba([10, 10, 200, 255])));
    pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        },
        Duration::from_millis(1),
    ));

    assert_eq!(
        pixels.cover_motion(Duration::from_millis(1)),
        CoverMotion::Crossfading
    );
}

#[test]
fn a_withheld_new_path_stays_still() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Plain;
    sources.appearance.window.animations = Animations::On;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.set_cover(cover("first.mp3", Rgba([200, 10, 10, 255])));
    pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Withheld,
            wash: CoverWash::Idle,
        },
        Duration::ZERO,
    ));

    pixels.set_cover(cover("second.mp3", Rgba([10, 10, 200, 255])));
    pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Withheld,
            wash: CoverWash::Idle,
        },
        Duration::from_millis(1),
    ));

    assert_eq!(
        pixels.cover_motion(Duration::from_millis(1)),
        CoverMotion::Still
    );
}

#[test]
fn a_crossfade_reports_crossfading_until_a_paint_settles_it() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Plain;
    sources.appearance.window.animations = Animations::On;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.set_cover(cover("first.mp3", Rgba([200, 10, 10, 255])));
    pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Withheld,
            wash: CoverWash::Idle,
        },
        Duration::ZERO,
    ));

    pixels.set_cover(cover("second.mp3", Rgba([10, 10, 200, 255])));
    pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        },
        Duration::from_millis(1),
    ));

    let elapsed = Duration::from_millis(1) + crossfade_duration();
    assert_eq!(pixels.cover_motion(elapsed), CoverMotion::Crossfading);

    pixels.refresh(sources.sources_at(
        CoverPlacement {
            layout,
            fade: CoverFade::Allowed,
            wash: CoverWash::Idle,
        },
        elapsed,
    ));
    assert_eq!(pixels.cover_motion(elapsed), CoverMotion::Still);
}

#[test]
fn a_wider_rect_forces_a_rebuild_without_a_crossfade() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Plain;
    sources.appearance.window.animations = Animations::On;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    pixels.set_cover(cover("song.mp3", Rgba([200, 10, 10, 255])));

    pixels.refresh(sources.sources(CoverPlacement {
        layout: layout_with_cover(Some(cover_rect())),
        fade: CoverFade::Allowed,
        wash: CoverWash::Idle,
    }));
    let wider = Rect::new(0, 0, 16, 4);
    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout: layout_with_cover(Some(wider)),
        fade: CoverFade::Allowed,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(art, OwnedCoverArt::Image));
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[test]
fn a_settled_theme_wash_ends_on_the_new_image_and_goes_still() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Vinyl;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));

    let _ = sources.model.revisions.theme.bump();
    let mid = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Running {
            progress: 0.2,
            screen_width: 80,
        },
    }));
    assert!(matches!(mid, OwnedCoverArt::Image));
    assert_eq!(
        pixels.cover_motion(Duration::ZERO),
        CoverMotion::Crossfading
    );

    let settled = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));
    assert!(matches!(settled, OwnedCoverArt::Image));
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}

#[test]
fn a_theme_change_with_no_wash_staged_installs_the_new_image_at_once() {
    let mut sources = Scenery::new(playing_model("moon-river", 200, 50));
    sources.appearance.cover.style = CoverStyle::Vinyl;
    sources.appearance.window.animations = Animations::Off;
    let mut pixels = CoverRenderer::new(Picker::halfblocks());
    let layout = layout_with_cover(Some(cover_rect()));

    pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));

    let _ = sources.model.revisions.theme.bump();
    let art = pixels.refresh(sources.sources(CoverPlacement {
        layout,
        fade: CoverFade::Withheld,
        wash: CoverWash::Idle,
    }));

    assert!(matches!(art, OwnedCoverArt::Image));
    assert_eq!(pixels.cover_motion(Duration::ZERO), CoverMotion::Still);
}
