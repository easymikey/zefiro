use std::{error::Error, time::Instant};

use crossbeam_channel::{Receiver, Select, never};
use kernel::{ConfigFact, Message, domain::ThemeName};

use crate::{
    error::RunError,
    library::cover::CoverDecoded,
    runtime::{Change, Runtime},
    shell::{Flow, FrameDue, Reaction, Reload, Shell},
};

pub fn run<S: Shell>(
    mut runtime: Runtime,
    shell: &mut S,
    input: &Receiver<S::Input>,
) -> Result<(), RunError<S::Error>>
where
    S::Error: Error + 'static,
{
    let ended = EventLoop::new(&mut runtime, shell, input).drive();
    runtime.drain();
    ended
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repaint {
    Needed,
    Settled,
}

enum Arrival<I> {
    Input(I),
    Message(Message),
    Reload(Reload),
    Cover(CoverDecoded),
    Nothing,
}

struct EventLoop<'a, S: Shell> {
    runtime: &'a mut Runtime,
    shell: &'a mut S,
    input: &'a Receiver<S::Input>,
    repaint: Repaint,
}

impl<'a, S: Shell> EventLoop<'a, S>
where
    S::Error: Error + 'static,
{
    fn new(
        runtime: &'a mut Runtime,
        shell: &'a mut S,
        input: &'a Receiver<S::Input>,
    ) -> Self {
        Self {
            runtime,
            shell,
            input,
            repaint: Repaint::Needed,
        }
    }

    fn drive(mut self) -> Result<(), RunError<S::Error>> {
        loop {
            let deadline = self.deadline();
            let first = self.wait(deadline)?;
            self.gather(first);
            self.fire_timers(Instant::now());
            for effect in self.runtime.take_shell_effects() {
                self.shell.effect(effect);
            }
            if let Flow::Stop = self.runtime.flow() {
                return Ok(());
            }
            self.paint_if_due()?;
        }
    }

    fn deadline(&self) -> Option<Instant> {
        let frame = match self.shell.frame_due() {
            FrameDue::At(at) => Some(at),
            FrameDue::Settled => None,
        };
        let immediate = match self.repaint {
            Repaint::Needed => Some(Instant::now()),
            Repaint::Settled => None,
        };
        [self.runtime.timers.next_deadline(), frame, immediate]
            .into_iter()
            .flatten()
            .min()
    }

    fn wait(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<Arrival<S::Input>, RunError<S::Error>> {
        let wiring = &mut self.runtime.wiring;
        let mut select = Select::new();
        let input_index = select.recv(self.input);
        let mailbox_index = select.recv(&wiring.mailbox);
        let reloads_index = select.recv(&wiring.reloads);
        select.recv(&wiring.decoded);
        let operation = match deadline {
            Some(deadline) => match select.select_deadline(deadline) {
                Ok(operation) => operation,
                Err(_) => return Ok(Arrival::Nothing),
            },
            None => select.select(),
        };
        let index = operation.index();
        if index == input_index {
            return operation
                .recv(self.input)
                .map(Arrival::Input)
                .map_err(|_| RunError::InputClosed);
        }
        if index == mailbox_index {
            return Ok(operation.recv(&wiring.mailbox).map_or_else(
                |_| {
                    wiring.mailbox = never();
                    Arrival::Nothing
                },
                Arrival::Message,
            ));
        }
        if index == reloads_index {
            return Ok(operation.recv(&wiring.reloads).map_or_else(
                |_| {
                    wiring.reloads = never();
                    Arrival::Nothing
                },
                Arrival::Reload,
            ));
        }
        Ok(operation.recv(&wiring.decoded).map_or_else(
            |_| {
                wiring.decoded = never();
                Arrival::Nothing
            },
            Arrival::Cover,
        ))
    }

    fn gather(&mut self, first: Arrival<S::Input>) {
        if let Arrival::Nothing = first {
            return;
        }
        let mailbox = self.runtime.wiring.mailbox.clone();
        let reloads = self.runtime.wiring.reloads.clone();
        let decoded = self.runtime.wiring.decoded.clone();
        let arrivals = std::iter::once(first)
            .chain(ready(self.input).map(Arrival::Input))
            .chain(ready(&mailbox).map(Arrival::Message))
            .chain(ready(&reloads).map(Arrival::Reload))
            .chain(ready(&decoded).map(Arrival::Cover));
        for arrival in arrivals {
            if let Flow::Stop = self.runtime.flow() {
                return;
            }
            self.apply(arrival);
        }
    }

    fn apply(&mut self, arrival: Arrival<S::Input>) {
        match arrival {
            Arrival::Input(event) => match self.shell.input(event, &self.runtime.model)
            {
                Reaction::Message(message) => {
                    let change = self.runtime.step(message);
                    self.note(change);
                }
                Reaction::Repaint => self.repaint = Repaint::Needed,
                Reaction::Ignored => {}
            },
            Arrival::Message(message) => {
                let change = self.runtime.step(message);
                self.note(change);
            }
            Arrival::Reload(reload) => {
                self.repaint = Repaint::Needed;
                let name = match &reload {
                    Reload::Theme(theme) => ThemeName::new(theme.name.clone()).ok(),
                    Reload::Appearance(_) => None,
                };
                self.shell.reloaded(reload);
                if let Some(name) = name {
                    let change = self
                        .runtime
                        .step(Message::Config(ConfigFact::ThemeReloaded(name)));
                    self.note(change);
                }
            }
            Arrival::Cover(decoded) => {
                self.repaint = Repaint::Needed;
                self.shell.cover(decoded);
            }
            Arrival::Nothing => {}
        }
    }

    fn note(&mut self, change: Change) {
        if let Change::Applied = change {
            self.repaint = Repaint::Needed;
        }
    }

    fn fire_timers(&mut self, now: Instant) {
        for timer in self.runtime.timers.due(now) {
            let change = self.runtime.step(Message::Elapsed(timer));
            self.note(change);
        }
    }

    fn paint_if_due(&mut self) -> Result<(), RunError<S::Error>> {
        let frame_passed = matches!(
            self.shell.frame_due(),
            FrameDue::At(at) if at <= Instant::now()
        );
        if let (Repaint::Settled, false) = (self.repaint, frame_passed) {
            return Ok(());
        }
        let painted = self
            .shell
            .paint(self.runtime.view())
            .map_err(RunError::Paint)?;
        self.repaint = Repaint::Settled;
        if let Some(request) = painted.cover {
            self.runtime.request_cover(request);
        }
        if let Some(visible_rows) = painted.viewport {
            let change = self.runtime.step(Message::Viewport { visible_rows });
            self.note(change);
        }
        Ok(())
    }
}

fn ready<T>(receiver: &Receiver<T>) -> impl Iterator<Item = T> + '_ {
    receiver.try_iter().take(receiver.len())
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, path::PathBuf, time::Instant};

    use config::{Hex, ThemeColors, ThemeFile};
    use crossbeam_channel::{Receiver, Sender, unbounded};
    use kernel::{
        Cue,
        DriverMessage,
        Message,
        Timer,
        WindowColorsCmd,
        WorkspaceRequest,
        domain::{Driver, DriverStatus, Model, Startup, Toast},
        update::{DriverRejection, Rejection},
    };
    use rstest::rstest;

    use crate::{
        error::RunError,
        event_loop::{EventLoop, Repaint},
        library::cover::{CoverDecoded, CoverRequest},
        runtime::{Change, Runtime, Wiring},
        shell::{FrameDue, Painted, Reaction, Reload, Shell, ShellEffect, View},
        trace::{Trace, TraceEntry},
    };

    #[derive(Debug, Clone, Copy)]
    enum Key {
        Quit,
        Stray,
        Ping,
        Resize,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Order {
        Reloaded,
        Effect(ShellEffect),
    }

    struct Scripted {
        keys: Sender<Key>,
        quit_after: usize,
        continue_with: Key,
        cover: Option<CoverRequest>,
        effects: Vec<ShellEffect>,
        toasts: Vec<Option<String>>,
        order: Vec<Order>,
    }

    impl Scripted {
        fn new(keys: Sender<Key>, quit_after: usize) -> Self {
            Self {
                keys,
                quit_after,
                continue_with: Key::Stray,
                cover: None,
                effects: Vec::new(),
                toasts: Vec::new(),
                order: Vec::new(),
            }
        }
    }

    impl Shell for Scripted {
        type Input = Key;
        type Error = Infallible;

        fn input(&mut self, event: Key, _model: &Model) -> Reaction {
            match event {
                Key::Quit => Reaction::Message(Message::Quit),
                Key::Stray => Reaction::Message(Message::Driver(
                    Driver::Audio,
                    DriverMessage::Stopped,
                )),
                Key::Ping => Reaction::Message(Message::Workspace(
                    WorkspaceRequest::ShowToast(Toast::error("ping".to_owned())),
                )),
                Key::Resize => Reaction::Repaint,
            }
        }

        fn reloaded(&mut self, _reload: Reload) {
            self.order.push(Order::Reloaded);
        }

        fn effect(&mut self, effect: ShellEffect) {
            self.order.push(Order::Effect(effect.clone()));
            self.effects.push(effect);
        }

        fn cover(&mut self, _decoded: CoverDecoded) {}

        fn frame_due(&self) -> FrameDue {
            FrameDue::Settled
        }

        fn paint(&mut self, view: View<'_>) -> Result<Painted, Infallible> {
            let toast = view.model.workspace.toast.as_ref();
            self.toasts.push(toast.map(|toast| toast.text.clone()));
            let next = if self.toasts.len() >= self.quit_after {
                Key::Quit
            } else {
                self.continue_with
            };
            let _ = self.keys.send(next);
            Ok(Painted {
                cover: self.cover.clone(),
                viewport: None,
            })
        }
    }

    struct Fixture {
        runtime: Runtime,
        cover_inbox: Receiver<CoverRequest>,
        _reloads: Sender<Reload>,
        _decoded: Sender<CoverDecoded>,
    }

    fn stock_startup() -> Startup {
        Startup::default()
    }

    fn fixture() -> Fixture {
        let (wiring, cover_inbox, reloads_sender, decoded_sender) = Wiring::idle();
        Fixture {
            runtime: Runtime::assemble(stock_startup(), wiring, Trace::default()),
            cover_inbox,
            _reloads: reloads_sender,
            _decoded: decoded_sender,
        }
    }

    #[rstest]
    #[case::applied_marks_repaint_needed(Change::Applied, Repaint::Needed)]
    #[case::refused_leaves_repaint_settled(Change::Refused, Repaint::Settled)]
    fn note_sets_repaint_only_when_the_model_changed(
        #[case] change: Change,
        #[case] expected: Repaint,
    ) {
        let mut fixture = fixture();
        let (keys, input) = unbounded::<Key>();
        let mut shell = Scripted::new(keys, usize::MAX);
        let mut event_loop = EventLoop::new(&mut fixture.runtime, &mut shell, &input);
        event_loop.repaint = Repaint::Settled;

        event_loop.note(change);

        assert_eq!(event_loop.repaint, expected);
        fixture.runtime.drain();
    }

    #[test]
    fn quit_before_paint_resets_the_window_and_paints_nothing() {
        let mut fixture = fixture();
        let (keys, input) = unbounded();
        keys.send(Key::Quit).unwrap();
        let mut shell = Scripted::new(keys, usize::MAX);

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert!(shell.toasts.is_empty());
        assert_eq!(
            shell.effects,
            vec![ShellEffect::WindowColors(WindowColorsCmd::Reset)]
        );
        fixture.runtime.drain();
    }

    #[test]
    fn closed_input_ends_the_loop() {
        let mut fixture = fixture();
        let (keys, input) = unbounded::<Key>();
        drop(keys);
        let mut shell = Scripted::new(unbounded().0, usize::MAX);

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Err(RunError::InputClosed)));
        fixture.runtime.drain();
    }

    #[test]
    fn a_timer_fires_elapsed_with_no_input() {
        let mut fixture = fixture();
        fixture
            .runtime
            .step(Message::Workspace(WorkspaceRequest::ShowToast(
                Toast::error("hello".to_owned()),
            )));
        let generation = fixture.runtime.model.toast_generation;
        fixture
            .runtime
            .timers
            .schedule(Instant::now(), Timer::Toast(generation));
        fixture.runtime.take_shell_effects();
        let (keys, input) = unbounded();
        let mut shell = Scripted::new(keys, 1);

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell.toasts, vec![None]);
        assert!(
            shell
                .effects
                .contains(&ShellEffect::Animate(Cue::ToastDismissed))
        );
        fixture.runtime.drain();
    }

    #[test]
    fn a_rejection_lands_in_the_trace_only() {
        let mut fixture = fixture();
        fixture
            .runtime
            .model
            .drivers
            .record_mut(Driver::Audio)
            .status = DriverStatus::Stopped;
        let (keys, input) = unbounded();
        keys.send(Key::Stray).unwrap();
        let mut shell = Scripted::new(keys, 1);

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        let rejected: Vec<&TraceEntry> = fixture
            .runtime
            .trace
            .iter()
            .filter(|entry| matches!(entry, TraceEntry::Rejected { .. }))
            .collect();
        assert_eq!(
            rejected,
            vec![&TraceEntry::Rejected {
                message: "driver",
                rejection: Rejection::Driver(Driver::Audio, DriverRejection::Stopped),
            }]
        );
        assert_eq!(shell.toasts, vec![None]);
        assert_eq!(
            shell.effects,
            vec![ShellEffect::WindowColors(WindowColorsCmd::Reset)]
        );
        assert_eq!(
            fixture.runtime.model.drivers.status(Driver::Audio),
            &DriverStatus::Stopped
        );
        fixture.runtime.drain();
    }

    #[test]
    fn a_cover_the_frame_keeps_asking_for_is_requested_once() {
        let mut fixture = fixture();
        let (keys, input) = unbounded();
        keys.send(Key::Ping).unwrap();
        let mut shell = Scripted::new(keys, 2);
        shell.continue_with = Key::Ping;
        shell.cover = Some(CoverRequest {
            path: PathBuf::from("/music/cover.mp3"),
            side: 64,
        });

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell.toasts.len(), 2);
        assert_eq!(fixture.cover_inbox.try_iter().count(), 1);
        fixture.runtime.drain();
    }

    #[test]
    fn a_repaint_request_paints_once_and_sends_no_kernel_message() {
        let mut fixture = fixture();
        let (keys, input) = unbounded();
        keys.send(Key::Resize).unwrap();
        let mut shell = Scripted::new(keys, 1);

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell.toasts, vec![None]);
        assert_eq!(fixture.runtime.trace.iter().count(), 0);
        fixture.runtime.drain();
    }

    fn stub_theme() -> ThemeFile {
        ThemeFile {
            name: "test".to_owned(),
            colors: ThemeColors {
                background: Hex([0, 0, 0]),
                foreground: Hex([255, 255, 255]),
                bright_foreground: Hex([255, 255, 255]),
                accent: Hex([0, 0, 0]),
                green: Hex([0, 0, 0]),
                yellow: Hex([0, 0, 0]),
                red: Hex([0, 0, 0]),
                window_background: None,
            },
            scanning_label: "scanning".to_owned(),
        }
    }

    #[test]
    fn a_theme_reload_bumps_the_generation_and_animates_after_reloaded() {
        let mut fixture = fixture();
        let before = fixture.runtime.model.theme_generation;
        fixture._reloads.send(Reload::Theme(stub_theme())).unwrap();
        let (keys, input) = unbounded();
        let mut shell = Scripted::new(keys, 1);

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(
            fixture.runtime.model.theme_generation.get(),
            before.next().get()
        );
        let reloaded_at = shell
            .order
            .iter()
            .position(|order| matches!(order, Order::Reloaded))
            .unwrap();
        let animated_at = shell
            .order
            .iter()
            .position(|order| {
                matches!(
                    order,
                    Order::Effect(ShellEffect::Animate(Cue::ThemeChanged))
                )
            })
            .unwrap();
        assert!(reloaded_at < animated_at);
        fixture.runtime.drain();
    }
}
