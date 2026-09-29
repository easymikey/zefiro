use std::{error::Error, time::Instant};

use crossbeam_channel::{Receiver, Select, never};
use kernel::Message;

use crate::{
    error::RunError,
    repaint::{Repaint, Source, repaint_after},
    runtime::{Change, Runtime},
    shell::{Flow, Reaction, Shell},
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

enum Arrival<I> {
    Input(I),
    Message(Message),
    Doorbell,
    Nothing,
}

pub(crate) struct EventLoop<'a, S: Shell> {
    pub(crate) runtime: &'a mut Runtime,
    pub(crate) shell: &'a mut S,
    input: &'a Receiver<S::Input>,
    pub(crate) repaint: Repaint,
    pub(crate) last_paint: Option<Instant>,
}

impl<'a, S: Shell> EventLoop<'a, S>
where
    S::Error: Error + 'static,
{
    pub(crate) fn new(
        runtime: &'a mut Runtime,
        shell: &'a mut S,
        input: &'a Receiver<S::Input>,
    ) -> Self {
        Self {
            runtime,
            shell,
            input,
            repaint: Repaint::Now,
            last_paint: None,
        }
    }

    pub(crate) fn drive(mut self) -> Result<(), RunError<S::Error>> {
        let mut frame_due = self.shell.frame_due(&self.runtime.view(Instant::now()));
        loop {
            let now = Instant::now();
            let deadline = self.deadline(now, frame_due);
            let first = self.wait(deadline)?;
            self.gather(first);
            self.fire_timers(now);
            self.runtime.settle_congestion();
            for effect in self.runtime.take_shell_effects() {
                self.shell.effect(effect);
            }
            if let Flow::Stop = self.runtime.flow() {
                return Ok(());
            }
            frame_due = self.shell.frame_due(&self.runtime.view(now));
            self.paint_if_due(now, frame_due)?;
        }
    }

    fn wait(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<Arrival<S::Input>, RunError<S::Error>> {
        let wiring = &mut self.runtime.wiring;
        let mut select = Select::new();
        let input_index = select.recv(self.input);
        let mailbox_index = select.recv(&wiring.mailbox);
        select.recv(&wiring.doorbell);
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
        Ok(operation.recv(&wiring.doorbell).map_or_else(
            |_| {
                wiring.doorbell = never();
                Arrival::Nothing
            },
            |()| Arrival::Doorbell,
        ))
    }

    fn gather(&mut self, first: Arrival<S::Input>) {
        if let Arrival::Nothing = first {
            return;
        }
        let mailbox = self.runtime.wiring.mailbox.clone();
        let doorbell = self.runtime.wiring.doorbell.clone();
        let mut inputs = Vec::new();
        let mut messages = Vec::new();
        let mut rang = false;
        match first {
            Arrival::Input(event) => inputs.push(event),
            Arrival::Message(message) => messages.push(message),
            Arrival::Doorbell => rang = true,
            Arrival::Nothing => {}
        }
        inputs.extend(ready(self.input));
        messages.extend(ready(&mailbox));
        rang = rang || ready(&doorbell).count() > 0;
        let arrivals = inputs
            .into_iter()
            .map(Arrival::Input)
            .chain(messages.into_iter().map(Arrival::Message))
            .chain(rang.then_some(Arrival::Doorbell));
        for arrival in arrivals {
            if let Flow::Stop = self.runtime.flow() {
                return;
            }
            self.apply(arrival);
        }
    }

    fn apply(&mut self, arrival: Arrival<S::Input>) {
        match arrival {
            Arrival::Input(event) => match self.shell.input(event) {
                Reaction::Message(message) => {
                    let change = self.runtime.step(message);
                    self.note(change, Source::Input);
                }
                Reaction::Repaint => {
                    self.repaint = repaint_after(self.repaint, Source::Input);
                }
                Reaction::Ignored => {}
            },
            Arrival::Message(message) => {
                let change = self.runtime.step(message);
                self.note(change, Source::Fact);
            }
            Arrival::Doorbell => {
                self.repaint = repaint_after(self.repaint, Source::Fact);
            }
            Arrival::Nothing => {}
        }
    }

    pub(crate) fn note(&mut self, change: Change, source: Source) {
        if let Change::Applied = change {
            self.repaint = repaint_after(self.repaint, source);
        }
    }

    fn fire_timers(&mut self, now: Instant) {
        for timer in self.runtime.timers.due(now) {
            let change = self.runtime.step(Message::Elapsed(timer));
            self.note(change, Source::Fact);
        }
    }
}

fn ready<T>(receiver: &Receiver<T>) -> impl Iterator<Item = T> + '_ {
    receiver.try_iter().take(receiver.len())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{convert::Infallible, path::PathBuf, time::Instant};

    use config::{Hex, ThemeColors, ThemeFile};
    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use kernel::{
        ConfigFact,
        Cue,
        Delivery,
        DriverMessage,
        LibraryFact,
        Message,
        Outbox,
        Timer,
        WindowColorsCmd,
        WorkspaceRequest,
        domain::{Driver, DriverStatus, Startup, ThemeName, Toast},
        update::{DriverRejection, Rejection},
    };

    use crate::{
        cells::Writers,
        error::RunError,
        event_loop::EventLoop,
        interpret::LibraryCommand,
        library::cover::CoverRequest,
        mailbox::{Congestion, Mailbox},
        port::{LibraryPort, Port},
        runtime::Runtime,
        shell::{Flow, FrameDue, Painted, Reaction, Shell, ShellEffect, View},
        trace::{Trace, TraceEntry},
        wiring::Wiring,
    };

    #[derive(Debug, Clone, Copy)]
    pub(crate) enum Key {
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

    pub(crate) struct Scripted {
        keys: Sender<Key>,
        quit_after: usize,
        continue_with: Key,
        cover: Option<CoverRequest>,
        effects: Vec<ShellEffect>,
        pub(crate) toasts: Vec<Option<String>>,
        order: Vec<Order>,
        pub(crate) pending_failures: Vec<Message>,
    }

    impl Scripted {
        pub(crate) fn new(keys: Sender<Key>, quit_after: usize) -> Self {
            Self {
                keys,
                quit_after,
                continue_with: Key::Stray,
                cover: None,
                effects: Vec::new(),
                toasts: Vec::new(),
                order: Vec::new(),
                pending_failures: Vec::new(),
            }
        }
    }

    impl Shell for Scripted {
        type Input = Key;
        type Error = Infallible;

        fn input(&mut self, event: Key) -> Reaction {
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

        fn effect(&mut self, effect: ShellEffect) {
            self.order.push(Order::Effect(effect.clone()));
            self.effects.push(effect);
        }

        fn frame_due(&self, _view: &View<'_>) -> FrameDue {
            FrameDue::Settled
        }

        fn paint(&mut self, view: View<'_>) -> Result<Painted, Infallible> {
            if view.cells.theme.take().is_some() {
                self.order.push(Order::Reloaded);
            }
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
                failures: std::mem::take(&mut self.pending_failures),
            })
        }
    }

    pub(crate) struct Fixture {
        pub(crate) runtime: Runtime,
        cover_inbox: Receiver<LibraryCommand>,
        _writers: Writers,
    }

    pub(crate) fn stock_startup() -> Startup {
        Startup::default()
    }

    pub(crate) fn fixture() -> Fixture {
        let (wiring, cover_inbox, writers) = Wiring::idle();
        let seed = Runtime::seeded(stock_startup());
        Fixture {
            runtime: Runtime::assemble(seed, wiring, Trace::default()),
            cover_inbox,
            _writers: writers,
        }
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
        fixture.runtime.drain();
        let covers = fixture
            .cover_inbox
            .iter()
            .filter(|command| matches!(command, LibraryCommand::Cover(_)))
            .count();
        assert_eq!(covers, 1);
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
        fixture._writers.theme.publish(stub_theme());
        let fact = ConfigFact::ThemeReloaded(ThemeName::from_static("test"));
        fixture
            .runtime
            .mailbox_sender()
            .send(Message::Config(fact))
            .unwrap();
        let (keys, input) = unbounded();
        let mut shell = Scripted::new(keys, 1);

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(
            fixture.runtime.model.theme_generation.get(),
            before.next().get()
        );
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
        let reloaded_at = shell
            .order
            .iter()
            .position(|order| matches!(order, Order::Reloaded))
            .unwrap();
        assert!(animated_at < reloaded_at);
        fixture.runtime.drain();
    }

    const CAPACITY: usize = 256;

    fn fill_the_mailbox(outbox: &Mailbox<LibraryFact>, congestion: &Congestion) {
        for _ in 0..CAPACITY {
            let delivery = outbox.send(LibraryFact::HistoryLoaded(Vec::new()));
            assert!(matches!(delivery, Delivery::Sent));
        }
        congestion.raise();
    }

    fn toasts_in_one_iteration(runtime: &mut Runtime) -> usize {
        let (_keys, input) = unbounded::<Key>();
        let mut shell = Scripted::new(unbounded().0, usize::MAX);
        let mut event_loop = EventLoop::new(runtime, &mut shell, &input);
        let first = event_loop.wait(Some(Instant::now())).unwrap();
        event_loop.gather(first);
        event_loop.runtime.settle_congestion();
        for effect in event_loop.runtime.take_shell_effects() {
            event_loop.shell.effect(effect);
        }
        shell
            .effects
            .iter()
            .filter(|effect| matches!(effect, ShellEffect::Animate(Cue::ToastRaised)))
            .count()
    }

    #[test]
    fn a_full_mailbox_raises_one_congestion_toast_per_episode() {
        let mut fixture = fixture();
        let (sender, mailbox) = bounded(CAPACITY);
        let congestion = Congestion::default();
        let outbox = Mailbox::new(sender.clone(), congestion.clone());
        let (library_commands, _library_inbox) = unbounded();
        fixture.runtime.wiring.mailbox = mailbox;
        fixture.runtime.wiring.mailbox_sender = sender;
        fixture.runtime.wiring.ports.library = LibraryPort::new(Port::new(
            Driver::Library,
            library_commands,
            congestion.clone(),
        ));

        fill_the_mailbox(&outbox, &congestion);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);

        fill_the_mailbox(&outbox, &congestion);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        fill_the_mailbox(&outbox, &congestion);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);
        fixture.runtime.drain();
    }

    #[test]
    fn a_key_behind_a_driver_flood_is_stepped_first() {
        let mut fixture = fixture();
        for _ in 0..50 {
            fixture
                .runtime
                .wiring
                .mailbox_sender
                .send(Message::Workspace(WorkspaceRequest::ShowToast(
                    Toast::error("fact".to_owned()),
                )))
                .unwrap();
        }
        let (keys, input) = unbounded();
        keys.send(Key::Quit).unwrap();
        let mut shell = Scripted::new(unbounded().0, usize::MAX);
        let mut event_loop = EventLoop::new(&mut fixture.runtime, &mut shell, &input);

        let first = event_loop.wait(None).unwrap();
        event_loop.gather(first);

        assert!(matches!(fixture.runtime.flow(), Flow::Stop));
        assert!(fixture.runtime.model.workspace.toast.is_none());
        fixture.runtime.drain();
    }
}
