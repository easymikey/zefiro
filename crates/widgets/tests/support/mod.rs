#![cfg(test)]

use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use kernel::domain::{
    appearance::{Breakpoints, Rgb},
    geometry::Cells,
    model::Model,
    player::Player,
    playhead::Playhead,
    speed::Speed,
    time::Moment,
    toast::Toast,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};
use widgets::{
    animation::{
        catalogue::PaintedCell,
        stage::Backdrop,
        timings::{AnimationTimings, TIMINGS},
    },
    card::metrics::CardMetrics,
    playlist::row::{PlaylistAreas, RowWindow},
    screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
    theme::{
        backdrop_style::BackdropStyle,
        rgb::{ColorDepth, color_at_depth, lerp_rgb},
    },
};

pub(crate) mod fixtures;

use fixtures::{SceneSources, track};

pub(crate) const AREA: Rect = Rect {
    x: 0,
    y: 0,
    width: 8,
    height: 1,
};

pub(crate) const SCREEN: Rect = Rect {
    x: 0,
    y: 0,
    width: 24,
    height: 6,
};

pub(crate) const COVER: Rect = Rect {
    x: 0,
    y: 0,
    width: 6,
    height: 4,
};

pub(crate) const PROGRESS_LINE: Rect = Rect {
    x: 8,
    y: 5,
    width: 10,
    height: 1,
};

pub(crate) const CARD_TITLE: Rect = Rect {
    x: 0,
    y: 0,
    width: 24,
    height: 1,
};

pub(crate) const PANE_STATUS: Rect = Rect {
    x: 0,
    y: 3,
    width: 24,
    height: 1,
};

pub(crate) const PANE_ROW: Rect = Rect {
    x: 0,
    y: 4,
    width: 18,
    height: 1,
};

pub(crate) const VOLUME_LABEL: Rect = Rect {
    x: 18,
    y: 5,
    width: 6,
    height: 1,
};

pub(crate) fn pane_star() -> Rect {
    widgets::playlist::row::favorite_cell(PANE_ROW)
}

pub(crate) const TOAST_CARD: Rect = Rect {
    x: 13,
    y: 1,
    width: 10,
    height: 5,
};

pub(crate) const BACKGROUND: Color = Color::Rgb(0, 0, 0);
pub(crate) const TEXT: Color = Color::Rgb(200, 210, 220);
pub(crate) const ACCENT: Color = Color::Rgb(240, 120, 40);
pub(crate) const VOLUME_FILL: Rgb = Rgb([220, 80, 160]);
pub(crate) const VOLUME_GROOVE: Color = Color::Rgb(70, 60, 90);
pub(crate) const TEXT_HEX: Rgb = Rgb([200, 210, 220]);

pub(crate) fn volume_fill() -> Color {
    color_at_depth(VOLUME_FILL, ColorDepth::TrueColor)
}

pub(crate) fn volume_lifted() -> Color {
    let mixed = lerp_rgb(VOLUME_FILL, TEXT_HEX, TIMINGS.volume_pulse_mix);
    color_at_depth(mixed, ColorDepth::TrueColor)
}

fn filled_buffer(area: Rect) -> Buffer {
    let mut buffer = Buffer::empty(area);
    for row in 0..area.height {
        buffer.set_string(
            0,
            row,
            "X".repeat(usize::from(area.width)),
            Style::default().fg(TEXT).bg(BACKGROUND),
        );
    }
    buffer
}

pub(crate) fn animation_frame() -> Buffer {
    let mut buffer = Buffer::empty(AREA);
    buffer.set_string(0, 0, "SETTINGS", Style::default().fg(TEXT));
    buffer
}

pub(crate) fn screen_frame() -> Buffer {
    filled_buffer(SCREEN)
}

pub(crate) fn volume_bar_frame() -> Buffer {
    let mut buffer = filled_buffer(SCREEN);
    buffer.set_string(
        VOLUME_LABEL.x,
        VOLUME_LABEL.y,
        "━━━",
        Style::default().fg(volume_fill()).bg(BACKGROUND),
    );
    buffer.set_string(
        VOLUME_LABEL.x.saturating_add(3),
        VOLUME_LABEL.y,
        "━━━",
        Style::default().fg(VOLUME_GROOVE).bg(BACKGROUND),
    );
    buffer
}

pub(crate) fn card_metrics(
    status_row: Rect,
    title_row: Rect,
    volume_row: Rect,
) -> CardMetrics {
    CardMetrics {
        cover_square: Rect::default(),
        content_column: Rect::default(),
        row_width: Cells(0),
        status_row,
        title_row,
        artist_row: Rect::default(),
        time_row: Rect::default(),
        progress_row: Rect::default(),
        spectrum_row: Rect::default(),
        volume_row,
    }
}

