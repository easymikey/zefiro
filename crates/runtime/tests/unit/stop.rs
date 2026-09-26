use std::convert::Infallible;

use crossbeam_channel::{Sender, unbounded};
use kernel::{
    Message,
    Nudge,
    domain::{Driver, Model, SettingRow},
};
use runtime::{
    CoverDecoded,
    FrameDue,
    Painted,
    Reaction,
    Reload,
    Runtime,
    Shell,
    ShellEffect,
    View,
    run,
};

use crate::support::{boot_paths, panicking_hardware, stock_startup, stub_hardware};

#[derive(Debug, Clone, Copy)]
enum SaveStep {
    Adjust,
    Quit,
}

struct AdjustThenQuit;

impl Shell for AdjustThenQuit {
    type Input = SaveStep;
    type Error = Infallible;

    fn input(&mut self, event: SaveStep, _model: &Model) -> Reaction {
        match event {
            SaveStep::Adjust => Reaction::Message(Message::Adjust {
                row: SettingRow::Replaygain,
                nudge: Nudge::Up,
            }),
            SaveStep::Quit => Reaction::Message(Message::Quit),
        }
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

#[test]
fn drain_on_stop_writes_the_pending_config_save() {
    let directory = tempfile::tempdir().unwrap();
    let paths = boot_paths(directory.path());
    let config_path = paths.config.config.clone().unwrap();
    let startup = stock_startup();
    let runtime = Runtime::boot(startup, paths, stub_hardware()).unwrap();

    let (steps, input) = unbounded();
    steps.send(SaveStep::Adjust).unwrap();
    steps.send(SaveStep::Quit).unwrap();
    let mut shell = AdjustThenQuit;

    let ended = run(runtime, &mut shell, &input);

    assert!(matches!(ended, Ok(())));
    let text = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        text.contains("replaygain"),
        "drain must flush the pending replaygain save to disk"
    );
}

#[derive(Debug, Clone, Copy)]
enum LifeStep {
    Paint,
    Quit,
}

struct ObserveDeadThenQuit {
    steps: Sender<LifeStep>,
    paints: usize,
    restarts: usize,
}

impl Shell for ObserveDeadThenQuit {
    type Input = LifeStep;
    type Error = Infallible;

    fn input(&mut self, event: LifeStep, _model: &Model) -> Reaction {
        match event {
            LifeStep::Paint => Reaction::Ignored,
            LifeStep::Quit => Reaction::Message(Message::Quit),
        }
    }

    fn reloaded(&mut self, _reload: Reload) {}

    fn effect(&mut self, _effect: ShellEffect) {}

    fn cover(&mut self, _decoded: CoverDecoded) {}

    fn frame_due(&self) -> FrameDue {
        FrameDue::Settled
    }

    fn paint(&mut self, view: View<'_>) -> Result<Painted, Infallible> {
        self.paints += 1;
        self.restarts = view.model.drivers.record(Driver::Audio).restarts.count();
        let next = if self.restarts > 0 || self.paints >= 20 {
            LifeStep::Quit
        } else {
            LifeStep::Paint
        };
        let _ = self.steps.send(next);
        Ok(Painted::default())
    }
}

#[test]
fn a_driver_panic_is_supervised_through_view() {
    let directory = tempfile::tempdir().unwrap();
    let startup = stock_startup();
    let runtime =
        Runtime::boot(startup, boot_paths(directory.path()), panicking_hardware())
            .unwrap();

    let (steps, input) = unbounded();
    steps.send(LifeStep::Paint).unwrap();
    let mut shell = ObserveDeadThenQuit {
        steps,
        paints: 0,
        restarts: 0,
    };

    let ended = run(runtime, &mut shell, &input);

    assert!(matches!(ended, Ok(())));
    assert_eq!(shell.restarts, 1);
}
