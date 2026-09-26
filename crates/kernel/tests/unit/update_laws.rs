use kernel::{Moment, update::update};
use proptest::prelude::{prop_assert_eq, proptest};

use crate::support::strategies::{message, reached_model};

proptest! {
    #[test]
    fn a_rejected_message_leaves_the_model_as_it_was(
        mut model in reached_model(),
        message in message(),
    ) {
        let before = format!("{model:?}");
        if update(&mut model, message, Moment::default()).is_err() {
            prop_assert_eq!(format!("{model:?}"), before);
        }
    }
}
