use kernel::{
    Cmd,
    Cue,
    Effect,
    Message,
    Model,
    Moment,
    domain::{DeviceDefault, ListedDevice, OutputDevice},
};

use crate::support::{device, first_toast_expiry, step::update};

#[test]
fn a_device_that_fell_back_replaces_the_requested_name_and_says_so() {
    let mut m = Model::default();
    m.settings.audio.device = OutputDevice::Named(device("usb-dac"));

    let cmd = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::DeviceFellBack(
            OutputDevice::SystemDefault,
        )),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(m.settings.audio.device, OutputDevice::SystemDefault);
    assert_eq!(
        cmd,
        Cmd::from_iter([Effect::Animate(Cue::ToastRaised), first_toast_expiry()])
    );
    let toast = m
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
    let mut m = Model::default();
    m.settings.audio.device = OutputDevice::Named(device("usb-dac"));

    let cmd = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::DeviceFellBack(OutputDevice::Named(
            device("usb-dac"),
        ))),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(
        m.settings.audio.device,
        OutputDevice::Named(device("usb-dac"))
    );
    assert_eq!(cmd, Cmd::none());
    assert!(m.workspace.toasts.is_empty());
}

#[test]
fn devices_loaded_replaces_output_devices_and_emits_nothing() {
    let mut m = Model::default();
    assert!(m.settings.output_devices.is_empty());

    let devices = vec![
        ListedDevice {
            name: device("Speakers"),
            default: DeviceDefault::Default,
        },
        ListedDevice {
            name: device("Headphones"),
            default: DeviceDefault::Named,
        },
    ];
    let cmd = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::DevicesListed(devices.clone())),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(m.settings.output_devices, devices);
    assert_eq!(cmd, Cmd::none());
}
