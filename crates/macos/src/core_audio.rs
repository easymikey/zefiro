use std::{
    ffi::c_void,
    mem,
    mem::MaybeUninit,
    ptr,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use crossbeam_channel::{Sender, TrySendError};
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

use crate::message::MacosMessage;

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
struct Listener {
    callback_sender: Sender<MacosMessage>,
    missed: AtomicBool,
}

#[derive(Debug)]
pub(crate) struct HardwareListeners {
    listener: *mut Listener,
    device_id: AudioObjectID,
    stale: Vec<AudioObjectID>,
}

impl HardwareListeners {
    pub(crate) fn new(callback_sender: Sender<MacosMessage>) -> Result<Self, Error> {
        let listener = Box::into_raw(Box::new(Listener {
            callback_sender,
            missed: AtomicBool::new(false),
        }));
        let device = default_output_device();
        match add_listeners(listener.cast(), device) {
            Ok(()) => Ok(Self {
                listener,
                device_id: device,
                stale: Vec::new(),
            }),
            Err(error) => {
                // SAFETY: `add_listeners` removed every listener it had added.
                drop(unsafe { Box::from_raw(listener) });
                Err(error)
            }
        }
    }

    pub(crate) fn resend(&self) {
        // SAFETY: `listener` stays boxed until `drop`.
        resend(unsafe { &*self.listener });
    }

    pub(crate) fn tracked_device(&self) -> AudioObjectID {
        self.device_id
    }

    pub(crate) fn rebind_to(&mut self, device_id: AudioObjectID) -> Result<(), Error> {
        let listener = self.listener.cast::<c_void>();
        add_device_listeners(device_id, listener)?;
        let previous = mem::replace(&mut self.device_id, device_id);
        remove_device_listeners(previous, listener)
            .inspect_err(|_| self.stale.push(previous))
    }
}

fn add_listeners(listener: *mut c_void, device_id: AudioObjectID) -> Result<(), Error> {
    add_listener(system_object(), &default_output_address(), listener)?;
    add_device_listeners(device_id, listener).or_else(|error| {
        remove_listener(system_object(), &default_output_address(), listener)
            .and(Err(error))
    })
}

fn add_device_listeners(
    device_id: AudioObjectID,
    listener: *mut c_void,
) -> Result<(), Error> {
    add_listener(device_id, &volume_address(), listener)?;
    add_listener(device_id, &mute_address(), listener).or_else(|error| {
        remove_listener(device_id, &volume_address(), listener).and(Err(error))
    })
}

fn remove_device_listeners(
    device_id: AudioObjectID,
    listener: *mut c_void,
) -> Result<(), Error> {
    let volume = remove_listener(device_id, &volume_address(), listener);
    let mute = remove_listener(device_id, &mute_address(), listener);
    volume.and(mute)
}

pub(crate) fn read_volume(device_id: AudioObjectID) -> Option<Percent> {
    let scalar = read_property::<f32>(device_id, &volume_address())?;
    Some(match read_mute(device_id) {
        Some(Muted::Yes) => Percent::clamped(0),
        Some(Muted::No) | None => Percent::from_ratio(scalar),
    })
}

fn read_mute(device_id: AudioObjectID) -> Option<Muted> {
    read_property::<u32>(device_id, &mute_address()).map(|flag| match flag {
        0 => Muted::No,
        _ => Muted::Yes,
    })
}

pub(crate) fn write_volume(
    device_id: AudioObjectID,
    volume: Percent,
) -> Result<(), Error> {
    write_property(device_id, &volume_address(), volume.ratio())?;
    read_mute(device_id)
        .and_then(|muted| mute_change(volume, muted))
        .map_or(Ok(()), |flag| {
            write_property(device_id, &mute_address(), flag)
        })
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
    checked(status)
}

fn checked(code: i32) -> Result<(), Error> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error {
            status: OsStatus(code),
        })
    }
}

impl Drop for HardwareListeners {
    fn drop(&mut self) {
        let listener = self.listener.cast::<c_void>();
        let system =
            remove_listener(system_object(), &default_output_address(), listener);
        let device = remove_device_listeners(self.device_id, listener);
        if system.and(device).is_ok() && self.stale.is_empty() {
            // SAFETY: from `Box::into_raw` in `new`; every listener is removed.
            drop(unsafe { Box::from_raw(self.listener) });
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
    listener: *mut c_void,
) -> Result<(), Error> {
    let address = NonNull::from(address);
    // SAFETY: `listener` stays valid until the matching `remove_listener` call.
    let status = unsafe {
        AudioObjectAddPropertyListener(
            object,
            address,
            Some(on_property_changed),
            listener,
        )
    };
    checked(status)
}

fn remove_listener(
    object: AudioObjectID,
    address: &AudioObjectPropertyAddress,
    listener: *mut c_void,
) -> Result<(), Error> {
    let address = NonNull::from(address);
    // SAFETY: same object, address and callback as the matching `add_listener`.
    let status = unsafe {
        AudioObjectRemovePropertyListener(
            object,
            address,
            Some(on_property_changed),
            listener,
        )
    };
    checked(status)
}

extern "C-unwind" fn on_property_changed(
    _object: AudioObjectID,
    _count: u32,
    _addresses: NonNull<AudioObjectPropertyAddress>,
    client_data: *mut c_void,
) -> i32 {
    // SAFETY: `client_data` is the sender and latch boxed by `new`.
    hear(unsafe { &*client_data.cast::<Listener>() });
    0
}

fn hear(listener: &Listener) {
    let Listener {
        callback_sender,
        missed,
    } = listener;
    match callback_sender.try_send(MacosMessage::HardwareChanged) {
        Ok(()) | Err(TrySendError::Disconnected(_)) => {}
        Err(TrySendError::Full(_)) => missed.store(true, Ordering::Release),
    }
}

fn resend(listener: &Listener) {
    if listener.missed.swap(false, Ordering::AcqRel) {
        hear(listener);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use kernel::domain::{bounded::Bounded, percent::Percent};
    use rstest::rstest;

    use crate::{
        core_audio::{
            Listener,
            Muted,
            default_output_device,
            hear,
            mute_change,
            read_volume,
            resend,
            write_volume,
        },
        message::MacosMessage,
    };

    #[test]
    fn a_change_heard_while_the_channel_is_full_is_sent_again() {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let listener = Listener {
            callback_sender: sender,
            missed: AtomicBool::new(false),
        };
        listener
            .callback_sender
            .send(MacosMessage::Listened)
            .unwrap();
        hear(&listener);
        assert!(matches!(receiver.try_recv(), Ok(MacosMessage::Listened)));
        assert!(receiver.is_empty());
        resend(&listener);
        assert!(matches!(
            receiver.try_recv(),
            Ok(MacosMessage::HardwareChanged)
        ));
        resend(&listener);
        assert!(receiver.is_empty());
    }

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
