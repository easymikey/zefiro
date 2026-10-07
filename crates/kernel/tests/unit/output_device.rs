use kernel::{
    cmd::{Cmd, Effect},
    domain::{
        cue::Cue,
        device::{DeviceDefault, ListedDevice, OutputDevice},
        model::Model,
        time::Moment,
    },
    message::Message,
    update::machine::Unhandled,
};

use crate::support::{device, first_toast_expiry, update::update};

#[test]
fn a_device_that_fell_back_replaces_the_requested_name_and_says_so() {
    let mut model = Model::default();
    model.settings.audio_settings.device = OutputDevice::Named(device("usb-dac"));

    let cmd = update(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::DeviceFellBack(
            OutputDevice::SystemDefault,
        )),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(
        model.settings.audio_settings.device,
        OutputDevice::SystemDefault
    );
    assert_eq!(
        cmd,
        Cmd::from_iter([Effect::Animate(Cue::ToastRaised), first_toast_expiry()])
    );
    let toast = model
        .workspace
        .toasts
        .first()
        .and_then(|toast| toast.text.clone());
    assert_eq!(
        toast.as_deref(),
        Some("output device 'usb-dac' is gone — playing on the system default")
    );
}

#[test]
fn a_device_that_opened_as_asked_leaves_the_toast_alone() {
    let mut model = Model::default();
    model.settings.audio_settings.device = OutputDevice::Named(device("usb-dac"));

    let result = update(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::DeviceFellBack(
            OutputDevice::Named(device("usb-dac")),
        )),
        Moment::default(),
    );

    assert_eq!(result, Err(Unhandled));
    assert_eq!(
        model.settings.audio_settings.device,
        OutputDevice::Named(device("usb-dac"))
    );
    assert!(model.workspace.toasts.is_empty());
}

#[test]
fn a_fallback_to_the_device_already_set_is_refused() {
    let mut model = Model::default();
    model.settings.audio_settings.device = OutputDevice::SystemDefault;
    let before = model.clone();

    let result = update(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::DeviceFellBack(
            OutputDevice::SystemDefault,
        )),
        Moment::default(),
    );

    assert_eq!(result, Err(Unhandled));
    assert_eq!(model, before);
}

#[test]
fn an_identical_device_list_is_refused() {
    let mut model = Model::default();
    let devices = vec![ListedDevice {
        name: device("Speakers"),
        default: DeviceDefault::Yes,
    }];
    model.settings.output_devices = devices.clone();
    let before = model.clone();

    let result = update(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::DevicesListed(devices)),
        Moment::default(),
    );

    assert_eq!(result, Err(Unhandled));
    assert_eq!(model, before);
}

#[test]
fn devices_loaded_replaces_output_devices_and_emits_nothing() {
    let mut model = Model::default();
    assert!(model.settings.output_devices.is_empty());

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
    let cmd = update(
        &mut model,
        Message::Audio(kernel::message::AudioEvent::DevicesListed(devices.clone())),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(model.settings.output_devices, devices);
    assert_eq!(cmd, Cmd::none());
}
