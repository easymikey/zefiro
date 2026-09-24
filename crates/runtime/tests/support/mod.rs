use std::{convert::Infallible, path::Path};

use audio::SpectrumTap;
use crossbeam_channel::{Receiver, Sender, unbounded};
use kernel::{
    AudioCmd,
    Message,
    domain::{Model, Startup},
};
use library::LibraryPaths;
use runtime::{
    BootPaths,
    ConfigPaths,
    CoverDecoded,
    DriverLoop,
    FrameDue,
    Hardware,
    NoDriver,
    Painted,
    Reaction,
    Reload,
    Shell,
    ShellEffect,
    View,
};

pub(crate) fn stock_startup() -> Startup {
    Startup::default()
}

pub(crate) fn boot_paths(directory: &Path) -> BootPaths {
    BootPaths {
        config: ConfigPaths {
            config: Some(directory.join("config.toml")),
            appearance: directory.join("sifr-ui.toml"),
            themes: directory.join("themes"),
            theme: None,
        },
        library: LibraryPaths {
            cache: directory.join("cache"),
            data: directory.join("data"),
            playlists: directory.join("playlists"),
        },
    }
}

pub(crate) struct QuitShell;

impl Shell for QuitShell {
    type Input = ();
    type Error = Infallible;

    fn input(&mut self, (): (), _model: &Model) -> Reaction {
        Reaction::Message(Message::Quit)
    }

    fn reloaded(&mut self, _reload: Reload) {}

    fn effect(&mut self, _effect: ShellEffect) {}

    fn cover(&mut self, _decoded: CoverDecoded) {}

    fn frame_due(&self) -> FrameDue {
        FrameDue::Settled
    }

    fn paint(&mut self, _view: View<'_>) -> Result<Painted, Infallible> {
        Ok(Painted::default())
    }
}

pub(crate) fn stub_hardware() -> Hardware<NoDriver, NoDriver> {
    Hardware::new(NoDriver, SpectrumTap::silent(), NoDriver)
}

pub(crate) struct RecordingAudio {
    forward: Sender<AudioCmd>,
}

impl DriverLoop<AudioCmd> for RecordingAudio {
    fn run(self, inbox: &Receiver<AudioCmd>, _mailbox: &Sender<Message>) {
        while let Ok(command) = inbox.recv() {
            if self.forward.send(command).is_err() {
                return;
            }
        }
    }
}

pub(crate) fn recording_hardware()
-> (Hardware<RecordingAudio, NoDriver>, Receiver<AudioCmd>) {
    let (forward, commands) = unbounded();
    let hardware =
        Hardware::new(RecordingAudio { forward }, SpectrumTap::silent(), NoDriver);
    (hardware, commands)
}

pub(crate) struct PanickingAudio;

pub(crate) fn panicking_hardware() -> Hardware<PanickingAudio, NoDriver> {
    Hardware::new(PanickingAudio, SpectrumTap::silent(), NoDriver)
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::{Receiver, Sender};
    use kernel::{AudioCmd, Message};
    use runtime::DriverLoop;

    use crate::support::PanickingAudio;

    impl DriverLoop<AudioCmd> for PanickingAudio {
        fn run(self, _inbox: &Receiver<AudioCmd>, _mailbox: &Sender<Message>) {
            panic!("boom");
        }
    }
}
