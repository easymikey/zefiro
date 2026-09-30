#![cfg(test)]

use std::{sync::Arc, time::Duration};

use config::{AppearanceFile, Rgb};
use kernel::{
    Moment,
    domain::{
        AudioFormat,
        KeymapOverrides,
        Model,
        Player,
        Playhead,
        Preload,
        Speed,
        Tags,
        Track,
    },
    playlist::Playlist,
    update::keymap::{Bindings, KeyBinding},
};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use widgets::{
    AnimationTimings,
    Backdrop,
    Breakpoint,
    CardMetrics,
    CellAspect,
    ColorDepth,
    FrameLayout,
    PixelPath,
    PlaylistAreas,
    SPECTRUM_BANDS,
    Scene,
    Spectrum,
    Theme,
    color_at_depth,
    lerp_rgb,
};

pub(crate) fn noir_theme() -> Theme {
    let file =
        config::parse_theme(include_str!("../../../../themes/noir.toml"), "noir")
            .unwrap();
    Theme::from(file)
}

pub(crate) fn bindings() -> Vec<KeyBinding> {
    Bindings::new(&KeymapOverrides::default())
        .as_slice()
        .to_vec()
}

pub(crate) fn silent_spectrum() -> Spectrum {
    [0.0; SPECTRUM_BANDS]
}

pub(crate) fn track(title: &str) -> Arc<Track> {
    Arc::new(
        Track::builder()
            .path(format!("/music/{title}.mp3"))
            .duration(Duration::from_secs(245))
            .tags(Tags {
                title: Some(title.to_string()),
                artist: Some("Test Artist".to_string()),
                ..Tags::default()
            })
            .audio_format(AudioFormat::default())
            .build(),
    )
}

pub(crate) fn playing_track(title: &str) -> Model {
    let track = track(title);
    Model {
        player: Player::Playing {
            track,
            head: Playhead::anchored(
                Duration::from_secs(30),
                Moment::default(),
                Speed::default(),
            ),
            preload: Preload::None,
        },
        ..Model::default()
    }
}

pub(crate) fn model_with_tracks(count: usize) -> Model {
    Model {
        playlist: Playlist {
            tracks: (0..count)
                .map(|index| track(&format!("song{index:02}")))
                .collect(),
            ..Playlist::default()
        },
        ..Model::default()
    }
}

#[derive(Debug)]
pub(crate) struct Scenery {
    pub(crate) model: Model,
    pub(crate) theme: Theme,
    pub(crate) appearance: AppearanceFile,
    pub(crate) bindings: Vec<KeyBinding>,
    pub(crate) spectrum: Spectrum,
}

impl Scenery {
    #[must_use]
    pub(crate) fn new(model: Model) -> Self {
        Self {
            model,
            theme: noir_theme(),
            appearance: AppearanceFile::default(),
            bindings: bindings(),
            spectrum: silent_spectrum(),
        }
    }

    #[must_use]
    pub(crate) fn scene(&self) -> Scene<'_> {
        Scene {
            model: &self.model,
            theme: &self.theme,
            color_depth: ColorDepth::TrueColor,
            appearance: &self.appearance,
            bindings: &self.bindings,
            spectrum: &self.spectrum,
            pixel_path: PixelPath::Halfblocks,
            cell_aspect: CellAspect::default(),
            clock: Duration::ZERO,
            now: Moment::default(),
            music_dir: "/home/user/Music",
            sleep_left: None,
        }
    }
}

pub(crate) fn painted<W>(widget: &W, width: u16, height: u16) -> String
where
    for<'a> &'a W: Widget,
{
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(widget, frame.area()))
        .unwrap();
    format!("{}", terminal.backend())
}

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
    widgets::favorite_cell(PANE_ROW)
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
    let mixed = lerp_rgb(
        VOLUME_FILL,
        TEXT_HEX,
        AnimationTimings::default().volume_pulse_mix,
    );
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
        row_width: 0,
        status_row,
        title_row,
        artist_row: Rect::default(),
        time_row: Rect::default(),
        progress_row: Rect::default(),
        spectrum_row: Rect::default(),
        volume_row,
    }
}

pub(crate) fn playlist_areas(selected: Option<Rect>) -> PlaylistAreas {
    PlaylistAreas {
        pane: Rect::default(),
        body: Rect::default(),
        rows: Rect::default(),
        scrollbar: Rect::default(),
        selected,
    }
}

pub(crate) fn empty_layout(screen: Rect) -> FrameLayout {
    FrameLayout {
        screen,
        breakpoint: Breakpoint::Full,
        content: Rect::default(),
        header: Rect::default(),
        card: None,
        cover: None,
        playlist_pane: Rect::default(),
        playlist: None,
        key_hints: None,
        search_bounds: Rect::default(),
        overlay: None,
        toast: None,
    }
}

pub(crate) fn quiet_backdrop() -> Backdrop {
    Backdrop {
        animations: config::Animations::On,
        layout: empty_layout(Rect::default()),
        background: BACKGROUND,
        accent: ACCENT,
        volume_fill: volume_fill(),
        volume_lifted: volume_lifted(),
        wash_from: BACKGROUND,
    }
}

pub(crate) fn screen_backdrop() -> Backdrop {
    Backdrop {
        layout: empty_layout(SCREEN),
        ..quiet_backdrop()
    }
}

pub(crate) fn pane_backdrop() -> Backdrop {
    Backdrop {
        layout: FrameLayout {
            card: Some(card_metrics(PANE_STATUS, CARD_TITLE, VOLUME_LABEL)),
            playlist: Some(playlist_areas(Some(PANE_ROW))),
            ..empty_layout(Rect::default())
        },
        ..quiet_backdrop()
    }
}

pub(crate) fn chip_backdrop() -> Backdrop {
    Backdrop {
        layout: FrameLayout {
            card: Some(card_metrics(AREA, Rect::default(), Rect::default())),
            ..empty_layout(Rect::default())
        },
        ..quiet_backdrop()
    }
}

pub(crate) fn overlay_backdrop(overlay: Option<Rect>) -> Backdrop {
    Backdrop {
        layout: FrameLayout {
            overlay: overlay.map(widgets::OverlayAreas::Banner),
            ..empty_layout(Rect::default())
        },
        ..quiet_backdrop()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ToastPresence {
    Shown,
    Hidden,
}

pub(crate) fn toast_backdrop(presence: ToastPresence) -> Backdrop {
    let toast = match presence {
        ToastPresence::Shown => Some(widgets::ToastAreas {
            outer: AREA,
            painted: AREA,
        }),
        ToastPresence::Hidden => None,
    };
    Backdrop {
        layout: FrameLayout {
            toast,
            ..empty_layout(Rect::default())
        },
        ..quiet_backdrop()
    }
}

pub(crate) fn toast_card_backdrop() -> Backdrop {
    Backdrop {
        layout: FrameLayout {
            toast: Some(widgets::ToastAreas {
                outer: TOAST_CARD,
                painted: TOAST_CARD,
            }),
            ..empty_layout(Rect::default())
        },
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
    Duration::from_millis(u64::from(
        pick(AnimationTimings::default()).0 / parts.max(1),
    ))
}
