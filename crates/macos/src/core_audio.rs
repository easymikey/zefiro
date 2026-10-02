use std::{ffi::c_void, mem::MaybeUninit, ptr, ptr::NonNull};

use crossbeam_channel::Sender;
use kernel::{Bounded, Percent};
use objc2_core_audio::{
    AudioObjectAddPropertyListener,
    AudioObjectGetPropertyData,
    AudioObjectID,
    AudioObjectPropertyAddress,
    AudioObjectRemovePropertyListener,
    AudioObjectSetPropertyData,
    kAudioDevicePropertyMute,
    kAudioDevicePropertyVolumeScalar,
    kAudioHardwarePropertyDefaultOutputDevice,
    kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput,
    kAudioObjectSystemObject,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("CoreAudio refused the request (status {status})")]
pub(crate) struct CoreAudioError {
    status: i32,
}

#[derive(Debug)]
pub(crate) struct HardwareWatch {
    notify: *mut Sender<()>,
    device: AudioObjectID,
}

impl HardwareWatch {
    pub(crate) fn new(notify: Sender<()>) -> Result<Self, CoreAudioError> {
        let notify = Box::into_raw(Box::new(notify));
        let device = default_output_device();
        match add_listeners(notify.cast(), device) {
            Ok(()) => Ok(Self { notify, device }),
            Err(error) => {
                remove_listeners(notify.cast(), device);
                // SAFETY: every listener that saw `notify` was just removed.
                drop(unsafe { Box::from_raw(notify) });
                Err(error)
            }
        }
    }

    pub(crate) fn tracked_device(&self) -> AudioObjectID {
        self.device
    }

    pub(crate) fn rebind_to(
        &mut self,
        device: AudioObjectID,
    ) -> Result<(), CoreAudioError> {
        let notify = self.notify.cast::<c_void>();
        remove_listener(self.device, &volume_address(), notify);
        remove_listener(self.device, &mute_address(), notify);
        add_listener(device, &volume_address(), notify)?;
        add_listener(device, &mute_address(), notify)
            .inspect_err(|_| remove_listener(device, &volume_address(), notify))
            .map(|()| self.device = device)
    }
}

fn add_listeners(
    notify: *mut c_void,
    device: AudioObjectID,
) -> Result<(), CoreAudioError> {
    add_listener(system_object(), &default_output_address(), notify)
        .and_then(|()| add_listener(device, &volume_address(), notify))
        .and_then(|()| add_listener(device, &mute_address(), notify))
}

fn remove_listeners(notify: *mut c_void, device: AudioObjectID) {
    remove_listener(system_object(), &default_output_address(), notify);
    remove_listener(device, &volume_address(), notify);
    remove_listener(device, &mute_address(), notify);
}

pub(crate) fn read_volume(device: AudioObjectID) -> Option<Percent> {
    let scalar = read_property::<f32>(device, &volume_address())?;
    let muted = read_property::<u32>(device, &mute_address()).is_some_and(|m| m != 0);
    Some(if muted {
        Percent::clamped(0)
    } else {
        percent_from_scalar(scalar)
    })
}

pub(crate) fn write_volume(
    device: AudioObjectID,
    volume: Percent,
) -> Result<(), CoreAudioError> {
    write_property(device, &volume_address(), volume.ratio())?;
    let audible = volume.get() > 0;
    let muted = read_property::<u32>(device, &mute_address()).map(|m| m != 0);
    if muted == Some(audible) {
        let cleared = write_property(device, &mute_address(), u32::from(!audible));
        if audible {
            cleared?;
        }
    }
    Ok(())
}

fn percent_from_scalar(scalar: f32) -> Percent {
    let scalar = if scalar.is_nan() { 0.0 } else { scalar };
    let scaled = scalar.clamp(0.0, 1.0) * 100.0;
    let step = (0..=100u8)
        .find(|step| f32::from(*step) + 0.5 > scaled)
        .unwrap_or(100);
    Percent::clamped(step)
}

fn write_property<Value: Copy>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    mut property_value: Value,
) -> Result<(), CoreAudioError> {
    let Ok(size) = u32::try_from(size_of::<Value>()) else {
        return Err(CoreAudioError { status: -1 });
    };
    let address = NonNull::from(address);
    let data_ptr = NonNull::from(&mut property_value).cast::<c_void>();
    // SAFETY: `address` and `data_ptr` are valid pointers to `size` live bytes.
    let status = unsafe {
        AudioObjectSetPropertyData(object, address, 0, ptr::null(), size, data_ptr)
    };
    if status == 0 {
        Ok(())
    } else {
        Err(CoreAudioError { status })
    }
}

