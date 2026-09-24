use config::Animations;
use kernel::{Cue, PlaybackChange};
use ratatui::style::Color;

use crate::{
    animation::{
        catalogue::{
            VolumeShades,
            chip_pulse,
            delete_burst,
            favorite_pulse,
            modal_in,
            modal_out,
            row_flash,
            screen_wash,
            toast_burst,
            toast_slide_in,
            volume_pulse,
        },
        stage::{AnimationStage, Backdrop, VacatedAreas},
    },
    overlay::modal::OverlayAreas,
    playlist::favorite_cell,
};

impl AnimationStage {
    pub fn play(&mut self, cues: Vec<Cue>, backdrop: &Backdrop) {
        self.remember_protected(backdrop.layout);
        if backdrop.animations == Animations::On {
            let running = self.lift();
            for cue in once_each(cues) {
                self.stage_cue(cue, backdrop);
            }
            self.keep_behind(running);
        } else {
            self.clear();
        }
        let layout = backdrop.layout;
        self.vacated = VacatedAreas {
            overlay: layout.overlay.map(OverlayAreas::painted),
            toast: layout.toast.map(|toast| toast.painted),
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
                    layout.overlay.map(OverlayAreas::painted),
                    modal_in(timings),
                );
            }
            Cue::OverlayClosed => self.stage_at(vacated.overlay, modal_out(timings)),
            Cue::ToastRaised => {
                self.stage_at(
                    layout.toast.map(|toast| toast.painted),
                    toast_slide_in(backdrop.background, timings),
                );
            }
            Cue::ToastDismissed => {
                self.stage_at(
                    vacated.toast,
                    toast_burst(backdrop.background, self.cell_filter(), timings),
                );
            }
            Cue::PlaybackChanged(change) => {
                self.stage_at(
                    layout.card.map(|metrics| metrics.status_row),
                    chip_pulse(pulsed(change, backdrop), timings),
                );
            }
            Cue::FavoriteToggled => self.stage_favorite_toggled(backdrop),
            Cue::VolumeChanged => self.stage_volume_changed(backdrop),
            Cue::TrackDeleted => self.stage_at(
                vacated.selected_row,
                delete_burst(backdrop.background, self.cell_filter(), timings),
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
            selected.map(favorite_cell),
            row_flash(backdrop.accent, timings),
        );
        self.stage_at(
            layout.card.map(|metrics| metrics.title_row),
            favorite_pulse(backdrop.accent, timings),
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
        self.stage_at(layout.card.map(|metrics| metrics.volume_row), pulse);
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
    use config::Animations;
    use kernel::{Cue, PlaybackChange};
    use ratatui::{layout::Rect, style::Color};

    use crate::{
        animation::{
            play::{once_each, pulsed},
            stage::{AnimationStage, Backdrop},
        },
        screen::{Breakpoint, FrameLayout},
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

        assert_eq!(stage.staged(), 1);
    }
}
