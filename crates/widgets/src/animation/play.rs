use kernel::domain::{
    appearance::Animations,
    cue::{Cue, PlaybackChange},
};
use ratatui::style::Color;

use crate::{
    animation::{
        catalogue::{
            chip_pulse,
            modal_reveal,
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
        match backdrop.animations {
            Animations::On if cues.is_empty() => {}
            Animations::On => {
                let running = self.take_running();
                for cue in once_each(cues) {
                    self.stage_cue(cue, backdrop);
                }
                self.restore_running(running);
            }
            Animations::Off => self.clear(),
        }
        let layout = backdrop.layout;
        self.vacated_areas = VacatedAreas {
            overlay: layout.overlay_areas.map(OverlayAreas::outer),
            toast: layout.toast,
            selected_row: layout
                .playlist_areas
                .and_then(|playlist| playlist.selected_area),
        };
    }

    fn stage_cue(&mut self, cue: Cue, backdrop: &Backdrop) {
        let vacated = self.vacated_areas;
        let layout = backdrop.layout;
        match cue {
            Cue::OverlayOpened => {
                self.stage_at(
                    modal_reveal(),
                    layout.overlay_areas.map(OverlayAreas::outer),
                );
            }
            Cue::OverlayClosed => self.stage_at(modal_reveal(), vacated.overlay),
            Cue::ToastRaised => {
                self.stage_at(toast_slide_in(backdrop.style.background), layout.toast);
            }
            Cue::ToastDismissed => {
                self.stage_at(
                    scatter_burst(backdrop.style.background, self.cell_filter()),
                    vacated.toast,
                );
            }
            Cue::PlaybackChanged(change) => {
                self.stage_at(
                    chip_pulse(pulsed(change, backdrop)),
                    layout.card_metrics.map(|metrics| metrics.status_row),
                );
            }
            Cue::FavoriteToggled => self.stage_favorite_toggled(backdrop),
            Cue::VolumeChanged => self.stage_volume_changed(backdrop),
            Cue::TrackTrashed => self.stage_at(
                scatter_burst(backdrop.style.background, self.cell_filter()),
                vacated.selected_row,
            ),
            Cue::ThemeChanged | Cue::LayoutChanged => {
                self.stage_whole_screen(screen_wash(backdrop.wash_from), layout.screen);
            }
            Cue::TrackChanged
            | Cue::QueueChanged
            | Cue::PlayOrderChanged
            | Cue::LibraryOpened => {}
        }
    }

    fn stage_favorite_toggled(&mut self, backdrop: &Backdrop) {
        let layout = backdrop.layout;
        let selected = layout
            .playlist_areas
            .and_then(|playlist| playlist.selected_area);
        self.stage_at(
            row_flash(backdrop.style.accent),
            selected.map(favorite_cell),
        );
        self.stage_at(
            chip_pulse(backdrop.style.accent),
            layout.card_metrics.map(|metrics| metrics.title_row),
        );
    }

    fn stage_volume_changed(&mut self, backdrop: &Backdrop) {
        let layout = backdrop.layout;
        let pulse = volume_pulse(
            backdrop.style.volume_fill,
            backdrop.style.volume_lifted,
            self.cell_filter(),
        );
        self.stage_at(pulse, layout.card_metrics.map(|metrics| metrics.volume_row));
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
        PlaybackChange::Play => backdrop.style.background,
        PlaybackChange::Pause | PlaybackChange::Stop => backdrop.style.accent,
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
            catalogue::chip_pulse,
            play::{once_each, pulsed},
            stage::{AnimationStage, Backdrop},
        },
        screen::{breakpoint::Breakpoint, frame_layout::FrameLayout},
        theme::backdrop_style::BackdropStyle,
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
            layout: FrameLayout::empty(Rect::default(), Breakpoint::Full),
            style: BackdropStyle {
                background: Color::Rgb(0, 0, 0),
                accent: Color::Rgb(240, 120, 40),
                volume_fill: Color::Rgb(220, 80, 160),
                volume_lifted: Color::Rgb(200, 210, 220),
            },
            wash_from: Color::Rgb(0, 0, 0),
        }
    }

    #[test]
    fn pulsed_answers_the_background_when_playback_starts() {
        let backdrop = empty_backdrop();
        assert_eq!(
            pulsed(PlaybackChange::Play, &backdrop),
            backdrop.style.background
        );
    }

    #[test]
    fn pulsed_answers_the_accent_when_playback_pauses_or_stops() {
        let backdrop = empty_backdrop();
        assert_eq!(
            pulsed(PlaybackChange::Pause, &backdrop),
            backdrop.style.accent
        );
        assert_eq!(
            pulsed(PlaybackChange::Stop, &backdrop),
            backdrop.style.accent
        );
    }

    #[test]
    fn a_layout_change_stages_one_wash_over_the_whole_screen() {
        let mut stage = AnimationStage::default();
        let backdrop = empty_backdrop();

        stage.play(vec![Cue::LayoutChanged], &backdrop);

        assert!(stage.wash_progress().is_some());
        assert!(stage.take_running().is_empty());
    }

    fn staged_progress(stage: &mut AnimationStage) -> Vec<(Rect, Option<f32>)> {
        let running = stage.take_running();
        let staged = running
            .iter()
            .map(|(animation, rect)| {
                (*rect, animation.timer().map(|timer| timer.alpha()))
            })
            .collect();
        stage.restore_running(running);
        staged
    }

    #[test]
    fn a_running_animation_stays_staged_without_cues() {
        let mut stage = AnimationStage::default();
        let area = Rect::new(2, 3, 10, 1);
        stage.stage(chip_pulse(Color::Rgb(240, 120, 40)), area);
        let before = staged_progress(&mut stage);

        stage.play(Vec::new(), &empty_backdrop());

        assert_eq!(before.len(), 1);
        assert_eq!(before[0].0, area);
        assert_eq!(staged_progress(&mut stage), before);
    }
}
