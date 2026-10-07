use std::{
    mem,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, never, select_biased};
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
    input_receiver: &Receiver<S::Input>,
) -> Result<(), Error<S::Error>>
where
    S::Error: std::error::Error + 'static,
{
    let mut event_loop = EventLoop::new(&mut runtime, shell, input_receiver);
    let ended = event_loop.drive();
    if ended.is_err() {
        event_loop.step_and_repaint(Message::Quit, RepaintCause::Event);
        event_loop.run_shell_effects();
    }
    runtime.drain();
    ended
}

enum Arrival<I> {
    Input(I),
    Message(Message),
    Doorbell,
}

pub(crate) struct EventLoop<'a, S: Shell> {
    pub(crate) runtime: &'a mut Runtime,
    pub(crate) shell: &'a mut S,
    input_receiver: &'a Receiver<S::Input>,
    pub(crate) repaint: Repaint,
    pub(crate) last_paint_at: Option<Instant>,
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
        input_receiver: &'a Receiver<S::Input>,
    ) -> Self {
        Self {
            runtime,
            shell,
            input_receiver,
            repaint: Repaint::Now,
            last_paint_at: None,
            inputs: Vec::new(),
            messages: Vec::new(),
        }
    }

    pub(crate) fn drive(&mut self) -> Result<(), Error<S::Error>> {
        let mut frame_due = self.shell.frame_due(&self.runtime.frame(Instant::now()));
        loop {
            let deadline = self.deadline(Instant::now(), frame_due);
            let first = self.wait(deadline)?;
            self.gather(first);
            let now = Instant::now();
            self.fire_timers(now);
            self.report_congestion();
            self.run_shell_effects();
            if self.runtime.flow().is_break() {
                return Ok(());
            }
            frame_due = self.paint_if_due(now)?;
        }
    }

    fn run_shell_effects(&mut self) {
        for effect in self.runtime.take_shell_effects() {
            self.shell.effect(effect);
        }
    }

    fn wait(
        &mut self,
        deadline_at: Option<Instant>,
    ) -> Result<Option<Arrival<S::Input>>, Error<S::Error>> {
        let wiring = &mut self.runtime.wiring;
        let timeout = deadline_at.map_or(Duration::MAX, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        select_biased! {
            recv(self.input_receiver) -> input => {
                input
                    .map(|input| Some(Arrival::Input(input)))
                    .map_err(|_| Error::InputClosed)
            }
            recv(wiring.inbox_receiver) -> message => {
                Ok(message.map_or_else(
                    |_| {
                        wiring.inbox_receiver = never();
                        None
                    },
                    |message| Some(Arrival::Message(message)),
                ))
            }
            recv(wiring.doorbell) -> rang => {
                Ok(rang.map_or_else(
                    |_| {
                        wiring.doorbell = never();
                        None
                    },
                    |()| Some(Arrival::Doorbell),
                ))
            }
            default(timeout) => Ok(None),
        }
    }

    fn gather(&mut self, first: Option<Arrival<S::Input>>) {
        let Some(first) = first else {
            return;
        };
        let inbox_receiver = self.runtime.wiring.inbox_receiver.clone();
        let doorbell = self.runtime.wiring.doorbell.clone();
        let rang = matches!(first, Arrival::Doorbell) || ready(&doorbell).count() > 0;
        let mut inputs = mem::take(&mut self.inputs);
        let mut messages = mem::take(&mut self.messages);
        inputs.clear();
        messages.clear();
        match first {
            Arrival::Input(input) => inputs.push(input),
            Arrival::Message(message) => messages.push(message),
            Arrival::Doorbell => {}
        }
        inputs.extend(ready(self.input_receiver));
        messages.extend(ready(&inbox_receiver));
        let arrivals = inputs
            .drain(..)
            .map(Arrival::Input)
            .chain(messages.drain(..).map(Arrival::Message))
            .chain(rang.then_some(Arrival::Doorbell));
        for arrival in arrivals {
            if self.runtime.flow().is_break() {
                break;
            }
            self.route_arrival(arrival);
        }
        self.inputs = inputs;
        self.messages = messages;
    }

    fn route_arrival(&mut self, arrival: Arrival<S::Input>) {
        match arrival {
            Arrival::Input(input) => match self.shell.input(input) {
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
            Arrival::Doorbell => {
                self.repaint = repaint_after(self.repaint, RepaintCause::Event);
            }
        }
    }

    pub(crate) fn step_and_repaint(&mut self, message: Message, cause: RepaintCause) {
        if self.runtime.deliver(message).is_ok() {
            self.repaint = repaint_after(self.repaint, cause);
        }
    }

    fn report_congestion(&mut self) {
        for row in registry::REGISTRY {
            if self.runtime.flow().is_break() {
                return;
            }
            if let Some(event) = self.runtime.wiring.ports.congestion(row.driver_name) {
                self.step_and_repaint(
                    Message::Driver {
                        driver_name: row.driver_name,
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
        cell::RefCell,
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
        repaint::{FRAME_INTERVAL, Repaint, RepaintCause},
        runtime::Runtime,
        shell::{Frame, FrameDue, Painted, Reaction, Shell, ShellEffect},
        wiring::{INBOX_SLOTS, Wiring},
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
        key_sender: Sender<Key>,
        quit_after: usize,
        key: Option<Key>,
        frame_duration: Option<Duration>,
        asked_moments: RefCell<Vec<Moment>>,
        painted_moments: Vec<Moment>,
        cover_side: Option<Pixels>,
        shell_effects: Vec<ShellEffect>,
        pub(crate) toasts: Vec<Option<String>>,
        orders: Vec<Order>,
        pub(crate) pending_errors: Vec<kernel::message::PaintError>,
    }

    impl Scripted {
        pub(crate) fn new(key_sender: Sender<Key>, quit_after: usize) -> Self {
            Self {
                key_sender,
                quit_after,
                key: Some(Key::Stray),
                frame_duration: None,
                asked_moments: RefCell::new(Vec::new()),
                painted_moments: Vec::new(),
                cover_side: None,
                shell_effects: Vec::new(),
                toasts: Vec::new(),
                orders: Vec::new(),
                pending_errors: Vec::new(),
            }
        }
    }

    impl Shell for Scripted {
        type Input = Key;
        type Error = Infallible;

        fn input(&mut self, key: Key) -> Reaction {
            match key {
                Key::Quit => Reaction::Message(Message::Quit),
                Key::Stray => Reaction::Message(Message::Driver {
                    driver_name: DriverName::Audio,
                    event: DriverEvent::Stopped,
                }),
                Key::Ping => {
                    Reaction::Message(Message::Toast(Toast::error("ping".to_owned())))
                }
                Key::Resize => Reaction::Repaint,
            }
        }

        fn effect(&mut self, shell_effect: ShellEffect) {
            self.orders.push(Order::Effect(shell_effect.clone()));
            self.shell_effects.push(shell_effect);
        }

        fn frame_due(&self, frame: &Frame<'_>) -> FrameDue {
            self.asked_moments.borrow_mut().push(frame.now);
            self.frame_duration.map_or(FrameDue::Settled, |interval| {
                FrameDue::At(self.painted_moments.last().map_or(frame.now, |last| {
                    Moment::new(last.since_epoch() + interval)
                }))
            })
        }

        fn paint(&mut self, frame: Frame<'_>) -> Result<Painted, Infallible> {
            if frame.latest_receivers.theme_receiver.take().is_some() {
                self.orders.push(Order::Reloaded);
            }
            self.painted_moments.push(frame.now);
            let toast = frame.model.workspace.toasts.first();
            self.toasts.push(toast.map(|toast| toast.title.clone()));
            let next = if self.toasts.len() >= self.quit_after {
                Some(Key::Quit)
            } else {
                self.key
            };
            if let Some(next) = next {
                self.key_sender
                    .send(next)
                    .expect("the key receiver outlives the shell");
            }
            Ok(Painted {
                cover_side: self.cover_side,
                visible_rows: frame.model.workspace.visible_rows,
                errors: std::mem::take(&mut self.pending_errors),
            })
        }
    }

    pub(crate) struct Fixture {
        pub(crate) runtime: Runtime,
        library_cmd_receiver: Receiver<LibraryCmd>,
        _latest_senders: LatestSenders,
    }

    pub(crate) fn stock_startup() -> Startup {
        Startup::default()
    }

    pub(crate) fn fixture() -> Fixture {
        let (wiring, library_cmd_receiver, latest_senders) = Wiring::idle();
        let started = kernel::update::startup::startup(stock_startup());
        Fixture {
            runtime: Runtime::assemble(started, wiring).unwrap(),
            library_cmd_receiver,
            _latest_senders: latest_senders,
        }
    }

    #[test]
    fn quit_before_paint_resets_the_window_and_paints_nothing() {
        let mut fixture = fixture();
        let (keys, input) = unbounded();
        keys.send(Key::Quit).unwrap();
        let mut shell_scripted = Scripted::new(keys, usize::MAX);

        let ended =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert!(shell_scripted.toasts.is_empty());
        assert_eq!(
            shell_scripted.shell_effects,
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
        let mut shell_scripted = Scripted::new(keys, 1);
        let mut event_loop =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input);
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
        let mut shell_scripted = Scripted::new(unbounded().0, usize::MAX);

        let ended =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input).drive();

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
        let mut shell_scripted = Scripted::new(keys, 1);

        let ended =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell_scripted.toasts, vec![Some("hello".to_owned())]);
        assert!(
            !shell_scripted
                .shell_effects
                .contains(&ShellEffect::Animate(Cue::ToastDismissed))
        );
        fixture.runtime.drain();
    }

    #[test]
    fn a_frame_due_paint_asks_once_per_iteration_with_no_empty_iteration() {
        let mut fixture = fixture();
        let (keys, input) = unbounded();
        let mut shell_scripted = Scripted::new(keys, 4);
        shell_scripted.key = None;
        shell_scripted.frame_duration = Some(FRAME_INTERVAL);

        let ended =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell_scripted.painted_moments.len(), 4);
        let mut asked_moments = shell_scripted.asked_moments.take();
        asked_moments.retain(|at| !shell_scripted.painted_moments.contains(at));
        assert!(
            asked_moments.len() <= 1,
            "asked between paints: {asked_moments:?}"
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
        let mut shell_scripted = Scripted::new(keys, 1);

        let ended =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell_scripted.toasts, vec![None]);
        assert_eq!(
            shell_scripted.shell_effects,
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
        fixture
            .runtime
            .model
            .settings
            .appearance_settings
            .cover_mode = CoverMode::Plain;
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
        let mut shell_scripted = Scripted::new(keys, 2);
        shell_scripted.key = Some(Key::Ping);
        shell_scripted.cover_side = Some(Pixels(64));

        let ended =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell_scripted.toasts.len(), 2);
        fixture.runtime.drain();
        let decodes = fixture
            .library_cmd_receiver
            .iter()
            .filter(|cmd| matches!(cmd, LibraryCmd::DecodeCover(_)))
            .count();
        assert_eq!(decodes, 1);
    }

    #[test]
    fn a_repaint_request_paints_once_and_sends_no_kernel_message() {
        let mut fixture = fixture();
        let (keys, input) = unbounded();
        keys.send(Key::Resize).unwrap();
        let mut shell_scripted = Scripted::new(keys, 1);

        let ended =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(shell_scripted.toasts, vec![None]);
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
        fixture._latest_senders.theme_sender.publish(stub_theme());
        let event = ConfigEvent::ThemeReloaded(ThemeName::from_static("test"));
        fixture
            .runtime
            .wiring
            .inbox
            .clone()
            .send(Message::Config(event))
            .unwrap();
        let (keys, input) = unbounded();
        let mut shell_scripted = Scripted::new(keys, 1);

        let ended =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input).drive();

        assert!(matches!(ended, Ok(())));
        assert_eq!(
            fixture.runtime.model.revisions.theme.get(),
            before.next().get()
        );
        let animated_at = shell_scripted
            .orders
            .iter()
            .position(|order| {
                matches!(
                    order,
                    Order::Effect(ShellEffect::Animate(Cue::ThemeChanged))
                )
            })
            .unwrap();
        let reloaded_at = shell_scripted
            .orders
            .iter()
            .position(|order| matches!(order, Order::Reloaded))
            .unwrap();
        assert!(animated_at < reloaded_at);
        fixture.runtime.drain();
    }

    fn fill_the_inbox(inbox: &Sender<Message>, congestion: &Congestion) {
        for _ in 0..INBOX_SLOTS {
            let delivery = send(
                inbox,
                congestion,
                LibraryEvent::HistoryLoaded(Vec::new()).into(),
            );
            assert!(matches!(delivery, Ok(())));
        }
        congestion.raise();
    }

    fn toasts_in_one_iteration(runtime: &mut Runtime) -> usize {
        let (_keys, input) = unbounded::<Key>();
        let mut shell_scripted = Scripted::new(unbounded().0, usize::MAX);
        let mut event_loop = EventLoop::new(runtime, &mut shell_scripted, &input);
        let first = event_loop.wait(Some(Instant::now())).unwrap();
        event_loop.gather(first);
        event_loop.report_congestion();
        let effects = event_loop.runtime.take_shell_effects();
        for effect in effects {
            event_loop.shell.effect(effect);
        }
        shell_scripted
            .shell_effects
            .iter()
            .filter(|effect| matches!(effect, ShellEffect::Animate(Cue::ToastRaised)))
            .count()
    }

    fn congested_library_port(runtime: &mut Runtime) -> (Sender<Message>, Congestion) {
        let (inbox, inbox_receiver) = bounded(INBOX_SLOTS);
        let congestion = Congestion::default();
        let (library_cmd_sender, _library_cmd_receiver) = unbounded();
        runtime.wiring.inbox_receiver = inbox_receiver;
        runtime.wiring.inbox = inbox.clone();
        runtime.wiring.ports.library =
            Port::new(DriverName::Library, library_cmd_sender, congestion.clone());
        (inbox, congestion)
    }

    #[test]
    fn a_full_inbox_raises_a_toast_per_full_episode() {
        let mut fixture = fixture();
        let (inbox, congestion) = congested_library_port(&mut fixture.runtime);

        fill_the_inbox(&inbox, &congestion);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);

        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        fill_the_inbox(&inbox, &congestion);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);

        fill_the_inbox(&inbox, &congestion);
        fixture.runtime.flow = ControlFlow::Break(());
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);
        assert!(congestion.take());
        fixture.runtime.drain();
    }

    #[test]
    fn two_flagged_batches_in_one_congestion_episode_report_full_once() {
        let mut fixture = fixture();
        let (inbox, congestion) = congested_library_port(&mut fixture.runtime);

        fill_the_inbox(&inbox, &congestion);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);

        fill_the_inbox(&inbox, &congestion);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);
        fixture.runtime.drain();
    }

    #[test]
    fn a_new_congestion_episode_after_the_inbox_drains_reports_full_again() {
        let mut fixture = fixture();
        let (inbox, congestion) = congested_library_port(&mut fixture.runtime);

        fill_the_inbox(&inbox, &congestion);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 1);
        fill_the_inbox(&inbox, &congestion);
        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        assert_eq!(toasts_in_one_iteration(&mut fixture.runtime), 0);

        fill_the_inbox(&inbox, &congestion);
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
        let mut shell_scripted = Scripted::new(unbounded().0, usize::MAX);
        let mut event_loop =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input);

        let first = event_loop.wait(None).unwrap();
        event_loop.gather(first);

        assert!(fixture.runtime.flow().is_break());
        assert!(fixture.runtime.model.workspace.toasts.is_empty());
        fixture.runtime.drain();
    }
}
