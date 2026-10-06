use std::{path::PathBuf, time::Duration};

use kernel::{
    cmd::{CoverJob, Effect, LibraryCmd},
    domain::{
        appearance::{AppearanceSettings, CoverMode},
        geometry::{Cells, Pixels},
        model::Model,
        time::Moment,
    },
    message::{ConfigEvent, Message, PlaybackRequest},
};
use rstest::rstest;

use crate::support::{model_playing_at, update::update};

const SIDE: Pixels = Pixels(240);
const WIDER: Pixels = Pixels(320);

fn shown(cover_mode: CoverMode, side: Option<Pixels>) -> Model {
    let mut model = model_playing_at(3, 0, Duration::ZERO);
    model.settings.appearance_settings.cover_mode = cover_mode;
    model.workspace.cover_side = side;
    model
}

fn viewport(side: Option<Pixels>) -> Message {
    Message::Viewport {
        visible_rows: Cells(10),
        cover_side: side,
    }
}

fn next() -> Message {
    Message::Playback(PlaybackRequest::Next)
}

fn switched_to(cover_mode: CoverMode) -> Message {
    Message::Config(ConfigEvent::AppearanceReloaded(AppearanceSettings {
        cover_mode,
        ..AppearanceSettings::default()
    }))
}

fn job(track_number: usize, side: Pixels) -> CoverJob {
    CoverJob {
        path: PathBuf::from(format!("/tmp/track{track_number}.flac")),
        side,
    }
}

#[rstest]
#[case::track_change(shown(CoverMode::Plain, Some(SIDE)), next(), vec![job(1, SIDE)])]
#[case::side_change(shown(CoverMode::Vinyl, Some(SIDE)), viewport(Some(WIDER)), vec![job(0, WIDER)])]
#[case::no_change(shown(CoverMode::Vinyl, Some(SIDE)), viewport(Some(SIDE)), vec![])]
#[case::side_none(shown(CoverMode::Plain, Some(SIDE)), viewport(None), vec![])]
#[case::track_change_without_side(shown(CoverMode::Plain, None), next(), vec![])]
#[case::mode_off(shown(CoverMode::Off, None), viewport(Some(SIDE)), vec![])]
#[case::mode_milkdrop(shown(CoverMode::Milkdrop, None), viewport(Some(SIDE)), vec![])]
#[case::milkdrop_to_plain(shown(CoverMode::Milkdrop, Some(SIDE)), switched_to(CoverMode::Plain), vec![job(0, SIDE)])]
#[case::plain_to_off(shown(CoverMode::Plain, Some(SIDE)), switched_to(CoverMode::Off), vec![])]
fn the_kernel_decodes_a_cover_when_the_shown_one_changes(
    #[case] mut model: Model,
    #[case] message: Message,
    #[case] expected: Vec<CoverJob>,
) {
    let cmd = update(&mut model, message, Moment::default()).unwrap();

    let decoded_jobs: Vec<CoverJob> = cmd
        .effects()
        .filter_map(|effect| {
            let Effect::Library(LibraryCmd::DecodeCover(job)) = effect else {
                return None;
            };
            Some(job.clone())
        })
        .collect();
    assert_eq!(decoded_jobs, expected);
}
