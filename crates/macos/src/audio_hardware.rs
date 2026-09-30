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
pub(crate) struct HardwareError {
    status: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("CoreAudio refused a property listener (status {status})")]
pub(crate) struct WatchError {
    status: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Muted {
    Yes,
    No,
}

#[derive(Debug)]
pub(crate) struct HardwareWatch {
    notify: *mut Sender<()>,
    device: AudioObjectID,
}

impl HardwareWatch {
    pub(crate) fn new(notify: Sender<()>) -> Result<Self, WatchError> {
        let notify = Box::into_raw(Box::new(notify));
        let device = default_output_device();
        match add_listeners(notify.cast(), device) {
            Ok(()) => Ok(Self { notify, device }),
            Err(failure) => {
                remove_listeners(notify.cast(), device);
                // SAFETY: every listener that saw `notify` was just removed.
                drop(unsafe { Box::from_raw(notify) });
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
    ) -> Result<(), WatchError> {
        remove_listener(self.device, &volume_address(), self.notify.cast());
        remove_listener(self.device, &mute_address(), self.notify.cast());
        match add_listener(device, &volume_address(), self.notify.cast())
            .and_then(|()| add_listener(device, &mute_address(), self.notify.cast()))
        {
            Ok(()) => {
                self.device = device;
                Ok(())
            }
            Err(failure) => Err(failure),
        }
    }
}

fn add_listeners(notify: *mut c_void, device: AudioObjectID) -> Result<(), WatchError> {
    add_listener(system_object(), &default_output_address(), notify)
        .and_then(|()| add_listener(device, &volume_address(), notify))
        .and_then(|()| add_listener(device, &mute_address(), notify))
}

fn remove_listeners(notify: *mut c_void, device: AudioObjectID) {
    remove_listener(system_object(), &default_output_address(), notify);
    remove_listener(device, &volume_address(), notify);
    remove_listener(device, &mute_address(), notify);
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
) -> Result<(), HardwareError> {
    write_property(device, &volume_address(), scalar)
}

pub(crate) fn set_muted(
    device: AudioObjectID,
    muted: Muted,
) -> Result<(), HardwareError> {
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
) -> Result<(), HardwareError> {
    let mut value = value;
    let Ok(size) = u32::try_from(size_of::<Value>()) else {
        return Err(HardwareError { status: -1 });
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
        Err(HardwareError { status })
    }
}

impl Drop for HardwareWatch {
    fn drop(&mut self) {
        remove_listeners(self.notify.cast(), self.device);
        // SAFETY: this pointer was created by `Box::into_raw` in `new` and
        // every listener that was given it has just been removed above.
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
    notify: *mut c_void,
) -> Result<(), WatchError> {
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
        Err(WatchError { status })
    }
}

fn remove_listener(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    notify: *mut c_void,
) {
    let address = NonNull::from(address);
    // SAFETY: same object, address and callback as the matching `add_listener`
    // call.
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
