use std::time::Duration;

use kernel::{
    domain::{model::Model, overlay::OverlayName, time::Moment, toast::Toast},
    message::{Message, OverlayRequest},
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

fn titles_after(steps: Vec<Step>) -> Vec<String> {
    let mut model = Model::default();
    for (millis, message) in steps {
        let raised_at = Moment::new(Duration::from_millis(millis));
        let _cmd = update(&mut model, message, raised_at).unwrap();
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
fn the_stack_orders_pushes_dismissals_and_expiry(
    #[case] steps: Vec<Step>,
    #[case] expected: &[&str],
) {
    assert_eq!(titles_after(steps), expected);
}
