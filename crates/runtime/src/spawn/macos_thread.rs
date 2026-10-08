#![cfg(target_os = "macos")]

use ::macos::{driver::MacosDriver, job::MacosJob, message::MacosMessage};
use kernel::{cmd::MacosCmd, domain::driver::DriverName};

use crate::{
    driver::DriverLoop,
    driver_thread::DriverThread,
    error::SpawnError,
    registry,
    spawn_setup::SpawnSetup,
};

pub(crate) fn spawn_macos(
    setup: &SpawnSetup<'_>,
) -> Result<DriverThread<MacosCmd>, SpawnError> {
    let run_job = |job: MacosJob| job.run(library::cover::cover_bytes);
    let callback_sender = setup.macos_channel.callback_sender.clone();
    DriverLoop::<MacosDriver, MacosJob> {
        row: registry::row(DriverName::Macos),
        inbox: setup.inbox.clone(),
        callback_receiver: setup.macos_channel.callback_receiver.clone(),
        message: Some(MacosMessage::Started),
        run_job,
    }
    .spawn(move || MacosDriver::new(callback_sender))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossbeam_channel::Sender;
    use kernel::{domain::startup::Startup, message::Message};

    use crate::{
        spawn::macos_thread::{MacosMessage, spawn_macos},
        spawn_setup::{MacosChannel, SpawnSetup, StartupPaths},
    };

    fn spawn_on(
        channel: &MacosChannel,
        paths: &StartupPaths,
        inbox: &Sender<Message>,
    ) -> crate::driver_thread::DriverThread<kernel::cmd::MacosCmd> {
        let (model, _cmd) = kernel::update::startup::startup(Startup::default());
        let (latest_senders, _latest_receivers, _doorbell) =
            crate::latest::latest_channels();
        spawn_macos(&SpawnSetup {
            audio_settings: &model.settings.audio_settings,
            paths,
            inbox,
            latest_senders: &latest_senders,
            macos_channel: channel,
        })
        .unwrap()
    }

    #[test]
    #[ignore = "hardware: MacosDriver reads CoreAudio devices"]
    fn the_kept_sender_reaches_a_restarted_driver() {
        let paths = crate::spawn::tests::stub_paths(std::path::Path::new(""));
        let channel = MacosChannel::new();
        let (inbox, inbox_receiver) = crossbeam_channel::unbounded::<Message>();
        let first = spawn_on(&channel, &paths, &inbox);
        drop(first.cmd_sender);
        first.handle.join().unwrap();

        let second = spawn_on(&channel, &paths, &inbox);
        channel
            .callback_sender
            .send(MacosMessage::HardwareChanged)
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(!second.handle.is_finished());
        drop(second.cmd_sender);
        second.handle.join().unwrap();
        drop(inbox_receiver);
    }
}
