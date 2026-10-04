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
            let deadline = self.deadline(Instant::now(), frame_due);
            let first = self.wait(deadline)?;
            self.gather(first);
            let now = Instant::now();
            self.fire_timers(now);
            self.runtime.report_full();
            let effects = self.runtime.take_shell_effects();
            let animations = self.runtime.animations();
            for effect in effects {
                self.shell.effect(effect, animations);
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
        let mut inputs = Vec::new();
        let mut messages = Vec::new();
        let mut notices = Vec::new();
        match first {
            Arrival::Input(event) => inputs.push(event),
            Arrival::Message(message) => messages.push(message),
            Arrival::Notified => notices.push(Arrival::Notified),
        }
        inputs.extend(ready(self.input));
        messages.extend(ready(&queued));
        if notices.is_empty() && ready(&notified).count() > 0 {
            notices.push(Arrival::Notified);
        }
        let arrivals = inputs
            .into_iter()
            .map(Arrival::Input)
            .chain(messages.into_iter().map(Arrival::Message))
            .chain(notices);
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
        if self.runtime.step(message).is_ok() {
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

    use config::{TomlColors, TomlTheme};
    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use kernel::{
        ConfigEvent,
        Cue,
        DriverEvent,
        LibraryEvent,
        Message,
        Timer,
        WindowColorsCmd,
        domain::{
            DriverName,
            DriverStatus,
            Startup,
            ThemeName,
            Toast,
            appearance::Rgb,
        },
    };
    use library::{CoverJob, LibraryMessage};

    use crate::{
        error::Error,
        event_loop::EventLoop,
        latest::LatestSenders,
        outbox::{Congestion, Outbox},
        port::{LibraryPort, Port},
        runtime::Runtime,
        shell::{Frame, FrameDue, Painted, Reaction, Shell, ShellEffect},
        trace::Trace,
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
        cover: Option<CoverJob>,
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
                    driver: DriverName::Audio,
                    event: DriverEvent::Stopped,
                }),
                Key::Ping => {
                    Reaction::Message(Message::Toast(Toast::error("ping".to_owned())))
                }
                Key::Resize => Reaction::Repaint,
            }
        }

        fn effect(
            &mut self,
            effect: ShellEffect,
            _animations: kernel::domain::appearance::Animations,
        ) {
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
            let toast = frame.model.workspace.toasts.first();
            self.toasts.push(toast.map(|toast| toast.title.clone()));
            let next = if self.toasts.len() >= self.quit_after {
                Key::Quit
            } else {
                self.continue_with
            };
            self.keys
                .send(next)
                .expect("the key receiver outlives the shell");
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
        assert_eq!(
            fixture.runtime.step(Message::Toast(Toast::error("hello"))),
            Ok(())
        );
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
        assert_eq!(shell.toasts, vec![Some("hello".to_owned())]);
        assert!(
            !shell
                .effects
                .contains(&ShellEffect::Animate(Cue::ToastDismissed))
        );
        fixture.runtime.drain();
    }

    #[test]
    fn a_rejection_repaints_nothing() {
        let mut fixture = fixture();
        fixture
            .runtime
            .model
            .drivers
            .record_mut(DriverName::Audio)
            .status = DriverStatus::Stopped;
        let (keys, input) = unbounded();
        keys.send(Key::Stray).unwrap();
        let mut shell = Scripted::new(keys, 1);

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell.toasts, vec![None]);
        assert_eq!(
            shell.effects,
            vec![ShellEffect::WindowColors(WindowColorsCmd::Reset)]
        );
        assert_eq!(
            fixture.runtime.model.drivers.status(DriverName::Audio),
            &DriverStatus::Stopped
        );
        fixture.runtime.drain();
    }

    #[test]
    fn a_cover_the_frame_keeps_asking_for_is_forwarded_on_every_paint() {
        let mut fixture = fixture();
        let (keys, input) = unbounded();
        keys.send(Key::Ping).unwrap();
        let mut shell = Scripted::new(keys, 2);
        shell.continue_with = Key::Ping;
        shell.cover = Some(CoverJob::new(
            PathBuf::from("/music/cover.mp3"),
            kernel::domain::geometry::Pixels(64),
        ));

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell.toasts.len(), 2);
        fixture.runtime.drain();
        let covers = fixture
            .cover_inbox
            .iter()
            .filter(|command| matches!(command, LibraryMessage::Cover(_)))
            .count();
        assert_eq!(covers, 2);
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

    fn stub_theme() -> TomlTheme {
        TomlTheme {
            name: ThemeName::from_static("test"),
            colors: TomlColors {
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
            .wiring
            .inbox
            .clone()
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

    fn fill_the_inbox(outbox: &Outbox<LibraryEvent>, full: &Congestion) {
        for _ in 0..CAPACITY {
            let delivery = outbox.send(LibraryEvent::HistoryLoaded(Vec::new()));
            assert!(matches!(delivery, Ok(())));
        }
        full.raise();
    }

    fn toasts_in_one_iteration(runtime: &mut Runtime) -> usize {
        let (_keys, input) = unbounded::<Key>();
        let mut shell = Scripted::new(unbounded().0, usize::MAX);
        let mut event_loop = EventLoop::new(runtime, &mut shell, &input);
        let first = event_loop.wait(Some(Instant::now())).unwrap();
        event_loop.gather(first);
        event_loop.runtime.report_full();
        let effects = event_loop.runtime.take_shell_effects();
        let animations = event_loop
            .runtime
            .frame(Instant::now())
            .model
            .settings
            .appearance
            .settings
            .animations;
        for effect in effects {
            event_loop.shell.effect(effect, animations);
        }
        shell
            .effects
            .iter()
            .filter(|effect| matches!(effect, ShellEffect::Animate(Cue::ToastRaised)))
            .count()
    }

    #[test]
    fn a_full_inbox_raises_a_toast_per_full_episode() {
        let mut fixture = fixture();
        let (inbox, arrivals) = bounded(CAPACITY);
        let full = Congestion::default();
        let outbox = Outbox::new(inbox.clone(), full.clone());
        let (library_commands, _library_inbox) = unbounded();
        fixture.runtime.wiring.receiver = arrivals;
        fixture.runtime.wiring.inbox = inbox;
        fixture.runtime.wiring.ports.library = LibraryPort::new(
            Port::new(DriverName::Library, library_commands, full.clone()),
            unbounded().0,
        );

        fill_the_inbox(&outbox, &full);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);

        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        fill_the_inbox(&outbox, &full);
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
                .inbox
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
        assert!(fixture.runtime.model.workspace.toasts.is_empty());
        fixture.runtime.drain();
    }
}
