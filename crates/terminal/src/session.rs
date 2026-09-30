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

fn leave_the_alternate_screen() {
    let _ = disable_raw_mode();
    window_colors::reset_on_panic();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
}

pub fn install_panic_hook(worker_panicked: fn(String)) {
    let original = std::panic::take_hook();
    let painting_thread = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == painting_thread {
            leave_the_alternate_screen();
            original(info);
            return;
        }
        worker_panicked(info.to_string());
    }));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawMode {
    Active,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Restoration {
    Pending,
    Done,
}

pub struct TerminalSession<W: Write> {
    terminal: Terminal<CrosstermBackend<W>>,
    raw_mode: RawMode,
    restoration: Restoration,
}

impl<W: Write> std::fmt::Debug for TerminalSession<W> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalSession")
            .field("raw_mode", &self.raw_mode)
            .field("restoration", &self.restoration)
            .finish_non_exhaustive()
    }
}

impl TerminalSession<Stdout> {
    pub fn enter() -> Result<Self, Error> {
        enable_raw_mode().map_err(Error::Setup)?;

        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(Error::Setup(error));
        }

        let terminal = match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                let _ = disable_raw_mode();
                return Err(Error::Setup(error));
            }
        };

        Ok(Self {
            terminal,
            raw_mode: RawMode::Active,
            restoration: Restoration::Pending,
        })
    }
}

impl<W: Write> TerminalSession<W> {
    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<W>> {
        &mut self.terminal
    }

    pub fn restore(&mut self) -> Result<(), io::Error> {
        if self.restoration == Restoration::Done {
            return Ok(());
        }

        if std::mem::replace(&mut self.raw_mode, RawMode::Disabled) == RawMode::Active {
            disable_raw_mode()?;
        }
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        self.restoration = Restoration::Done;
        Ok(())
    }
}

impl<W: Write> Drop for TerminalSession<W> {
    fn drop(&mut self) {
        let _ = self.restore();
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

    use crate::session::{RawMode, Restoration, TerminalSession};

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
            raw_mode: RawMode::Disabled,
            restoration: Restoration::Pending,
        }
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
}
