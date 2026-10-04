#![cfg(target_os = "macos")]

use ::macos::{
    driver::MacosDriver,
    effect::MacosEffect,
    job::MacosJob,
    message::MacosMessage,
};
use kernel::{cmd::MacosCmd, domain::driver::DriverName};

use crate::{
    driver::DriverLoop,
    driver_thread::DriverThread,
    error::Error,
    jobs::{Jobs, LoopEffect},
    registry,
    spawn_setup::SpawnSetup,
};

fn macos_split(effect: MacosEffect) -> LoopEffect<MacosEffect, MacosJob, MacosMessage> {
    match effect {
        MacosEffect::Run(job) => LoopEffect::Run(job),
        effect @ (MacosEffect::Watch
        | MacosEffect::Poll
        | MacosEffect::Rebind(_)
        | MacosEffect::SetVolume(_)
        | MacosEffect::Publish
        | MacosEffect::ClearArtwork
        | MacosEffect::ShowArtwork(_)) => LoopEffect::Execute(effect),
    }
}

pub(crate) fn spawn(setup: &SpawnSetup<'_>) -> Result<DriverThread<MacosCmd>, Error> {
    let heard_sender = setup.macos.sender.clone();
    let jobs = Jobs {
        split: macos_split,
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
    .spawn(move || MacosDriver::new(heard_sender))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossbeam_channel::Sender;
    use kernel::{domain::startup::Startup, message::Message};

    use crate::{
        macos_channel::MacosChannel,
        spawn::macos_thread::{MacosMessage, spawn},
        spawn_setup::SpawnSetup,
        startup_paths::StartupPaths,
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