impl Drop for HardwareWatch {
    fn drop(&mut self) {
        remove_listeners(self.notify.cast(), self.device);
        // SAFETY: from `Box::into_raw` in `new`; listeners are removed.
        drop(unsafe { Box::from_raw(self.notify) });
    }
}

fn system_object() -> AudioObjectID {
    kAudioObjectSystemObject as AudioObjectID
}

fn default_output_address() -> AudioObjectPropertyAddress {
    property_address(
        kAudioHardwarePropertyDefaultOutputDevice,
        kAudioObjectPropertyScopeGlobal,
    )
}

fn volume_address() -> AudioObjectPropertyAddress {
    property_address(
        kAudioDevicePropertyVolumeScalar,
        kAudioObjectPropertyScopeOutput,
    )
}

fn mute_address() -> AudioObjectPropertyAddress {
    property_address(kAudioDevicePropertyMute, kAudioObjectPropertyScopeOutput)
}

fn property_address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain,
    }
}

pub(crate) fn default_output_device() -> AudioObjectID {
    read_property::<AudioObjectID>(system_object(), &default_output_address())
        .unwrap_or(system_object())
}

fn read_property<Value: Copy>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
) -> Option<Value> {
    let mut value = MaybeUninit::<Value>::uninit();
    let mut size = u32::try_from(size_of::<Value>()).ok()?;
    let address = NonNull::from(address);
    let size_ptr = NonNull::from(&mut size);
    let data_ptr = NonNull::new(value.as_mut_ptr().cast::<c_void>())?;
    // SAFETY: all pointers are live and `data_ptr` has room for one `Value`.
    let status = unsafe {
        AudioObjectGetPropertyData(object, address, 0, ptr::null(), size_ptr, data_ptr)
    };
    // SAFETY: status 0 means CoreAudio wrote a valid `Value`.
    (status == 0).then(|| unsafe { value.assume_init() })
}

fn add_listener(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    notify: *mut c_void,
) -> Result<(), CoreAudioError> {
    let address = NonNull::from(address);
    // SAFETY: `notify` stays valid until the matching `remove_listener` call.
    let status = unsafe {
        AudioObjectAddPropertyListener(
            object,
            address,
            Some(on_property_changed),
            notify,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(CoreAudioError { status })
    }
}

fn remove_listener(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    notify: *mut c_void,
) {
    let address = NonNull::from(address);
    // SAFETY: same object, address and callback as the matching `add_listener`.
    let _ = unsafe {
        AudioObjectRemovePropertyListener(
            object,
            address,
            Some(on_property_changed),
            notify,
        )
    };
}

extern "C-unwind" fn on_property_changed(
    _object: AudioObjectID,
    _count: u32,
    _addresses: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    // SAFETY: `client_data` is the `Sender<()>` boxed by `new`.
    let notify = unsafe { &*client_data.cast::<Sender<()>>() };
    match notify.try_send(()) {
        Ok(()) | Err(_) => {}
    }
    0
}

#[cfg(test)]
mod tests {
    use kernel::{Bounded, Percent};
    use rstest::rstest;

    use crate::core_audio::{
        default_output_device,
        percent_from_scalar,
        read_volume,
        write_volume,
    };

    #[rstest]
    #[case::floor(0.0, 0)]
    #[case::rounds_down(0.404, 40)]
    #[case::rounds_up(0.406, 41)]
    #[case::ceiling(1.0, 100)]
    #[case::clamps_above_one(1.7, 100)]
    #[case::clamps_below_zero(-0.2, 0)]
    #[case::not_a_number_is_zero(f32::NAN, 0)]
    fn percent_from_scalar_rounds_and_clamps(#[case] scalar: f32, #[case] percent: u8) {
        assert_eq!(percent_from_scalar(scalar), Percent::clamped(percent));
    }

    #[rstest]
    #[case::silence(0)]
    #[case::a_sliver(1)]
    #[case::two_fifths(40)]
    #[case::almost_full(99)]
    #[case::full(100)]
    fn scalar_round_trips_every_percent(#[case] percent: u8) {
        let volume = Percent::clamped(percent);
        assert_eq!(percent_from_scalar(volume.ratio()), volume);
    }

    #[test]
    #[ignore = "hardware: writes the system volume"]
    fn the_system_volume_reads_back_what_was_written() {
        let device = default_output_device();
        let original = read_volume(device);
        assert_eq!(write_volume(device, Percent::clamped(37)), Ok(()));
        let after = read_volume(device).unwrap();
        assert!(after.get().abs_diff(37) <= 1);
        if let Some(original) = original {
            assert_eq!(write_volume(device, original), Ok(()));
        }
    }
}
