use std::time::Instant;

use crossbeam_channel::{Receiver, Select, never};
use kernel::Message;

use crate::{
    error::Error,
    repaint::{Repaint, Source, repaint_after},
    runtime::Runtime,
    shell::{Reaction, Shell},
};

pub fn run<S: Shell>(
    mut runtime: Runtime,
    shell: &mut S,
    input: &Receiver<S::Input>,
) -> Result<(), Error<S::Error>>
where
    S::Error: std::error::Error + 'static,
{
    let ended = EventLoop::new(&mut runtime, shell, input).drive();
    runtime.drain();
    ended
}

enum Arrival<I> {
    Input(I),
    Message(Message),
    Notified,
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
    S::Error: std::error::Error + 'static,
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

    pub(crate) fn drive(mut self) -> Result<(), Error<S::Error>> {
        let mut frame_due = self.shell.frame_due(&self.runtime.frame(Instant::now()));
        loop {
            let now = Instant::now();
            let deadline = self.deadline(now, frame_due);
            let first = self.wait(deadline)?;
            self.gather(first);
            self.fire_timers(now);
            self.runtime.report_full();
            for effect in self.runtime.take_shell_effects() {
                self.shell.effect(effect);
            }
            if self.runtime.flow().is_break() {
                return Ok(());
            }
            frame_due = self.shell.frame_due(&self.runtime.frame(now));
            self.paint_if_due(now, frame_due)?;
        }
    }

    fn wait(
        &mut self,
        deadline: Option<Instant>,
    ) -> Result<Option<Arrival<S::Input>>, Error<S::Error>> {
        let wiring = &mut self.runtime.wiring;
        let mut select = Select::new();
        let input_index = select.recv(self.input);
        let sender_index = select.recv(&wiring.receiver);
        select.recv(&wiring.notified);
        let operation = match deadline {
            Some(deadline) => match select.select_deadline(deadline) {
                Ok(operation) => operation,
                Err(_) => return Ok(None),
            },
            None => select.select(),
        };
        let index = operation.index();
        if index == input_index {
            return operation
                .recv(self.input)
                .map(|event| Some(Arrival::Input(event)))
                .map_err(|_| Error::InputClosed);
        }
        if index == sender_index {
            return Ok(operation.recv(&wiring.receiver).map_or_else(
                |_| {
                    wiring.receiver = never();
                    None
                },
                |message| Some(Arrival::Message(message)),
            ));
        }
        Ok(operation.recv(&wiring.notified).map_or_else(
            |_| {
                wiring.notified = never();
                None
            },
            |()| Some(Arrival::Notified),
        ))
    }

    fn gather(&mut self, first: Option<Arrival<S::Input>>) {
        let Some(first) = first else {
            return;
        };
        let queued = self.runtime.wiring.receiver.clone();
        let notified = self.runtime.wiring.notified.clone();
        let (first_input, first_message, first_rang) = match first {
            Arrival::Input(event) => (Some(event), None, false),
            Arrival::Message(message) => (None, Some(message), false),
            Arrival::Notified => (None, None, true),
        };
        let inputs: Vec<_> = first_input.into_iter().chain(ready(self.input)).collect();
        let messages: Vec<_> =
            first_message.into_iter().chain(ready(&queued)).collect();
        let rang = first_rang || ready(&notified).count() > 0;
        let arrivals = inputs
            .into_iter()
            .map(Arrival::Input)
            .chain(messages.into_iter().map(Arrival::Message))
            .chain(rang.then_some(Arrival::Notified));
        for arrival in arrivals {
            if self.runtime.flow().is_break() {
                return;
            }
            self.dispatch_arrival(arrival);
        }
    }

    fn dispatch_arrival(&mut self, arrival: Arrival<S::Input>) {
        match arrival {
            Arrival::Input(event) => match self.shell.input(event) {
                Reaction::Message(message) => {
                    self.step_and_repaint(message, Source::Input);
                }
                Reaction::Repaint => {
                    self.repaint = repaint_after(self.repaint, Source::Input);
                }
                Reaction::Ignored => {}
            },
            Arrival::Message(message) => {
                self.step_and_repaint(message, Source::Event);
            }
            Arrival::Notified => {
                self.repaint = repaint_after(self.repaint, Source::Event);
            }
        }
    }

    pub(crate) fn step_and_repaint(&mut self, message: Message, source: Source) {
        if self.runtime.step(message) {
            self.repaint = repaint_after(self.repaint, source);
        }
    }

    fn fire_timers(&mut self, now: Instant) {
        for timer in self.runtime.timers.due(now) {
            self.step_and_repaint(Message::Elapsed(timer), Source::Event);
        }
    }
}

