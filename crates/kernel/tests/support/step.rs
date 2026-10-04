use kernel::{Cmd, Message, Model, Moment, update::Unhandled};

pub(crate) fn update(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    kernel::update::update(model, message, now)
        .map(|effects| effects.into_iter().collect::<Cmd>())
}

pub(crate) fn apply_at(model: &mut Model, message: Message, now: Moment) {
    drop(update(model, message, now).unwrap());
}

pub(crate) fn apply(model: &mut Model, message: Message) {
    apply_at(model, message, Moment::default());
}
