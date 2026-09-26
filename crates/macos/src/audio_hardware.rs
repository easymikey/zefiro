use std::{ffi::c_void, mem::MaybeUninit, ptr, ptr::NonNull};

use crossbeam_channel::Sender;
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
#[error("CoreAudio refused the property (status {status})")]
pub(crate) struct HardwareFault {
    status: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("CoreAudio refused a property listener (status {status})")]
pub(crate) struct WatchFailure {
    status: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Muted {
    Yes,
    No,
}

#[derive(Debug)]
pub(crate) struct HardwareWatch {
    bell: *mut Sender<()>,
    device: AudioObjectID,
}

impl HardwareWatch {
    pub(crate) fn new(bell: Sender<()>) -> Result<Self, WatchFailure> {
        let bell = Box::into_raw(Box::new(bell));
        let device = default_output_device().unwrap_or(system_object());
        match add_listeners(bell.cast(), device) {
            Ok(()) => Ok(Self { bell, device }),
            Err(failure) => {
                remove_listeners(bell.cast(), device);
                // SAFETY: every listener that saw `bell` was just removed.
                drop(unsafe { Box::from_raw(bell) });
                Err(failure)
            }
        }
    }

    pub(crate) fn tracked_device(&self) -> AudioObjectID {
        self.device
    }

    pub(crate) fn rebind_to(
        &mut self,
        device: AudioObjectID,
    ) -> Result<(), WatchFailure> {
        remove_listener(self.device, &volume_address(), self.bell.cast());
        remove_listener(self.device, &mute_address(), self.bell.cast());
        match add_listener(device, &volume_address(), self.bell.cast())
            .and_then(|()| add_listener(device, &mute_address(), self.bell.cast()))
        {
            Ok(()) => {
                self.device = device;
                Ok(())
            }
            Err(failure) => Err(failure),
        }
    }
}

fn add_listeners(bell: *mut c_void, device: AudioObjectID) -> Result<(), WatchFailure> {
    add_listener(system_object(), &default_output_address(), bell)
        .and_then(|()| add_listener(device, &volume_address(), bell))
        .and_then(|()| add_listener(device, &mute_address(), bell))
}

fn remove_listeners(bell: *mut c_void, device: AudioObjectID) {
    remove_listener(system_object(), &default_output_address(), bell);
    remove_listener(device, &volume_address(), bell);
    remove_listener(device, &mute_address(), bell);
}

pub(crate) fn current_default_device() -> AudioObjectID {
    default_output_device().unwrap_or(system_object())
}

pub(crate) fn volume_scalar(device: AudioObjectID) -> Option<f32> {
    read_property::<f32>(device, &volume_address())
}

pub(crate) fn muted(device: AudioObjectID) -> Option<bool> {
    read_property::<u32>(device, &mute_address()).map(|value| value != 0)
}

pub(crate) fn set_volume_scalar(
    device: AudioObjectID,
    scalar: f32,
) -> Result<(), HardwareFault> {
    write_property(device, &volume_address(), scalar)
}

pub(crate) fn set_muted(
    device: AudioObjectID,
    muted: Muted,
) -> Result<(), HardwareFault> {
    let value: u32 = match muted {
        Muted::Yes => 1,
        Muted::No => 0,
    };
    write_property(device, &mute_address(), value)
}

fn write_property<Value: Copy>(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    value: Value,
) -> Result<(), HardwareFault> {
    let mut value = value;
    let Ok(size) = u32::try_from(size_of::<Value>()) else {
        return Err(HardwareFault { status: -1 });
    };
    let address = NonNull::from(address);
    let data_ptr = NonNull::from(&mut value).cast::<c_void>();
    // SAFETY: `address` and `data_ptr` are valid pointers to `size` live bytes.
    let status = unsafe {
        AudioObjectSetPropertyData(object, address, 0, ptr::null(), size, data_ptr)
    };
    if status == 0 {
        Ok(())
    } else {
        Err(HardwareFault { status })
    }
}

impl Drop for HardwareWatch {
    fn drop(&mut self) {
        remove_listeners(self.bell.cast(), self.device);
        // SAFETY: this pointer was created by `Box::into_raw` in `new` and
        // every listener that was given it has just been removed above.
        drop(unsafe { Box::from_raw(self.bell) });
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

fn default_output_device() -> Option<AudioObjectID> {
    read_property::<AudioObjectID>(system_object(), &default_output_address())
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
    // SAFETY: `address`, `size_ptr` and `data_ptr` are valid live pointers, and
    // `data_ptr` points at `size_of::<Value>()` writable bytes.
    let status = unsafe {
        AudioObjectGetPropertyData(object, address, 0, ptr::null(), size_ptr, data_ptr)
    };
    // SAFETY: a zero status means CoreAudio filled the buffer with a valid
    // `Value`.
    (status == 0).then(|| unsafe { value.assume_init() })
}

fn add_listener(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    bell: *mut c_void,
) -> Result<(), WatchFailure> {
    let address = NonNull::from(address);
    // SAFETY: `bell` stays valid until the matching `remove_listener` call.
    let status = unsafe {
        AudioObjectAddPropertyListener(object, address, Some(on_property_changed), bell)
    };
    if status == 0 {
        Ok(())
    } else {
        Err(WatchFailure { status })
    }
}

fn remove_listener(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    bell: *mut c_void,
) {
    let address = NonNull::from(address);
    // SAFETY: same object, address and callback as the matching `add_listener`
    // call.
    let _ = unsafe {
        AudioObjectRemovePropertyListener(
            object,
            address,
            Some(on_property_changed),
            bell,
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
    let bell = unsafe { &*client_data.cast::<Sender<()>>() };
    match bell.try_send(()) {
        Ok(()) | Err(_) => {}
    }
    0
}
