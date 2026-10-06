use std::{mem, time::Instant};

use crossbeam_channel::{Receiver, Select, never};
use kernel::message::Message;

use crate::{
    error::Error,
    registry,
    repaint::{Repaint, RepaintCause, repaint_after},
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
    inputs: Vec<S::Input>,
    messages: Vec<Message>,
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
            inputs: Vec::new(),
            messages: Vec::new(),
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
            self.report_congestion();
            let effects = self.runtime.take_shell_effects();
            for effect in effects {
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
        let sender_index = select.recv(&wiring.mailbox);
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
            return Ok(operation.recv(&wiring.mailbox).map_or_else(
                |_| {
                    wiring.mailbox = never();
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
        let queued = self.runtime.wiring.mailbox.clone();
        let notified = self.runtime.wiring.notified.clone();
        let rang = matches!(first, Arrival::Notified) || ready(&notified).count() > 0;
        let mut inputs = mem::take(&mut self.inputs);
        let mut messages = mem::take(&mut self.messages);
        inputs.clear();
        messages.clear();
        match first {
            Arrival::Input(event) => inputs.push(event),
            Arrival::Message(message) => messages.push(message),
            Arrival::Notified => {}
        }
        inputs.extend(ready(self.input));
        messages.extend(ready(&queued));
        let arrivals = inputs
            .drain(..)
            .map(Arrival::Input)
            .chain(messages.drain(..).map(Arrival::Message))
            .chain(rang.then_some(Arrival::Notified));
        for arrival in arrivals {
            if self.runtime.flow().is_break() {
                break;
            }
            self.dispatch_arrival(arrival);
        }
        self.inputs = inputs;
        self.messages = messages;
    }

    fn dispatch_arrival(&mut self, arrival: Arrival<S::Input>) {
        match arrival {
            Arrival::Input(event) => match self.shell.input(event) {
                Reaction::Message(message) => {
                    self.step_and_repaint(message, RepaintCause::Input);
                }
                Reaction::Repaint => {
                    self.repaint = repaint_after(self.repaint, RepaintCause::Input);
                }
                Reaction::Ignored => {}
            },
            Arrival::Message(message) => {
                self.step_and_repaint(message, RepaintCause::Event);
            }
            Arrival::Notified => {
                self.repaint = repaint_after(self.repaint, RepaintCause::Event);
            }
        }
    }

    pub(crate) fn step_and_repaint(&mut self, message: Message, source: RepaintCause) {
        if self.runtime.deliver(message).is_ok() {
            self.repaint = repaint_after(self.repaint, source);
        }
    }

    fn report_congestion(&mut self) {
        for row in registry::REGISTRY {
            if self.runtime.flow().is_break() {
                return;
            }
            if let Some(event) = self.runtime.wiring.ports.full(row.driver) {
                self.step_and_repaint(
                    Message::Driver {
                        driver: row.driver,
                        event,
                    },
                    RepaintCause::Event,
                );
            }
        }
    }

    fn fire_timers(&mut self, now: Instant) {
        for timer in self.runtime.timers.take_due(now) {
            self.step_and_repaint(Message::Elapsed(timer), RepaintCause::Event);
        }
    }
}

fn ready<T>(receiver: &Receiver<T>) -> impl Iterator<Item = T> + '_ {
    receiver.try_iter().take(receiver.len())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{
        convert::Infallible,
        ops::ControlFlow,
        path::Path,
        sync::Arc,
        time::{Duration, Instant},
    };

    use config::theme_file::{TomlColors, TomlTheme};
    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use kernel::{
        cmd::{LibraryCmd, WindowColorsCmd},
        domain::{
            appearance::{CoverMode, Rgb},
            chord::ChordPrefix,
            cue::Cue,
            driver::{DriverName, DriverStatus},
            geometry::Pixels,
            key::{Key as KernelKey, KeyCode, KeyPress, Modifiers},
            player::Player,
            playhead::Playhead,
            speed::Speed,
            startup::Startup,
            theme::ThemeName,
            time::Moment,
            toast::Toast,
            track::Track,
        },
        message::{ConfigEvent, DriverEvent, LibraryEvent, Message, Timer},
    };

    use crate::{
        driver_thread::{Congestion, send},
        error::Error,
        event_loop::EventLoop,
        latest::LatestSenders,
        port::Port,
        repaint::{Repaint, RepaintCause},
        runtime::Runtime,
        shell::{Frame, FrameDue, Painted, Reaction, Shell, ShellEffect},
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
        cover_side: Option<Pixels>,
        effects: Vec<ShellEffect>,
        pub(crate) toasts: Vec<Option<String>>,
        order: Vec<Order>,
        pub(crate) pending_failures: Vec<kernel::message::PaintError>,
    }

    impl Scripted {
        pub(crate) fn new(keys: Sender<Key>, quit_after: usize) -> Self {
            Self {
                keys,
                quit_after,
                continue_with: Key::Stray,
                cover_side: None,
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
                cover_side: self.cover_side,
                visible_rows: frame.model.workspace.visible_rows,
                errors: std::mem::take(&mut self.pending_failures),
            })
        }
    }

    pub(crate) struct Fixture {
        pub(crate) runtime: Runtime,
        library_inbox: Receiver<LibraryCmd>,
        _writers: LatestSenders,
    }

    pub(crate) fn stock_startup() -> Startup {
        Startup::default()
    }

    pub(crate) fn fixture() -> Fixture {
        let (wiring, library_inbox, writers) = Wiring::idle();
        let started = kernel::update::startup::startup(stock_startup());
        Fixture {
            runtime: Runtime::assemble(started, wiring).unwrap(),
            library_inbox,
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
    fn cancelling_a_chord_repaints_once() {
        let mut fixture = fixture();
        fixture.runtime.model.workspace.chord_prefix = Some(ChordPrefix::G);
        let (_keys, input) = unbounded();
        let (keys, _receiver) = unbounded();
        let mut shell = Scripted::new(keys, 1);
        let mut event_loop = EventLoop::new(&mut fixture.runtime, &mut shell, &input);
        event_loop.repaint = Repaint::Settled;
        let key = KernelKey {
            code: KeyCode::Char('w'),
            modifiers: Modifiers::default(),
        };

        event_loop.step_and_repaint(
            Message::Key(KeyPress { key, typed: key }),
            RepaintCause::Input,
        );
        assert_eq!(event_loop.repaint, Repaint::Now);
        assert_eq!(event_loop.runtime.model.workspace.chord_prefix, None);
        event_loop.repaint = Repaint::Settled;
        event_loop.step_and_repaint(
            Message::Key(KeyPress { key, typed: key }),
            RepaintCause::Input,
        );
        assert_eq!(event_loop.repaint, Repaint::Settled);
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
            fixture
                .runtime
                .deliver(Message::Toast(Toast::error("hello"))),
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
    fn a_side_the_frame_keeps_reporting_decodes_the_cover_once() {
        let mut fixture = fixture();
        fixture.runtime.model.settings.appearance.cover_mode = CoverMode::Plain;
        fixture.runtime.model.player = Player::Playing {
            track: Arc::new(Track::listed(Path::new("/music/cover.mp3"))),
            playhead: Playhead::anchored(
                Duration::ZERO,
                Moment::default(),
                Speed::default(),
            ),
            preloaded: None,
        };
        let (keys, input) = unbounded();
        keys.send(Key::Ping).unwrap();
        let mut shell = Scripted::new(keys, 2);
        shell.continue_with = Key::Ping;
        shell.cover_side = Some(Pixels(64));

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell.toasts.len(), 2);
        fixture.runtime.drain();
        let decodes = fixture
            .library_inbox
            .iter()
            .filter(|command| matches!(command, LibraryCmd::DecodeCover(_)))
            .count();
        assert_eq!(decodes, 1);
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
        fixture.runtime.drain();
    }

    fn stub_theme() -> TomlTheme {
        TomlTheme {
            name: ThemeName::from_static("test"),
            colors: TomlColors {
                background: Rgb([0, 0, 0]),
                muted_foreground: Rgb([255, 255, 255]),
                foreground: Rgb([255, 255, 255]),
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

    fn fill_the_inbox(inbox: &Sender<Message>, full: &Congestion) {
        for _ in 0..CAPACITY {
            let delivery =
                send(inbox, full, LibraryEvent::HistoryLoaded(Vec::new()).into());
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
        event_loop.report_congestion();
        let effects = event_loop.runtime.take_shell_effects();
        for effect in effects {
            event_loop.shell.effect(effect);
        }
        shell
            .effects
            .iter()
            .filter(|effect| matches!(effect, ShellEffect::Animate(Cue::ToastRaised)))
            .count()
    }

    fn congested_library_port(runtime: &mut Runtime) -> (Sender<Message>, Congestion) {
        let (inbox, arrivals) = bounded(CAPACITY);
        let full = Congestion::default();
        let (library_commands, _library_inbox) = unbounded();
        runtime.wiring.mailbox = arrivals;
        runtime.wiring.inbox = inbox.clone();
        runtime.wiring.ports.library =
            Port::new(DriverName::Library, library_commands, full.clone());
        (inbox, full)
    }

    #[test]
    fn a_full_inbox_raises_a_toast_per_full_episode() {
        let mut fixture = fixture();
        let (inbox, full) = congested_library_port(&mut fixture.runtime);

        fill_the_inbox(&inbox, &full);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);

        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        fill_the_inbox(&inbox, &full);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);

        fill_the_inbox(&inbox, &full);
        fixture.runtime.flow = ControlFlow::Break(());
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);
        assert!(full.take());
        fixture.runtime.drain();
    }

    #[test]
    fn two_flagged_batches_in_one_congestion_episode_report_full_once() {
        let mut fixture = fixture();
        let (inbox, full) = congested_library_port(&mut fixture.runtime);

        fill_the_inbox(&inbox, &full);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);

        fill_the_inbox(&inbox, &full);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);
        fixture.runtime.drain();
    }

    #[test]
    fn a_new_congestion_episode_after_the_inbox_drains_reports_full_again() {
        let mut fixture = fixture();
        let (inbox, full) = congested_library_port(&mut fixture.runtime);

        fill_the_inbox(&inbox, &full);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);
        fill_the_inbox(&inbox, &full);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        fill_the_inbox(&inbox, &full);
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
