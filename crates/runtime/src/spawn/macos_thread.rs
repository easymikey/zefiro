#![cfg(target_os = "macos")]

use ::macos::{driver::MacosDriver, job::MacosJob, message::MacosMessage};
use kernel::{cmd::MacosCmd, domain::driver::DriverName};

use crate::{
    driver::DriverLoop,
    driver_thread::DriverThread,
    error::Error,
    jobs::Jobs,
    registry,
    spawn_setup::SpawnSetup,
};

fn macos_loop(setup: &SpawnSetup<'_>) -> DriverLoop<MacosDriver, MacosJob> {
    let jobs = Jobs {
        run: |job: MacosJob| {
            job.run(|path| {
                library::tags::embedded_cover(path).map_err(std::io::Error::other)
            })
        },
    };
    DriverLoop::<MacosDriver, MacosJob> {
        row: registry::row(DriverName::Macos),
        inbox: setup.inbox.clone(),
        heard: setup.macos.receiver.clone(),
        seed: Some(MacosMessage::Started),
        jobs,
    }
}

pub(crate) fn spawn(setup: &SpawnSetup<'_>) -> Result<DriverThread<MacosCmd>, Error> {
    let heard_sender = setup.macos.sender.clone();
    macos_loop(setup).spawn(move || MacosDriver::new(heard_sender))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossbeam_channel::Sender;
    use kernel::{domain::startup::Startup, message::Message};

    use crate::{
        spawn::macos_thread::{MacosMessage, macos_loop, spawn},
        spawn_setup::{MacosChannel, SpawnSetup, StartupPaths},
    };

    fn spawn_on(
        channel: &MacosChannel,
        paths: &StartupPaths,
        inbox: &Sender<Message>,
    ) -> crate::driver_thread::DriverThread<kernel::cmd::MacosCmd> {
        let (model, _cmd) = kernel::update::startup::startup(Startup::default());
        let (writers, _cells, _notified) = crate::latest::latest_channels();
        spawn(&SpawnSetup {
            audio: &model.settings.audio,
            paths,
            inbox,
            writers: &writers,
            macos: channel,
        })
        .unwrap()
    }

    #[test]
    fn the_loop_is_seeded_with_started_and_the_shared_channel_stays_empty() {
        let paths = crate::wiring::tests::stub_paths();
        let channel = MacosChannel::new();
        let (inbox, _arrivals) = crossbeam_channel::unbounded::<Message>();
        let (model, _cmd) = kernel::update::startup::startup(Startup::default());
        let (writers, _cells, _notified) = crate::latest::latest_channels();
        let driver_loop = macos_loop(&SpawnSetup {
            audio: &model.settings.audio,
            paths: &paths,
            inbox: &inbox,
            writers: &writers,
            macos: &channel,
        });

        assert!(matches!(driver_loop.seed, Some(MacosMessage::Started)));
        assert!(channel.receiver.is_empty());
    }

    #[test]
    #[ignore = "hardware: MacosDriver reads CoreAudio devices"]
    fn the_kept_sender_reaches_a_restarted_driver() {
        let paths = crate::wiring::tests::stub_paths();
        let channel = MacosChannel::new();
        let (inbox, arrivals) = crossbeam_channel::unbounded::<Message>();
        let first = spawn_on(&channel, &paths, &inbox);
        drop(first.commands);
        first.handle.join().unwrap().unwrap();

        let second = spawn_on(&channel, &paths, &inbox);
        channel.sender.send(MacosMessage::HardwareChanged).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(!second.handle.is_finished());
        drop(second.commands);
        second.handle.join().unwrap().unwrap();
        drop(arrivals);
    }
}
