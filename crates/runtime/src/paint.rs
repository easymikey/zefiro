use std::time::Instant;

use kernel::Message;

use crate::{
    error::Error,
    event_loop::EventLoop,
    repaint::{FRAME_INTERVAL, Repaint, Source},
    shell::{FrameDue, Shell},
};

impl<'a, S: Shell> EventLoop<'a, S>
where
    S::Error: std::error::Error + 'static,
{
    pub(crate) fn frame_deadline(&self, now: Instant) -> Instant {
        self.last_paint.map_or(now, |last| last + FRAME_INTERVAL)
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
        let should_paint = match self.repaint {
            Repaint::Now => true,
            Repaint::NextFrame => frame_passed || self.frame_deadline(now) <= now,
            Repaint::Settled => frame_passed,
        };
        if !should_paint {
            return Ok(());
        }
        let painted = self
            .shell
            .paint(self.runtime.frame(now))
            .map_err(Error::Paint)?;
        self.repaint = Repaint::Settled;
        self.last_paint = Some(now);
        if let Some(request) = painted.cover {
            self.runtime.request_cover(request);
        }
        if let Some(visible_rows) = painted.visible_rows {
            self.step_and_repaint(Message::Viewport { visible_rows }, Source::Event);
        }
        for message in painted.toasts {
            self.step_and_repaint(message, Source::Event);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crossbeam_channel::{bounded, unbounded};
    use kernel::{Message, Moment, domain::Toast};
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
        let mut shell = Scripted::new(keys, usize::MAX);
        let now = Instant::now();
        let frame = fixture.runtime.frame(now);
        let moment = Moment::new(frame.now.since_epoch() + Duration::from_millis(10));
        let mut event_loop = EventLoop::new(&mut fixture.runtime, &mut shell, &input);
        event_loop.repaint = Repaint::Settled;

        let deadline = event_loop.deadline(now, FrameDue::At(moment));

        assert_eq!(deadline, Some(event_loop.runtime.instant_of(moment)));
        fixture.runtime.drain();
    }

    #[test]
    fn painted_toasts_are_stepped_in_the_batch() {
        let mut fixture = fixture();
        let (keys, input) = bounded(1);
        let mut shell = Scripted::new(keys, 1);
        shell.pending_failures = vec![Message::Toast(Toast::error("fail".to_owned()))];

        let ended = EventLoop::new(&mut fixture.runtime, &mut shell, &input).drive();

        assert!(matches!(ended, Ok(())));
        let toast = fixture.runtime.model.workspace.toast.as_ref().unwrap();
        assert_eq!(toast.text, "fail");
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
        let mut shell = Scripted::new(keys, usize::MAX);
        let mut event_loop = EventLoop::new(&mut fixture.runtime, &mut shell, &input);
        let now = Instant::now();
        event_loop.last_paint = Some(now - Duration::from_millis(5));
        event_loop.repaint = repaint;

        event_loop.paint_if_due(now, FrameDue::Settled).unwrap();

        assert_eq!(shell.toasts.len(), painted);
        fixture.runtime.drain();
    }
}