pub(crate) fn playlist_areas(selected_area: Option<Rect>) -> PlaylistAreas {
    let sources = SceneSources::new(fixtures::model_with_tracks(1));
    let layout = FrameLayout::from_scene(&sources.scene(), Rect::new(0, 0, 120, 40));
    PlaylistAreas {
        window: RowWindow::default(),
        pane: Rect::default(),
        selected_area,
        ..layout
            .playlist_areas
            .expect("a stock scene lays out the playlist")
    }
}

pub(crate) fn quiet_backdrop() -> Backdrop<'static> {
    Backdrop {
        animations: kernel::domain::appearance::Animations::On,
        layout: FrameLayout::empty(Rect::default(), Breakpoint::Full),
        style: BackdropStyle {
            background: BACKGROUND,
            accent: volume_fill(),
            volume_lifted: volume_lifted(),
        },
        wash_from: Arc::default(),
    }
}

pub(crate) fn screen_backdrop() -> Backdrop<'static> {
    Backdrop {
        layout: FrameLayout::empty(SCREEN, Breakpoint::Full),
        wash_from: vec![
            PaintedCell {
                fg: ACCENT,
                bg: ACCENT,
            };
            usize::from(SCREEN.width) * usize::from(SCREEN.height)
        ]
        .into(),
        ..quiet_backdrop()
    }
}

pub(crate) fn pane_backdrop() -> Backdrop<'static> {
    Backdrop {
        layout: FrameLayout {
            card_metrics: Some(card_metrics(PANE_STATUS, CARD_TITLE, VOLUME_LABEL)),
            playlist_areas: Some(playlist_areas(Some(PANE_ROW))),
            ..FrameLayout::empty(Rect::default(), Breakpoint::Full)
        },
        ..quiet_backdrop()
    }
}

pub(crate) fn chip_backdrop() -> Backdrop<'static> {
    Backdrop {
        layout: FrameLayout {
            card_metrics: Some(card_metrics(AREA, Rect::default(), Rect::default())),
            ..FrameLayout::empty(Rect::default(), Breakpoint::Full)
        },
        ..quiet_backdrop()
    }
}

pub(crate) fn overlay_backdrop(overlay: Option<Rect>) -> Backdrop<'static> {
    Backdrop {
        layout: FrameLayout {
            overlay_areas: overlay
                .map(widgets::overlay::modal::placement::OverlayAreas::Banner),
            ..FrameLayout::empty(Rect::default(), Breakpoint::Full)
        },
        ..quiet_backdrop()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ToastPresence {
    Shown,
    Hidden,
}

const TOAST_CARD_SCREEN: Rect = Rect {
    x: 12,
    y: 0,
    width: 12,
    height: 6,
};

fn toast_sources(toast: Toast, full_min: Cells) -> SceneSources {
    let mut model = Model::default();
    model.workspace.toasts = vec![toast];
    let mut sources = SceneSources::new(model);
    sources.appearance_mut().breakpoints = Breakpoints {
        full_min_width: full_min,
        full_min_height: full_min,
        compact_min_width: full_min,
        compact_min_height: full_min,
        min_width: Cells(0),
        min_height: Cells(0),
    };
    sources
}

static TOAST_LINE_SOURCES: LazyLock<SceneSources> =
    LazyLock::new(|| toast_sources(Toast::info("Queued"), Cells(u16::MAX)));

static TOAST_CARD_SOURCES: LazyLock<SceneSources> = LazyLock::new(|| {
    toast_sources(Toast::info("Saved").with_text("one two"), Cells(0))
});

fn toast_layout(sources: &'static SceneSources, screen: Rect) -> FrameLayout<'static> {
    FrameLayout {
        toast_placement: FrameLayout::from_scene(&sources.scene(), screen)
            .toast_placement,
        ..FrameLayout::empty(Rect::default(), Breakpoint::Full)
    }
}

pub(crate) fn toast_backdrop(presence: ToastPresence) -> Backdrop<'static> {
    let layout = match presence {
        ToastPresence::Shown => toast_layout(&TOAST_LINE_SOURCES, AREA),
        ToastPresence::Hidden => FrameLayout::empty(Rect::default(), Breakpoint::Full),
    };
    Backdrop {
        layout,
        ..quiet_backdrop()
    }
}

pub(crate) fn toast_card_backdrop() -> Backdrop<'static> {
    Backdrop {
        layout: toast_layout(&TOAST_CARD_SOURCES, TOAST_CARD_SCREEN),
        ..quiet_backdrop()
    }
}

pub(crate) fn whole(
    pick: fn(AnimationTimings) -> (u32, tachyonfx::Interpolation),
) -> Duration {
    slice(pick, 1)
}

pub(crate) fn slice(
    pick: fn(AnimationTimings) -> (u32, tachyonfx::Interpolation),
    parts: u32,
) -> Duration {
    Duration::from_millis(u64::from(pick(TIMINGS).0 / parts.max(1)))
}

pub(crate) fn playing_track(title: &str) -> Model {
    Model {
        player: Player::Playing {
            track: track(title),
            playhead: Playhead::anchored(
                Duration::from_secs(30),
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        },
        ..Model::default()
    }
}
