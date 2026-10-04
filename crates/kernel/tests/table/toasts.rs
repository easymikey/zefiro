use std::time::Duration;

use kernel::{
    domain::{
        model::Model,
        overlay::OverlayName,
        revision::Revision,
        time::Moment,
        toast::Toast,
    },
    message::{Message, OverlayRequest, Timer},
    update::update,
};
use rstest::rstest;

type Step = (u64, Message);

fn raised(millis: u64, title: &str) -> Step {
    (millis, Message::Toast(Toast::info(title)))
}

fn key(millis: u64) -> Step {
    (
        millis,
        Message::Overlay(OverlayRequest::Open(OverlayName::Help)),
    )
}

fn first_timer(millis: u64) -> Step {
    (
        millis,
        Message::Elapsed(Timer::Toast(Revision::default().next())),
    )
}

fn titles_after(steps: Vec<Step>) -> Vec<String> {
    let mut model = Model::default();
    for (millis, message) in steps {
        let at = Moment::new(Duration::from_millis(millis));
        let _cmd = update(&mut model, message, at).unwrap();
    }
    model
        .workspace
        .toasts
        .iter()
        .map(|toast| toast.title.clone())
        .collect()
}

#[rstest]
#[case::the_newest_toast_comes_first(
    vec![raised(0, "a"), raised(1, "b")],
    &["b", "a"]
)]
#[case::the_stack_keeps_the_three_newest(
    vec![raised(0, "a"), raised(1, "b"), raised(2, "c"), raised(3, "d")],
    &["d", "c", "b"]
)]
#[case::a_key_dismisses_the_newest_only(
    vec![raised(0, "a"), raised(1, "b"), key(2)],
    &["a"]
)]
#[case::two_keys_dismiss_two_toasts(
    vec![raised(0, "a"), raised(1, "b"), key(2), key(3)],
    &[]
)]
#[case::a_toast_raised_after_the_key_stays(
    vec![raised(0, "a"), key(1), raised(2, "b")],
    &["b"]
)]
#[case::the_timer_drops_only_the_toasts_past_their_lifetime(
    vec![raised(0, "a"), raised(3000, "b"), first_timer(5000)],
    &["b"]
)]
#[case::the_timer_drops_every_expired_toast(
    vec![raised(0, "a"), raised(3000, "b"), first_timer(8000)],
    &[]
)]
#[case::an_early_timer_keeps_the_toast(
    vec![raised(0, "a"), first_timer(1000)],
    &["a"]
)]
#[case::an_event_does_not_dismiss(
    vec![raised(0, "a"), raised(1, "b"), first_timer(2)],
    &["b", "a"]
)]
fn the_stack_orders_pushes_dismissals_and_expiry(
    #[case] steps: Vec<Step>,
    #[case] expected: &[&str],
) {
    assert_eq!(titles_after(steps), expected);
}
