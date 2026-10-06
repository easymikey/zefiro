use std::time::Instant;

use kernel::message::Message;

use crate::{
    error::Error,
    event_loop::EventLoop,
    repaint::{FRAME_INTERVAL, Repaint, RepaintCause},
    shell::{FrameDue, Shell},
};

impl<'a, S: Shell> EventLoop<'a, S>
where
    S::Error: std::error::Error + 'static,
{
    pub(crate) fn frame_deadline(&self, now: Instant) -> Instant {
        self.last_paint_at.map_or(now, |last| last + FRAME_INTERVAL)
    }

    pub(crate) fn deadline(
        &self,
        now: Instant,
        frame_due: FrameDue,
    ) -> Option<Instant> {
        let frame = match frame_due {
            FrameDue::At(at) => Some(self.runtime.instant_of(at)),
            FrameDue::Settled => None,
        };
        let immediate = match self.repaint {
            Repaint::Settled => None,
            Repaint::Now => Some(now),
            Repaint::NextFrame => Some(self.frame_deadline(now)),
        };
        [self.runtime.timers.next_deadline(), frame, immediate]
            .into_iter()
            .flatten()
            .min()
    }

    pub(crate) fn paint_if_due(
        &mut self,
        now: Instant,
        frame_due: FrameDue,
    ) -> Result<(), Error<S::Error>> {
        let frame_passed =
            matches!(frame_due, FrameDue::At(at) if self.runtime.instant_of(at) <= now);
        let is_due = match self.repaint {
            Repaint::Now => true,
            Repaint::NextFrame => frame_passed || self.frame_deadline(now) <= now,
            Repaint::Settled => frame_passed,
        };
        if !is_due {
            return Ok(());
        }
        let painted = self
            .shell
            .paint(self.runtime.frame(now))
            .map_err(Error::Paint)?;
        self.repaint = Repaint::Settled;
        self.last_paint_at = Some(now);
        let viewport_message = Message::Viewport {
            visible_rows: painted.visible_rows,
            cover_side: painted.cover_side,
        };
        self.step_and_repaint(viewport_message, RepaintCause::Event);
        for error in painted.errors {
            self.step_and_repaint(Message::Paint(error), RepaintCause::Event);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crossbeam_channel::{bounded, unbounded};
    use kernel::{
        domain::{config::Diagnostic, time::Moment},
        message::PaintError,
    };
    use rstest::rstest;

    use crate::{
        event_loop::{
            EventLoop,
            tests::{Scripted, fixture},
        },
        repaint::Repaint,
        shell::FrameDue,
    };

    #[test]
    fn a_moment_deadline_wakes_the_loop_at_its_instant() {
        let mut fixture = fixture();
        let (keys, input) = unbounded();
        let mut shell_scripted = Scripted::new(keys, usize::MAX);
        let now = Instant::now();
        let frame = fixture.runtime.frame(now);
        let moment = Moment::new(frame.now.since_epoch() + Duration::from_millis(10));
        let mut event_loop =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input);
        event_loop.repaint = Repaint::Settled;

        let deadline = event_loop.deadline(now, FrameDue::At(moment));

        assert_eq!(deadline, Some(event_loop.runtime.instant_of(moment)));
        fixture.runtime.drain();
    }

    #[test]
    fn painted_errors_are_stepped_in_the_batch() {
        let mut fixture = fixture();
        let (keys, input) = bounded(1);
        let mut shell_scripted = Scripted::new(keys, 1);
        shell_scripted.pending_errors = vec![PaintError::Query(
            Diagnostic::from_error(&std::io::Error::other("no answer")),
        )];

        let ended =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input).drive();

        assert!(matches!(ended, Ok(())));
        let toast = fixture.runtime.model.workspace.toasts.first().unwrap();
        assert_eq!(toast.title, "Terminal probe failed");
    }

    #[rstest]
    #[case::a_fact_batch_waits_for_the_frame(Repaint::NextFrame, 0)]
    #[case::a_key_batch_paints_at_once(Repaint::Now, 1)]
    fn paint_if_due_respects_the_frame_cap(
        #[case] repaint: Repaint,
        #[case] painted: usize,
    ) {
        let mut fixture = fixture();
        let (keys, input) = unbounded();
        let mut shell_scripted = Scripted::new(keys, usize::MAX);
        let mut event_loop =
            EventLoop::new(&mut fixture.runtime, &mut shell_scripted, &input);
        let now = Instant::now();
        event_loop.last_paint_at = Some(now - Duration::from_millis(5));
        event_loop.repaint = repaint;

        event_loop.paint_if_due(now, FrameDue::Settled).unwrap();

        assert_eq!(shell_scripted.toasts.len(), painted);
        fixture.runtime.drain();
    }
}
