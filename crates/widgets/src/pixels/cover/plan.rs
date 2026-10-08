use ratatui::layout::Rect;

use crate::pixels::{cover::pixmap::Identity, vinyl::Wanted};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintPlan {
    Reuse,
    Rebuild,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct PlacedCover<'a> {
    pub(crate) identity: &'a Identity,
    pub(crate) rect: Rect,
}

#[must_use]
pub(crate) fn plan_paint(
    painted_cover: Option<PlacedCover<'_>>,
    wanted: &Wanted<'_>,
    rect: Rect,
) -> PaintPlan {
    if let Some(painted) = painted_cover
        && painted.rect == rect
        && painted.identity.is_wanted(wanted)
    {
        PaintPlan::Reuse
    } else {
        PaintPlan::Rebuild
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use image::{Rgba, RgbaImage};
    use kernel::domain::{appearance::Rgb, geometry::Pixels};
    use ratatui::layout::Rect;
    use rstest::rstest;

    use crate::pixels::{
        cover::{
            CoverImage,
            pixmap::Identity,
            plan::{PaintPlan, PlacedCover, plan_paint},
        },
        vinyl::{VinylCacheKey, VinylStyle, Wanted, tests::noir_vinyl_style},
    };

    fn cover_image(path: &str) -> CoverImage {
        CoverImage {
            path: PathBuf::from(path),
            image: Arc::new(RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]))),
        }
    }

    struct Want {
        path: &'static str,
        vinyl_style: VinylStyle,
        rect: Rect,
    }

    struct PlanRow {
        painted: Option<(Identity, Rect)>,
        want: Want,
    }

    fn want(path: &'static str, rect: Rect) -> Want {
        Want {
            path,
            vinyl_style: noir_vinyl_style(),
            rect,
        }
    }

    fn rect() -> Rect {
        Rect::new(0, 0, 10, 10)
    }

    fn other_rect() -> Rect {
        Rect::new(0, 0, 12, 10)
    }

    fn plain(path: &str) -> Identity {
        Identity::Plain(PathBuf::from(path))
    }

    fn vinyl_with_colors(path: &str, vinyl_style: VinylStyle) -> Identity {
        Identity::Vinyl(VinylCacheKey {
            path: Some(PathBuf::from(path)),
            side: Pixels(128),
            vinyl_style,
        })
    }

    fn vinyl(path: &str) -> Identity {
        vinyl_with_colors(path, noir_vinyl_style())
    }

    fn recolored() -> VinylStyle {
        VinylStyle {
            accent: Rgb([0x3d, 0x9b, 0xff]),
            ..noir_vinyl_style()
        }
    }

    #[rstest]
    #[case::plain_nothing_installed(
        PlanRow { painted: None, want: want("a.jpg", rect()) },
        PaintPlan::Rebuild
    )]
    #[case::plain_same_path_and_rect(
        PlanRow {
            painted: Some((plain("a.jpg"), rect())),
            want: want("a.jpg", rect()),
        },
        PaintPlan::Reuse
    )]
    #[case::plain_a_different_path(
        PlanRow {
            painted: Some((plain("a.jpg"), rect())),
            want: want("b.jpg", rect()),
        },
        PaintPlan::Rebuild
    )]
    #[case::plain_a_different_rect(
        PlanRow {
            painted: Some((plain("a.jpg"), rect())),
            want: want("a.jpg", other_rect()),
        },
        PaintPlan::Rebuild
    )]
    #[case::vinyl_same_key_and_rect(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            want: want("a.flac", rect()),
        },
        PaintPlan::Reuse
    )]
    #[case::vinyl_a_different_key(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            want: want("b.flac", rect()),
        },
        PaintPlan::Rebuild
    )]
    #[case::vinyl_only_the_colors_moved(
        PlanRow {
            painted: Some((vinyl("a.flac"), rect())),
            want: Want { vinyl_style: recolored(), ..want("a.flac", rect()) },
        },
        PaintPlan::Rebuild
    )]
    fn plan_paint_decides_reuse_or_rebuild(
        #[case] plan_row: PlanRow,
        #[case] expected: PaintPlan,
    ) {
        let PlanRow { painted, want } = plan_row;
        let painted = painted.as_ref().map(|(identity, rect)| PlacedCover {
            identity,
            rect: *rect,
        });
        let cover_image = cover_image(want.path);
        let wanted = Wanted {
            cover_image: Some(&cover_image),
            side: Pixels(128),
            vinyl_style: want.vinyl_style,
        };
        assert_eq!(plan_paint(painted, &wanted, want.rect), expected);
    }
}
