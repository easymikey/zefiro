use kernel::{
    cmd::{Cmd, Effect},
    domain::{
        cue::Cue,
        device::{DeviceDefault, ListedDevice, OutputDevice},
        model::Model,
        revision::Revision,
        time::Moment,
    },
    message::{AudioEvent, Message, PlaybackRequest},
    update::machine::Unhandled,
};

use crate::support::{
    device,
    first_toast_expiry,
    model_with_dated_tracks,
    update::{send, update},
};

fn deliver(model: &mut Model, events: Vec<AudioEvent>) -> Vec<Result<Cmd, Unhandled>> {
    events
        .into_iter()
        .map(|event| {
            let before = model.clone();
            let answer = update(model, Message::Audio(event), Moment::default());
            assert!(answer.is_ok() || *model == before);
            answer
        })
        .collect()
}

fn opened_on(name: &str) -> AudioEvent {
    AudioEvent::DeviceOpened(device(name))
}

struct DeviceRow {
    requested_device: OutputDevice,
    events: Vec<AudioEvent>,
    answers: Vec<Result<Cmd, Unhandled>>,
    device: OutputDevice,
    toasts: Vec<(&'static str, Option<&'static str>)>,
}

#[rstest::rstest]
#[case::a_report_that_it_opened_as_asked_is_refused(DeviceRow {
    requested_device: OutputDevice::Named(device("usb-dac")),
    events: vec![AudioEvent::DeviceFellBack(OutputDevice::Named(device("usb-dac")))],
    answers: vec![Err(Unhandled)],
    device: OutputDevice::Named(device("usb-dac")),
    toasts: vec![],
})]
#[case::a_fallback_to_the_device_already_set_is_refused(DeviceRow {
    requested_device: OutputDevice::SystemDefault,
    events: vec![AudioEvent::DeviceFellBack(OutputDevice::SystemDefault)],
    answers: vec![Err(Unhandled)],
    device: OutputDevice::SystemDefault,
    toasts: vec![],
})]
#[case::an_open_on_another_device_names_it(DeviceRow {
    requested_device: OutputDevice::SystemDefault,
    events: vec![opened_on("Speakers"), opened_on("Headphones")],
    answers: vec![Ok(Cmd::none()), Ok(Cmd::from_iter([Effect::Animate(Cue::ToastRaised), first_toast_expiry()]))],
    device: OutputDevice::SystemDefault,
    toasts: vec![("Playing on Headphones", None)],
})]
#[case::a_reopen_on_the_same_device_is_refused(DeviceRow {
    requested_device: OutputDevice::SystemDefault,
    events: vec![opened_on("Speakers"), opened_on("Speakers")],
    answers: vec![Ok(Cmd::none()), Err(Unhandled)],
    device: OutputDevice::SystemDefault,
    toasts: vec![],
})]
#[case::a_fallback_replaces_the_requested_name_and_a_later_open_elsewhere_names_it(DeviceRow {
    requested_device: OutputDevice::Named(device("usb-dac")),
    events: vec![
        opened_on("usb-dac"),
        AudioEvent::DeviceFellBack(OutputDevice::SystemDefault),
        opened_on("Speakers"),
        opened_on("Headphones"),
    ],
    answers: vec![
        Ok(Cmd::none()),
        Ok(Cmd::from_iter([Effect::Animate(Cue::ToastRaised), first_toast_expiry()])),
        Ok(Cmd::none()),
        Ok(Cmd::from_iter([Effect::Animate(Cue::ToastRaised)])),
    ],
    device: OutputDevice::SystemDefault,
    toasts: vec![("Playing on Headphones", None),
        (
            "Output device lost",
            Some("output device 'usb-dac' is gone — playing on the system default"),
        ),
    ],
})]
fn device_reports_raise_their_toasts(#[case] row: DeviceRow) {
    let DeviceRow {
        requested_device,
        events,
        answers,
        device,
        toasts,
    } = row;
    let mut model = Model::default();
    model.settings.audio_settings.device = requested_device;

    let replies = deliver(&mut model, events);

    assert_eq!(
        (
            replies,
            &model.settings.audio_settings.device,
            model
                .workspace
                .toasts
                .iter()
                .map(|raised| (raised.title.as_str(), raised.text.as_deref()))
                .collect::<Vec<_>>()
        ),
        (answers, &device, toasts)
    );
}

#[test]
fn a_device_list_loads_once_and_an_identical_one_is_refused() {
    let mut model = Model::default();
    let devices = vec![
        ListedDevice {
            name: device("Speakers"),
            default: DeviceDefault::Yes,
        },
        ListedDevice {
            name: device("Headphones"),
            default: DeviceDefault::No,
        },
    ];

    let replies = deliver(
        &mut model,
        vec![
            AudioEvent::DevicesListed(devices.clone()),
            AudioEvent::DevicesListed(devices.clone()),
        ],
    );

    assert_eq!(
        (replies, model.settings.output_devices),
        (vec![Ok(Cmd::none()), Err(Unhandled)], devices)
    );
}

struct BufferRow {
    events: Vec<AudioEvent>,
    answers: Vec<Result<Cmd, Unhandled>>,
    buffering_revision: Option<Revision>,
}

#[rstest::rstest]
#[case::its_buffered_report_clears_the_flag(BufferRow {
    events: vec![
        AudioEvent::Buffering(Revision::default().next()),
        AudioEvent::Buffered(Revision::default().next()),
    ],
    answers: vec![Ok(Cmd::none()), Ok(Cmd::none())],
    buffering_revision: None,
})]
#[case::a_buffered_report_of_another_download_is_refused(BufferRow {
    events: vec![
        AudioEvent::Buffering(Revision::default().next()),
        AudioEvent::Buffered(Revision::default().next().next()),
    ],
    answers: vec![Ok(Cmd::none()), Err(Unhandled)],
    buffering_revision: Some(Revision::default().next()),
})]
fn a_buffering_report_sets_the_transport_flag(#[case] row: BufferRow) {
    let BufferRow {
        events,
        answers,
        buffering_revision,
    } = row;
    let mut model = Model::default();

    let replies = deliver(&mut model, events);

    assert_eq!(
        (replies, model.transport.buffering_revision),
        (answers, buffering_revision)
    );
}

#[test]
fn a_loaded_track_clears_the_buffering_flag() {
    let mut model = model_with_dated_tracks(3);
    send(&mut model, Message::Playback(PlaybackRequest::Toggle));
    send(
        &mut model,
        Message::Audio(AudioEvent::Buffering(Revision::default().next())),
    );

    send(&mut model, Message::Audio(AudioEvent::Loaded(None)));

    assert_eq!(model.transport.buffering_revision, None);
}
