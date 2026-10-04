use std::ptr::NonNull;

use block2::RcBlock;
use kernel::message::PlaybackRequest;
use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSDictionary, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent,
    MPMediaItemArtwork,
    MPMediaItemPropertyAlbumTitle,
    MPMediaItemPropertyArtist,
    MPMediaItemPropertyArtwork,
    MPMediaItemPropertyPlaybackDuration,
    MPMediaItemPropertyTitle,
    MPNowPlayingInfoCenter,
    MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate,
    MPNowPlayingPlaybackState,
    MPRemoteCommand,
    MPRemoteCommandCenter,
    MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
    MPSeekCommandEvent,
    MPSeekCommandEventType,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Trigger {
    Press(PlaybackRequest),
    Hold(PlaybackRequest),
    Scrub,
}

pub(crate) fn shared_command_center() -> Retained<MPRemoteCommandCenter> {
    // SAFETY: no arguments; returns the process-wide singleton.
    unsafe { MPRemoteCommandCenter::sharedCommandCenter() }
}

pub(crate) fn remote_commands(
    center: &MPRemoteCommandCenter,
) -> [(Retained<MPRemoteCommand>, Trigger); 9] {
    // SAFETY: plain property getters on the live shared center.
    unsafe {
        [
            (center.playCommand(), Trigger::Press(PlaybackRequest::Play)),
            (
                center.pauseCommand(),
                Trigger::Press(PlaybackRequest::Pause),
            ),
            (
                center.togglePlayPauseCommand(),
                Trigger::Press(PlaybackRequest::Toggle),
            ),
            (center.stopCommand(), Trigger::Press(PlaybackRequest::Stop)),
            (
                center.nextTrackCommand(),
                Trigger::Press(PlaybackRequest::Next),
            ),
            (
                center.previousTrackCommand(),
                Trigger::Press(PlaybackRequest::Previous),
            ),
            (
                center.seekForwardCommand(),
                Trigger::Hold(PlaybackRequest::SeekForward),
            ),
            (
                center.seekBackwardCommand(),
                Trigger::Hold(PlaybackRequest::SeekBack),
            ),
            (
                Retained::into_super(center.changePlaybackPositionCommand()),
                Trigger::Scrub,
            ),
        ]
    }
}

pub(crate) fn enable_command(command: &MPRemoteCommand) {
    // SAFETY: `setEnabled:` on a live command accepts either flag.
    unsafe { command.setEnabled(true) }
}

pub(crate) fn add_command_target(
    command: &MPRemoteCommand,
    handler: &RcBlock<
        dyn Fn(NonNull<MPRemoteCommandEvent>) -> MPRemoteCommandHandlerStatus,
    >,
) -> Retained<AnyObject> {
    // SAFETY: the block has the declared signature, and the call copies it.
    unsafe { command.addTargetWithHandler(handler) }
}

pub(crate) fn remove_command_target(command: &MPRemoteCommand, target: &AnyObject) {
    // SAFETY: `target` is what this `command` returned when added.
    unsafe { command.removeTarget(Some(target)) };
}

pub(crate) fn borrow_command_event<T>(
    event: NonNull<MPRemoteCommandEvent>,
    scope: impl FnOnce(&MPRemoteCommandEvent) -> T,
) -> T {
    // SAFETY: MediaPlayer keeps the event alive for the duration of this call.
    scope(unsafe { event.as_ref() })
}

pub(crate) fn seek_event_phase(seek: &MPSeekCommandEvent) -> MPSeekCommandEventType {
    // SAFETY: a plain property getter on a live event.
    unsafe { seek.r#type() }
}

pub(crate) fn scrub_position_seconds(
    scrub: &MPChangePlaybackPositionCommandEvent,
) -> f64 {
    // SAFETY: a plain property getter on a live event.
    unsafe { scrub.positionTime() }
}

pub(crate) fn media_item_artwork(
    bounds: CGSize,
    handler: &RcBlock<dyn Fn(CGSize) -> NonNull<NSImage>>,
) -> Retained<MPMediaItemArtwork> {
    // SAFETY: the block returns its own live `NSImage`, never null.
    unsafe {
        MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            bounds,
            handler,
        )
    }
}

pub(crate) fn title_key() -> &'static NSString {
    // SAFETY: a framework string constant, immutable after load.
    unsafe { MPMediaItemPropertyTitle }
}

pub(crate) fn duration_key() -> &'static NSString {
    // SAFETY: a framework string constant, immutable after load.
    unsafe { MPMediaItemPropertyPlaybackDuration }
}

pub(crate) fn elapsed_key() -> &'static NSString {
    // SAFETY: a framework string constant, immutable after load.
    unsafe { MPNowPlayingInfoPropertyElapsedPlaybackTime }
}

pub(crate) fn rate_key() -> &'static NSString {
    // SAFETY: a framework string constant, immutable after load.
    unsafe { MPNowPlayingInfoPropertyPlaybackRate }
}

pub(crate) fn artist_key() -> &'static NSString {
    // SAFETY: a framework string constant, immutable after load.
    unsafe { MPMediaItemPropertyArtist }
}

pub(crate) fn album_key() -> &'static NSString {
    // SAFETY: a framework string constant, immutable after load.
    unsafe { MPMediaItemPropertyAlbumTitle }
}

pub(crate) fn artwork_key() -> &'static NSString {
    // SAFETY: a framework string constant, immutable after load.
    unsafe { MPMediaItemPropertyArtwork }
}

pub(crate) fn now_playing_info_center() -> Retained<MPNowPlayingInfoCenter> {
    // SAFETY: no arguments; returns the process-wide singleton.
    unsafe { MPNowPlayingInfoCenter::defaultCenter() }
}

pub(crate) fn publish_now_playing_info(
    center: &MPNowPlayingInfoCenter,
    dictionary: &NSDictionary<NSString, AnyObject>,
) {
    // SAFETY: each value has the class its `MP*Property*` key names.
    unsafe { center.setNowPlayingInfo(Some(dictionary)) };
}

pub(crate) fn publish_playback_state(
    center: &MPNowPlayingInfoCenter,
    state: MPNowPlayingPlaybackState,
) {
    // SAFETY: `state` is a declared `MPNowPlayingPlaybackState` value.
    unsafe { center.setPlaybackState(state) };
}