fn ready<T>(receiver: &Receiver<T>) -> impl Iterator<Item = T> + '_ {
    receiver.try_iter().take(receiver.len())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{convert::Infallible, path::PathBuf, time::Instant};

    use config::{Rgb, ThemeColors, ThemeFile};
    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use kernel::{
        ConfigEvent,
        Cue,
        DriverMessage,
        LibraryEvent,
        Message,
        Outbox,
        Timer,
        WindowColorsCmd,
        domain::{Driver, DriverStatus, Startup, ThemeName, Toast},
        update::{DriverStatusError, UpdateError},
    };

    use crate::{
        error::Error,
        event_loop::EventLoop,
        latest::LatestSenders,
        library::{cover::CoverRequest, machine::LibraryMessage},
        port::{LibraryPort, Port},
        runtime::Runtime,
        sender::{DriverSender, FullEdge},
        shell::{Frame, FrameDue, Painted, Reaction, Shell, ShellEffect},
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
                Key::Stray => Reaction::Message(Message::Driver {
                    driver: Driver::Audio,
                    event: DriverMessage::Stopped,
                }),
                Key::Ping => {
                    Reaction::Message(Message::Toast(Toast::error("ping".to_owned())))
                }
                Key::Resize => Reaction::Repaint,
            }
        }

        fn effect(&mut self, effect: ShellEffect) {
            self.order.push(Order::Effect(effect.clone()));
            self.effects.push(effect);
        }

        fn frame_due(&self, _frame: &Frame<'_>) -> FrameDue {
            FrameDue::Settled
        }

        fn paint(&mut self, frame: Frame<'_>) -> Result<Painted, Infallible> {
            if frame.latest.theme.take().is_some() {
                self.order.push(Order::Reloaded);
            }
            let toast = frame.model.workspace.toast.as_ref();
            self.toasts.push(toast.map(|toast| toast.text.clone()));
            let next = if self.toasts.len() >= self.quit_after {
                Key::Quit
            } else {
                self.continue_with
            };
            let _ = self.keys.send(next);
            Ok(Painted {
                cover: self.cover.clone(),
                visible_rows: None,
                toasts: std::mem::take(&mut self.pending_failures),
            })
        }
    }

    pub(crate) struct Fixture {
        pub(crate) runtime: Runtime,
        cover_inbox: Receiver<LibraryMessage>,
        _writers: LatestSenders,
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

        assert!(matches!(ended, Err(Error::InputClosed)));
        fixture.runtime.drain();
    }

    #[test]
    fn a_timer_fires_elapsed_with_no_input() {
        let mut fixture = fixture();
        fixture
            .runtime
            .step(Message::Toast(Toast::error("hello".to_owned())));
        let generation = fixture.runtime.model.revisions.toast;
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
                error: UpdateError::Driver(Driver::Audio, DriverStatusError::Stopped),
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
            size_px: 64,
        });

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell.toasts.len(), 2);
        fixture.runtime.drain();
        let covers = fixture
            .cover_inbox
            .iter()
            .filter(|command| matches!(command, LibraryMessage::Cover(_)))
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
            name: ThemeName::from_static("test"),
            colors: ThemeColors {
                background: Rgb([0, 0, 0]),
                foreground: Rgb([255, 255, 255]),
                bright_foreground: Rgb([255, 255, 255]),
                accent: Rgb([0, 0, 0]),
                green: Rgb([0, 0, 0]),
                yellow: Rgb([0, 0, 0]),
                red: Rgb([0, 0, 0]),
                window_background: None,
            },
            scanning_label: "scanning".to_owned(),
        }
    }

    #[test]
    fn a_theme_reload_bumps_the_revision_and_animates_after_reloaded() {
        let mut fixture = fixture();
        let before = fixture.runtime.model.revisions.theme;
        fixture._writers.theme.publish(stub_theme());
        let event = ConfigEvent::ThemeReloaded(ThemeName::from_static("test"));
        fixture
            .runtime
            .sender()
            .send(Message::Config(event))
            .unwrap();
        let (keys, input) = unbounded();
        let mut shell = Scripted::new(keys, 1);

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(
            fixture.runtime.model.revisions.theme.get(),
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

    fn fill_the_sender(outbox: &DriverSender<LibraryEvent>, full_edge: &FullEdge) {
        for _ in 0..CAPACITY {
            let delivery = outbox.send(LibraryEvent::HistoryLoaded(Vec::new()));
            assert!(matches!(delivery, Ok(())));
        }
        full_edge.raise();
    }

    fn toasts_in_one_iteration(runtime: &mut Runtime) -> usize {
        let (_keys, input) = unbounded::<Key>();
        let mut shell = Scripted::new(unbounded().0, usize::MAX);
        let mut event_loop = EventLoop::new(runtime, &mut shell, &input);
        let first = event_loop.wait(Some(Instant::now())).unwrap();
        event_loop.gather(first);
        event_loop.runtime.report_full();
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
    fn a_full_sender_raises_a_toast_per_full_edge() {
        let mut fixture = fixture();
        let (sender, arrivals) = bounded(CAPACITY);
        let full_edge = FullEdge::default();
        let outbox = DriverSender::new(sender.clone(), full_edge.clone());
        let (library_commands, _library_inbox) = unbounded();
        fixture.runtime.wiring.receiver = arrivals;
        fixture.runtime.wiring.sender = sender;
        fixture.runtime.wiring.ports.library = LibraryPort::new(Port::new(
            Driver::Library,
            library_commands,
            full_edge.clone(),
        ));

        fill_the_sender(&outbox, &full_edge);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);

        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        fill_the_sender(&outbox, &full_edge);
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
                .sender
                .send(Message::Toast(Toast::error("event".to_owned())))
                .unwrap();
        }
        let (keys, input) = unbounded();
        keys.send(Key::Quit).unwrap();
        let mut shell = Scripted::new(unbounded().0, usize::MAX);
        let mut event_loop = EventLoop::new(&mut fixture.runtime, &mut shell, &input);

        let first = event_loop.wait(None).unwrap();
        event_loop.gather(first);

        assert!(fixture.runtime.flow().is_break());
        assert!(fixture.runtime.model.workspace.toast.is_none());
        fixture.runtime.drain();
    }
}
