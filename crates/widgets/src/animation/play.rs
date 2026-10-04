use kernel::domain::{
    appearance::Animations,
    cue::{Cue, PlaybackChange},
};
use ratatui::style::Color;

use crate::{
    animation::{
        catalogue::{
            VolumeShades,
            chip_pulse,
            favorite_pulse,
            modal_in,
            modal_out,
            row_flash,
            scatter_burst,
            screen_wash,
            toast_slide_in,
            volume_pulse,
        },
        stage::{AnimationStage, Backdrop, VacatedAreas},
    },
    overlay::modal::placement::OverlayAreas,
    playlist::row::favorite_cell,
};

impl AnimationStage {
    pub fn play(&mut self, cues: Vec<Cue>, backdrop: &Backdrop) {
        self.remember_protected(backdrop.layout);
        if backdrop.animations == Animations::On {
            let running = self.take_running();
            for cue in once_each(cues) {
                self.stage_cue(cue, backdrop);
            }
            self.restore_running(running);
        } else {
            self.clear();
        }
        let layout = backdrop.layout;
        self.vacated = VacatedAreas {
            overlay: layout.overlay.map(OverlayAreas::outer),
            toast: layout.toast,
            selected_row: layout.playlist.and_then(|playlist| playlist.selected),
        };
    }

    fn stage_cue(&mut self, cue: Cue, backdrop: &Backdrop) {
        let timings = self.timings;
        let vacated = self.vacated;
        let layout = backdrop.layout;
        match cue {
            Cue::OverlayOpened => {
                self.stage_at(
                    modal_in(timings),
                    layout.overlay.map(OverlayAreas::outer),
                );
            }
            Cue::OverlayClosed => self.stage_at(modal_out(timings), vacated.overlay),
            Cue::ToastRaised => {
                self.stage_at(
                    toast_slide_in(backdrop.background, timings),
                    layout.toast,
                );
            }
            Cue::ToastDismissed => {
                self.stage_at(
                    scatter_burst(backdrop.background, self.cell_filter(), timings),
                    vacated.toast,
                );
            }
            Cue::PlaybackChanged(change) => {
                self.stage_at(
                    chip_pulse(pulsed(change, backdrop), timings),
                    layout.card.map(|metrics| metrics.status_row),
                );
            }
            Cue::FavoriteToggled => self.stage_favorite_toggled(backdrop),
            Cue::VolumeChanged => self.stage_volume_changed(backdrop),
            Cue::TrackDeleted => self.stage_at(
                scatter_burst(backdrop.background, self.cell_filter(), timings),
                vacated.selected_row,
            ),
            Cue::ThemeChanged | Cue::LayoutChanged => {
                self.stage_whole_screen(
                    screen_wash(backdrop.wash_from, timings),
                    layout.screen,
                );
            }
            Cue::TrackChanged
            | Cue::QueueChanged
            | Cue::PlayOrderChanged
            | Cue::LibraryOpened => {}
        }
    }

    fn stage_favorite_toggled(&mut self, backdrop: &Backdrop) {
        let timings = self.timings;
        let layout = backdrop.layout;
        let selected = layout.playlist.and_then(|playlist| playlist.selected);
        self.stage_at(
            row_flash(backdrop.accent, timings),
            selected.map(favorite_cell),
        );
        self.stage_at(
            favorite_pulse(backdrop.accent, timings),
            layout.card.map(|metrics| metrics.title_row),
        );
    }

    fn stage_volume_changed(&mut self, backdrop: &Backdrop) {
        let timings = self.timings;
        let layout = backdrop.layout;
        let shades = VolumeShades {
            fill: backdrop.volume_fill,
            lifted: backdrop.volume_lifted,
        };
        let pulse = volume_pulse(shades, self.cell_filter(), timings);
        self.stage_at(pulse, layout.card.map(|metrics| metrics.volume_row));
    }
}

fn once_each(cues: Vec<Cue>) -> Vec<Cue> {
    cues.into_iter().fold(Vec::new(), |mut once, cue| {
        if !once.contains(&cue) {
            once.push(cue);
        }
        once
    })
}

fn pulsed(change: PlaybackChange, backdrop: &Backdrop) -> Color {
    match change {
        PlaybackChange::Play => backdrop.background,
        PlaybackChange::Pause | PlaybackChange::Stop => backdrop.accent,
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::{
        appearance::Animations,
        cue::{Cue, PlaybackChange},
    };
    use ratatui::{layout::Rect, style::Color};

    use crate::{
        animation::{
            play::{once_each, pulsed},
            stage::{AnimationStage, Backdrop},
        },
        screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
    };

    #[test]
    fn once_each_keeps_the_first_occurrence_and_drops_repeats() {
        let cues = vec![
            Cue::FavoriteToggled,
            Cue::VolumeChanged,
            Cue::FavoriteToggled,
        ];
        assert_eq!(
            once_each(cues),
            vec![Cue::FavoriteToggled, Cue::VolumeChanged]
        );
    }

    #[test]
    fn once_each_leaves_a_list_without_repeats_untouched() {
        let cues = vec![Cue::TrackChanged, Cue::QueueChanged];
        assert_eq!(once_each(cues.clone()), cues);
    }

    fn empty_backdrop() -> Backdrop {
        Backdrop {
            animations: Animations::On,
            layout: FrameLayout {
                screen: Rect::default(),
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
            },
            background: Color::Rgb(0, 0, 0),
            accent: Color::Rgb(240, 120, 40),
            volume_fill: Color::Rgb(220, 80, 160),
            volume_lifted: Color::Rgb(200, 210, 220),
            wash_from: Color::Rgb(0, 0, 0),
        }
    }

    #[test]
    fn pulsed_answers_the_background_when_playback_starts() {
        let backdrop = empty_backdrop();
        assert_eq!(pulsed(PlaybackChange::Play, &backdrop), backdrop.background);
    }

    #[test]
    fn pulsed_answers_the_accent_when_playback_pauses_or_stops() {
        let backdrop = empty_backdrop();
        assert_eq!(pulsed(PlaybackChange::Pause, &backdrop), backdrop.accent);
        assert_eq!(pulsed(PlaybackChange::Stop, &backdrop), backdrop.accent);
    }

    #[test]
    fn a_layout_change_stages_one_wash_over_the_whole_screen() {
        let mut stage = AnimationStage::default();
        let backdrop = empty_backdrop();

        stage.play(vec![Cue::LayoutChanged], &backdrop);

        assert_eq!(stage.take_running().len(), 1);
    }
}
