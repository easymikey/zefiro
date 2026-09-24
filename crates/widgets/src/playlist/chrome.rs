use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders},
};

use crate::{
    playlist::pane::PlaylistView,
    primitive::{inset::Inset, list_chrome::spaced_title},
    status_line::{self, ScanProgress, Shuffle, StatusLineColors, StatusLineView},
    theme::ActiveTheme,
};

const TITLE_CELLS: u16 = 2;
const BORDER_COLUMNS: u16 = 2;

pub(crate) fn pane_block(
    title: Option<Line<'static>>,
    border: Color,
) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Thick)
        .border_style(Style::default().fg(border))
        .padding(Inset::default().padding());
    match title {
        Some(title) => block.title(spaced_title(title)),
        None => block,
    }
}

fn title_budget(area: Rect) -> usize {
    usize::from(
        area.width
            .saturating_sub(BORDER_COLUMNS)
            .saturating_sub(TITLE_CELLS),
    )
}

pub(crate) fn pane_title(
    area: Rect,
    view: PlaylistView<'_>,
    theme: ActiveTheme<'_>,
) -> Line<'static> {
    let shuffle = if view.playlist.play_order.is_shuffle() {
        Shuffle::On
    } else {
        Shuffle::Off
    };
    let status = StatusLineView {
        shuffle,
        repeat_mode: view.playlist.repeat,
        queue_len: view.queue.len(),
        position: view.browse_selected,
        total: view.playlist.tracks.len(),
        scan: ScanProgress::of(view.scan, theme.scanning_label.as_str()),
        theme_name: theme.name.as_str(),
        sleep_left: view.sleep_left,
    };
    let colors = StatusLineColors {
        frame: theme.frame(),
        dim: theme.dim(),
        accent: theme.accent(),
    };
    status_line::build(status, colors, title_budget(area))
}

#[cfg(test)]
mod tests {
    use kernel::{
        domain::{Favorites, ScanStatus},
        playlist::{PlayOrder, Playlist, RepeatMode},
    };
    use ratatui::layout::Rect;

    use crate::{
        playlist::{
            chrome::pane_title,
            pane::{LibraryLoad, PlaylistView},
        },
        scene::fixtures::noir,
        theme::{ActiveTheme, ColorDepth},
    };

    fn view<'a>(playlist: &'a Playlist, favorites: &'a Favorites) -> PlaylistView<'a> {
        PlaylistView {
            playlist,
            queue: &[],
            favorites,
            browse_selected: 0,
            playing: None,
            library_loading: LibraryLoad::Ready,
            scan: ScanStatus::Idle,
            sleep_left: None,
        }
    }

    #[test]
    fn the_title_names_the_pane_its_position_and_its_flags() {
        let playlist = Playlist {
            play_order: PlayOrder::ShufflePending,
            repeat: RepeatMode::All,
            ..Playlist::default()
        };
        let favorites = Favorites::default();
        let theme = noir();
        let theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let line =
            pane_title(Rect::new(0, 0, 80, 1), view(&playlist, &favorites), theme);
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains("shuffle on"), "got {text:?}");
        assert!(text.contains("repeat all"), "got {text:?}");
    }

    #[test]
    fn a_narrow_border_truncates_the_title_with_an_ellipsis() {
        let playlist = Playlist::default();
        let favorites = Favorites::default();
        let theme = noir();
        let theme = ActiveTheme::new(&theme, ColorDepth::TrueColor);
        let line =
            pane_title(Rect::new(0, 0, 24, 1), view(&playlist, &favorites), theme);
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.ends_with('…'), "got {text:?}");
    }
}
