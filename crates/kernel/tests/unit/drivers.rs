use kernel::{
    AudioEvent,
    Cmd,
    Cue,
    DriverMessage,
    Effect,
    EngineRejection,
    Message,
    ToastLevel,
    domain::{Driver, DriverFailure, DriverStatus},
    update::{Rejection, update},
};

use crate::support::{first_toast_expiry, playing_model};

fn died(driver: Driver) -> Message {
    Message::Driver(
        driver,
        DriverMessage::Died(DriverFailure::Panicked("index out of bounds".to_string())),
    )
}

#[test]
fn a_driver_death_is_recorded_and_told_as_an_error() {
    let mut model = playing_model(3);

    let cmd = update(&mut model, died(Driver::Audio)).unwrap();

    assert_eq!(
        cmd,
        Cmd::Batch(vec![
            Effect::Animate(Cue::ToastRaised),
            first_toast_expiry()
        ])
    );

    assert_eq!(
        model.drivers.audio,
        DriverStatus::Dead(DriverFailure::Panicked("index out of bounds".to_string()))
    );
    assert_eq!(
        model
            .workspace
            .toast
            .as_ref()
            .map(|toast| (toast.level, toast.text.as_str())),
        Some((
            ToastLevel::Error,
            "The audio driver stopped: panicked: index out of bounds"
        ))
    );
}

#[test]
fn an_engine_rejection_is_returned_without_a_toast() {
    let mut model = playing_model(3);
    let rejection = EngineRejection::WhileNotPlaying("/tmp/track1.flac".into());

    let outcome = update(
        &mut model,
        Message::Audio(AudioEvent::Rejected(rejection.clone())),
    );

    assert_eq!(outcome, Err(Rejection::Engine(rejection)));
    assert!(model.workspace.toast.is_none());
}
