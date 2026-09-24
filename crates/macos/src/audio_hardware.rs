use std::{ffi::c_void, mem::MaybeUninit, ptr, ptr::NonNull};

use crossbeam_channel::Sender;
use objc2_core_audio::{
    AudioObjectAddPropertyListener,
    AudioObjectGetPropertyData,
    AudioObjectID,
    AudioObjectPropertyAddress,
    AudioObjectRemovePropertyListener,
    kAudioDevicePropertyMute,
    kAudioDevicePropertyVolumeScalar,
    kAudioHardwarePropertyDefaultOutputDevice,
    kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeOutput,
    kAudioObjectSystemObject,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HardwareSignal {
    Changed,
}

#[derive(Debug)]
pub(crate) struct HardwareWatch {
    sender: *mut Sender<HardwareSignal>,
    device: AudioObjectID,
}

impl HardwareWatch {
    pub(crate) fn new(sender: Sender<HardwareSignal>) -> Self {
        let sender = Box::into_raw(Box::new(sender));
        add_listener(system_object(), &default_output_address(), sender.cast());
        let device = default_output_device().unwrap_or(system_object());
        add_listener(device, &volume_address(), sender.cast());
        add_listener(device, &mute_address(), sender.cast());
        Self { sender, device }
    }

    pub(crate) fn tracked_device(&self) -> AudioObjectID {
        self.device
    }

    pub(crate) fn rebind_to(&mut self, device: AudioObjectID) {
        remove_listener(self.device, &volume_address(), self.sender.cast());
        remove_listener(self.device, &mute_address(), self.sender.cast());
        self.device = device;
        add_listener(self.device, &volume_address(), self.sender.cast());
        add_listener(self.device, &mute_address(), self.sender.cast());
    }
}

pub(crate) fn current_default_device() -> AudioObjectID {
    default_output_device().unwrap_or(system_object())
}

impl Drop for HardwareWatch {
    fn drop(&mut self) {
        remove_listener(
            system_object(),
            &default_output_address(),
            self.sender.cast(),
        );
        remove_listener(self.device, &volume_address(), self.sender.cast());
        remove_listener(self.device, &mute_address(), self.sender.cast());
        // SAFETY: this pointer was created by `Box::into_raw` in `new` and
        // every listener that was given it has just been removed above.
        drop(unsafe { Box::from_raw(self.sender) });
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
    sender: *mut c_void,
) {
    let address = NonNull::from(address);
    // SAFETY: `on_property_changed` matches `AudioObjectPropertyListenerProc`,
    // and `sender` stays valid until the matching `remove_listener` call
    // runs.
    let _ = unsafe {
        AudioObjectAddPropertyListener(
            object,
            address,
            Some(on_property_changed),
            sender,
        )
    };
}

fn remove_listener(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    sender: *mut c_void,
) {
    let address = NonNull::from(address);
    // SAFETY: same object, address and callback as the matching `add_listener`
    // call.
    let _ = unsafe {
        AudioObjectRemovePropertyListener(
            object,
            address,
            Some(on_property_changed),
            sender,
        )
    };
}

/// # Safety
/// `client_data` must be the `Sender<HardwareSignal>` pointer given to
/// `add_listener`.
unsafe extern "C-unwind" fn on_property_changed(
    _object: AudioObjectID,
    _count: u32,
    _addresses: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    // SAFETY: `client_data` is the `Sender<HardwareSignal>` leaked by
    // `HardwareWatch::new`, kept alive until `HardwareWatch::drop` reclaims it.
    let sender = unsafe { &*client_data.cast::<Sender<HardwareSignal>>() };
    let _ = sender.send(HardwareSignal::Changed);
    0
}
