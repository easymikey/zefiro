use kernel::{
    domain::{model::Model, time::Moment},
    update::update,
};
use proptest::prelude::{prop_assert_eq, proptest};

use crate::support::strategies::{message, reached_model};

fn without_chord(model: &Model) -> Model {
    let mut model = model.clone();
    model.workspace.chord_prefix = None;
    model
}

proptest! {
    #[test]
    fn a_rejected_message_leaves_the_model_as_it_was_but_the_chord(
        mut model in reached_model(),
        message in message(),
    ) {
        let before = without_chord(&model);
        if update(&mut model, message, Moment::default()).is_err() {
            prop_assert_eq!(without_chord(&model), before);
        }
    }
}
