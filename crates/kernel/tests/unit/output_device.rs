use kernel::{
    Cmd,
    Cue,
    Effect,
    Message,
    Model,
    Moment,
    domain::{DeviceDefault, OutputDevice},
    update::update,
};

use crate::support::{device, first_toast_expiry};

#[test]
fn a_device_that_fell_back_replaces_the_requested_name_and_says_so() {
    let mut m = Model::default();
    m.settings.output_device = Some(device("usb-dac"));

    let cmd = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::DeviceFellBack(None)),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(m.settings.output_device, None);
    assert_eq!(
        cmd,
        Cmd::Batch(vec![
            Effect::Animate(Cue::ToastRaised),
            first_toast_expiry()
        ])
    );
    let toast = m.workspace.toast.map(|toast| toast.text);
    assert_eq!(
        toast.as_deref(),
        Some("output device 'usb-dac' is gone — playing on the system default")
    );
}

#[test]
fn a_device_that_opened_as_asked_leaves_the_toast_alone() {
    let mut m = Model::default();
    m.settings.output_device = Some(device("usb-dac"));

    let cmd = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::DeviceFellBack(Some(device("usb-dac")))),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(m.settings.output_device, Some(device("usb-dac")));
    assert_eq!(cmd, Cmd::None);
    assert!(m.workspace.toast.is_none());
}

#[test]
fn devices_loaded_replaces_output_devices_and_emits_nothing() {
    let mut m = Model::default();
    assert!(m.settings.output_devices.is_empty());

    let devices = vec![
        OutputDevice {
            name: device("Speakers"),
            default: DeviceDefault::Default,
        },
        OutputDevice {
            name: device("Headphones"),
            default: DeviceDefault::Named,
        },
    ];
    let cmd = update(
        &mut m,
        Message::Audio(kernel::AudioEvent::DevicesLoaded(devices.clone())),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(m.settings.output_devices, devices);
    assert_eq!(cmd, Cmd::None);
}
