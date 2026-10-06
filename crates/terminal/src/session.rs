use std::io::{self, Stdout, Write};

use crossterm::{
    cursor::Show,
    execute,
    terminal::{
        EnterAlternateScreen,
        LeaveAlternateScreen,
        disable_raw_mode,
        enable_raw_mode,
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{error::Error, window_colors};

fn restore_on_panic() -> Result<(), io::Error> {
    let raw_mode = disable_raw_mode();
    let window_colors = window_colors::reset_on_panic();
    let screen = execute!(io::stdout(), LeaveAlternateScreen, Show);
    raw_mode.and(window_colors).and(screen)
}

pub fn install_panic_hook() {
    let original = std::panic::take_hook();
    let painting_thread = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == painting_thread {
            drop(restore_on_panic());
        }
        original(info);
    }));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restoration {
    Raw,
    Cooked,
    Done,
}

pub struct TerminalSession<W: Write> {
    terminal: Terminal<CrosstermBackend<W>>,
    restoration: Restoration,
}

impl<W: Write> std::fmt::Debug for TerminalSession<W> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalSession")
            .field("restoration", &self.restoration)
            .finish_non_exhaustive()
    }
}

impl TerminalSession<Stdout> {
    pub fn enter() -> Result<Self, Error> {
        enable_raw_mode().map_err(Error::Setup)?;

        match execute!(io::stdout(), EnterAlternateScreen)
            .and_then(|()| Terminal::new(CrosstermBackend::new(io::stdout())))
        {
            Ok(terminal) => Ok(Self {
                terminal,
                restoration: Restoration::Raw,
            }),
            Err(error) => Err(abandon_setup(
                || execute!(io::stdout(), LeaveAlternateScreen),
                disable_raw_mode,
                error,
            )),
        }
    }
}

fn abandon_setup(
    leave_screen: impl FnOnce() -> Result<(), io::Error>,
    disable_raw: impl FnOnce() -> Result<(), io::Error>,
    error: io::Error,
) -> Error {
    let screen = leave_screen();
    let raw_mode = disable_raw();
    match screen.and(raw_mode) {
        Ok(()) => Error::Setup(error),
        Err(teardown) => Error::Teardown(teardown),
    }
}

impl<W: Write> TerminalSession<W> {
    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<W>> {
        &mut self.terminal
    }

    pub fn restore(&mut self) -> Result<(), io::Error> {
        match self.restoration {
            Restoration::Done => return Ok(()),
            Restoration::Raw => {
                disable_raw_mode()?;
                self.restoration = Restoration::Cooked;
            }
            Restoration::Cooked => {}
        }
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        self.restoration = Restoration::Done;
        Ok(())
    }
}

impl<W: Write> Drop for TerminalSession<W> {
    fn drop(&mut self) {
        drop(self.restore());
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        io::{self, Write},
        rc::Rc,
    };

    use ratatui::{
        Terminal,
        TerminalOptions,
        Viewport,
        backend::CrosstermBackend,
        layout::Rect,
    };
    use rstest::rstest;

    use crate::session::{Restoration, TerminalSession, abandon_setup};

    const LEAVE_ALTERNATE_SCREEN: &str = "\x1b[?1049l";

    const VIEWPORT: Rect = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };

    #[derive(Clone, Default)]
    struct Recorder {
        written: Rc<RefCell<Vec<u8>>>,
        writes_left_to_fail: Rc<Cell<usize>>,
    }

    impl Recorder {
        fn failing_once() -> Self {
            let recorder = Self::default();
            recorder.writes_left_to_fail.set(1);
            recorder
        }

        fn written(&self) -> String {
            String::from_utf8_lossy(&self.written.borrow()).into_owned()
        }

        fn leave_alternate_screens(&self) -> usize {
            self.written().matches(LEAVE_ALTERNATE_SCREEN).count()
        }
    }

    impl Write for Recorder {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if let Some(remaining) = self.writes_left_to_fail.get().checked_sub(1) {
                self.writes_left_to_fail.set(remaining);
                return Err(io::Error::other("the terminal went away mid-teardown"));
            }
            self.written.borrow_mut().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn session_over(recorder: Recorder) -> TerminalSession<Recorder> {
        let options = TerminalOptions {
            viewport: Viewport::Fixed(VIEWPORT),
        };
        TerminalSession {
            terminal: Terminal::with_options(CrosstermBackend::new(recorder), options)
                .unwrap(),
            restoration: Restoration::Cooked,
        }
    }

    fn step(outcome: Result<(), &'static str>) -> Result<(), io::Error> {
        outcome.map_err(io::Error::other)
    }

    #[rstest]
    #[case::both_succeed(Ok(()), Ok(()), "terminal setup: no tty")]
    #[case::screen_fails(Err("screen"), Ok(()), "terminal teardown: screen")]
    #[case::raw_mode_fails(Ok(()), Err("raw mode"), "terminal teardown: raw mode")]
    #[case::both_fail_keeps_the_screen_error(
        Err("screen"),
        Err("raw mode"),
        "terminal teardown: screen"
    )]
    fn an_abandoned_setup_runs_both_steps_and_combines_them(
        #[case] screen: Result<(), &'static str>,
        #[case] raw_mode: Result<(), &'static str>,
        #[case] expected: &str,
    ) {
        let raw_mode_ran = Cell::new(false);

        let error = abandon_setup(
            || step(screen),
            || {
                raw_mode_ran.set(true);
                step(raw_mode)
            },
            io::Error::other("no tty"),
        );

        assert!(raw_mode_ran.get());
        assert_eq!(error.to_string(), expected);
    }

    #[test]
    fn restore_is_idempotent() {
        let recorder = Recorder::default();
        let mut terminal_session = session_over(recorder.clone());

        terminal_session.restore().unwrap();
        terminal_session.restore().unwrap();
        drop(terminal_session);

        assert_eq!(recorder.leave_alternate_screens(), 1);
    }

    #[test]
    fn a_restore_that_failed_is_tried_again() {
        let recorder = Recorder::failing_once();
        let mut terminal_session = session_over(recorder.clone());

        assert!(terminal_session.restore().is_err());
        assert_eq!(recorder.leave_alternate_screens(), 0);

        terminal_session.restore().unwrap();
        drop(terminal_session);

        assert_eq!(recorder.leave_alternate_screens(), 1);
    }

    #[test]
    fn drop_restores_without_double_restoring() {
        let recorder = Recorder::default();

        drop(session_over(recorder.clone()));

        assert_eq!(recorder.leave_alternate_screens(), 1);
    }

    #[test]
    fn a_raw_session_restores_to_done() {
        let recorder = Recorder::default();
        let mut terminal_session = session_over(recorder.clone());
        terminal_session.restoration = Restoration::Raw;

        terminal_session.restore().unwrap();

        assert_eq!(terminal_session.restoration, Restoration::Done);
        assert_eq!(recorder.leave_alternate_screens(), 1);
    }
}
