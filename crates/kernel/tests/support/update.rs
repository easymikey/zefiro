use kernel::{
    cmd::Cmd,
    domain::{model::Model, time::Moment},
    message::Message,
    update::machine::Unhandled,
};

pub(crate) fn update(
    model: &mut Model,
    message: Message,
    now: Moment,
) -> Result<Cmd, Unhandled> {
    kernel::update::update(model, message, now)
        .map(|effects| effects.into_iter().collect::<Cmd>())
}

pub(crate) fn send_at(model: &mut Model, message: Message, now: Moment) {
    drop(update(model, message, now).unwrap());
}

pub(crate) fn send(model: &mut Model, message: Message) {
    send_at(model, message, Moment::default());
}
