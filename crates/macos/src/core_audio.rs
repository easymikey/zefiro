use std::{ffi::c_void, mem, mem::MaybeUninit, ptr, ptr::NonNull};

use crossbeam_channel::Sender;
use kernel::{
    domain::{bounded::Bounded, percent::Percent},
    message::OsStatus,
};
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

use crate::driver::MacosMessage;

const SIZE_OVERFLOW_STATUS: OsStatus = OsStatus(-1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("CoreAudio refused the request (status {status})")]
pub(crate) struct Error {
    status: OsStatus,
}

impl Error {
    pub(crate) const fn status(self) -> OsStatus {
        self.status
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Muted {
    Yes,
    No,
}

#[derive(Debug)]
pub(crate) struct HardwareWatch {
    notify: *mut Sender<MacosMessage>,
    device: AudioObjectID,
    stale: Vec<AudioObjectID>,
}

impl HardwareWatch {
    pub(crate) fn new(heard: Sender<MacosMessage>) -> Result<Self, Error> {
        let notify = Box::into_raw(Box::new(heard));
        let device = default_output_device();
        match add_listeners(notify.cast(), device) {
            Ok(()) => Ok(Self {
                notify,
                device,
                stale: Vec::new(),
            }),
            Err(error) => {
                // SAFETY: `add_listeners` removed every listener it had added.
                drop(unsafe { Box::from_raw(notify) });
                Err(error)
            }
        }
    }

    pub(crate) fn tracked_device(&self) -> AudioObjectID {
        self.device
    }

    pub(crate) fn rebind_to(&mut self, device: AudioObjectID) -> Result<(), Error> {
        let notify = self.notify.cast::<c_void>();
        add_device_listeners(device, notify)?;
        let previous = mem::replace(&mut self.device, device);
        remove_device_listeners(previous, notify)
            .inspect_err(|_| self.stale.push(previous))
    }
}

fn add_listeners(notify: *mut c_void, device: AudioObjectID) -> Result<(), Error> {
    add_listener(system_object(), &default_output_address(), notify)?;
    add_device_listeners(device, notify).or_else(|error| {
        remove_listener(system_object(), &default_output_address(), notify)
            .and(Err(error))
    })
}

fn add_device_listeners(
    device: AudioObjectID,
    notify: *mut c_void,
) -> Result<(), Error> {
    add_listener(device, &volume_address(), notify)?;
    add_listener(device, &mute_address(), notify).or_else(|error| {
        remove_listener(device, &volume_address(), notify).and(Err(error))
    })
}

fn remove_device_listeners(
    device: AudioObjectID,
    notify: *mut c_void,
) -> Result<(), Error> {
    let volume = remove_listener(device, &volume_address(), notify);
    let mute = remove_listener(device, &mute_address(), notify);
    volume.and(mute)
}

pub(crate) fn read_volume(device: AudioObjectID) -> Option<Percent> {
    let scalar = read_property::<f32>(device, &volume_address())?;
    Some(match read_mute(device) {
        Some(Muted::Yes) => Percent::clamped(0),
        Some(Muted::No) | None => Percent::from_ratio(scalar),
    })
}

fn read_mute(device: AudioObjectID) -> Option<Muted> {
    read_property::<u32>(device, &mute_address()).map(|flag| match flag {
        0 => Muted::No,
        _ => Muted::Yes,
    })
}

pub(crate) fn write_volume(
    device: AudioObjectID,
    volume: Percent,
) -> Result<(), Error> {
    write_property(device, &volume_address(), volume.ratio())?;
    read_mute(device)
        .and_then(|muted| mute_change(volume, muted))
        .map_or(Ok(()), |flag| write_property(device, &mute_address(), flag))
}

fn mute_change(volume: Percent, muted: Muted) -> Option<u32> {
    match (volume.get(), muted) {
        (0, Muted::No) => Some(1),
        (1.., Muted::Yes) => Some(0),
        (0, Muted::Yes) | (1.., Muted::No) => None,
    }
}

fn write_property<Value: Copy>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    mut property_value: Value,
) -> Result<(), Error> {
    let Ok(size) = u32::try_from(size_of::<Value>()) else {
        return Err(Error {
            status: SIZE_OVERFLOW_STATUS,
        });
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
        Err(Error {
            status: OsStatus(status),
        })
    }
}

impl Drop for HardwareWatch {
    fn drop(&mut self) {
        let notify = self.notify.cast::<c_void>();
        let system =
            remove_listener(system_object(), &default_output_address(), notify);
        let device = remove_device_listeners(self.device, notify);
        if system.and(device).is_ok() && self.stale.is_empty() {
            // SAFETY: from `Box::into_raw` in `new`; every listener is removed.
            drop(unsafe { Box::from_raw(self.notify) });
        }
    }
}

fn system_object() -> AudioObjectID {
    kAudioObjectSystemObject.unsigned_abs()
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
) -> Result<(), Error> {
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
        Err(Error {
            status: OsStatus(status),
        })
    }
}

fn remove_listener(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    notify: *mut c_void,
) -> Result<(), Error> {
    let address = NonNull::from(address);
    // SAFETY: same object, address and callback as the matching `add_listener`.
    let status = unsafe {
        AudioObjectRemovePropertyListener(
            object,
            address,
            Some(on_property_changed),
            notify,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(Error {
            status: OsStatus(status),
        })
    }
}

extern "C-unwind" fn on_property_changed(
    _object: AudioObjectID,
    _count: u32,
    _addresses: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    // SAFETY: `client_data` is the `Sender<MacosMessage>` boxed by `new`.
    let heard = unsafe { &*client_data.cast::<Sender<MacosMessage>>() };
    match heard.try_send(MacosMessage::HardwareChanged) {
        Ok(()) | Err(_) => {}
    }
    0
}

#[cfg(test)]
mod tests {
    use kernel::domain::{bounded::Bounded, percent::Percent};
    use rstest::rstest;

    use crate::core_audio::{
        Muted,
        default_output_device,
        mute_change,
        read_volume,
        write_volume,
    };

    #[rstest]
    #[case::silence_mutes_an_audible_device(0, Muted::No, Some(1))]
    #[case::silence_keeps_a_muted_device(0, Muted::Yes, None)]
    #[case::sound_unmutes_a_muted_device(40, Muted::Yes, Some(0))]
    #[case::sound_keeps_an_audible_device(40, Muted::No, None)]
    fn mute_follows_the_written_volume(
        #[case] percent: u8,
        #[case] muted: Muted,
        #[case] change: Option<u32>,
    ) {
        assert_eq!(mute_change(Percent::clamped(percent), muted), change);
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
