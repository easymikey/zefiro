#![cfg(target_os = "macos")]

use ::macos::{MacosDriver, MacosEffect, MacosJob, MacosMessage};
use crossbeam_channel::{Receiver, Sender, bounded};
use kernel::{MacosCmd, domain::DriverName};

use crate::{
    driver::{DriverLoop, DriverThread, LoopEffect},
    error::Error,
    jobs::Jobs,
    registry,
    spawn::SpawnSetup,
};

const HEARD: usize = 64;

#[derive(Debug, Clone)]
pub(crate) struct MacosChannel {
    pub(crate) sender: Sender<MacosMessage>,
    pub(crate) receiver: Receiver<MacosMessage>,
}

impl MacosChannel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = bounded(HEARD);
        Self { sender, receiver }
    }
}

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
    if let Err(unheard) = heard_sender.try_send(MacosMessage::Started) {
        drop(unheard.into_inner());
    }
    let jobs = Jobs {
        split: macos_split,
        run: |job: MacosJob| job.run(library::embedded_cover),
    };
    DriverLoop::<MacosDriver, MacosJob> {
        row: registry::row(DriverName::Macos),
        inbox: setup.inbox.clone(),
        heard: setup.macos.receiver.clone(),
        seed: None,
        jobs,
    }
    .spawn(move || MacosDriver::new(heard_sender))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossbeam_channel::Sender;
    use kernel::{Message, domain::Startup};

    use crate::{
        macos::{MacosChannel, MacosMessage, spawn},
        runtime::StartupPaths,
        spawn::SpawnSetup,
    };

    fn spawn_on(
        channel: &MacosChannel,
        paths: &StartupPaths,
        inbox: &Sender<Message>,
    ) -> crate::driver::DriverThread<kernel::MacosCmd> {
        let (model, _cmd) = kernel::startup(Startup::default());
        let (writers, _cells, _notified) = crate::latest::latest_channels();
        spawn(&SpawnSetup {
            audio: &model.settings.audio,
            theme: &model.themes.selected,
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
