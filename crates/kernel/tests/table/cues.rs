use kernel::{
    cmd::{Cmd, Effect},
    domain::{cue::Cue, model::Model, revision::Revision, time::Moment},
    message::{BrowseRequest, Message, Timer},
    update::machine::Unhandled,
};

use crate::support::{model_with_tracks, update::update};

fn cues(model: &mut Model, messages: Vec<Message>) -> Vec<Cue> {
    let mut seen = Vec::new();
    for message in messages {
        let cmd = update(model, message, Moment::default()).unwrap();
        seen.extend(found(&cmd));
    }
    seen
}

fn found(cmd: &Cmd) -> Vec<Cue> {
    cmd.effects()
        .filter_map(|effect| match effect {
            Effect::Animate(cue) => Some(*cue),
            Effect::Audio(_)
            | Effect::Library(_)
            | Effect::Macos(_)
            | Effect::Config(_)
            | Effect::Remote(_)
            | Effect::WindowColors(_)
            | Effect::RollShuffle(..)
            | Effect::After { .. }
            | Effect::Restart(_)
            | Effect::Quit => None,
        })
        .collect()
}

#[test]
fn a_toast_timer_without_a_toast_is_refused() {
    let mut model = model_with_tracks(3);
    let before = model.clone();
    let message = Message::Elapsed(Timer::Toast(Revision::default()));

    let result = update(&mut model, message, Moment::default());

    assert_eq!(result, Err(Unhandled));
    assert_eq!(model, before);
}

#[test]
fn favoriting_twice_raises_two_cues_where_the_diff_saw_none() {
    let mut model = model_with_tracks(3);
    let seen = cues(
        &mut model,
        vec![
            Message::Browse(BrowseRequest::ToggleFavorite),
            Message::Browse(BrowseRequest::ToggleFavorite),
        ],
    );
    assert_eq!(
        seen.iter()
            .filter(|cue| **cue == Cue::FavoriteToggled)
            .count(),
        2,
        "both toggles must raise a cue, saw {seen:?}"
    );
}
