use std::{
    any::Any,
    collections::{HashMap, hash_map::Entry},
    mem,
    panic::resume_unwind,
};

use crossbeam_channel::{Sender, TrySendError};
use kernel::{
    message::Message,
    update::machine::{Driver, LoopEffect},
};

use crate::{
    driver_thread::Congestion,
    error::SpawnError,
    jobs::{spawn_jobs, stash},
    registry::DriverRow,
    timers::Timers,
    watcher::FileStream,
};

pub(crate) struct Outlets<'a, D: Driver, J> {
    pub(crate) inbox: &'a Sender<Message>,
    pub(crate) congestion: &'a Congestion,
    pub(crate) row: &'static DriverRow,
    pub(crate) run_job: fn(J) -> D::Message,
    pub(crate) result_sender: Sender<Result<D::Message, Box<dyn Any + Send>>>,
    pub(crate) workers: HashMap<mem::Discriminant<J>, Sender<J>>,
    pub(crate) pending: Vec<J>,
    pub(crate) timers: Timers<D::Message>,
    pub(crate) file_stream: FileStream<D::Message>,
}

impl<D, J> Outlets<'_, D, J>
where
    D: Driver,
    D::Message: Send + 'static,
    J: Send + 'static,
{
    pub(crate) fn hand_over(&mut self) -> Result<(), SpawnError> {
        for job in mem::take(&mut self.pending) {
            let worker = match self.workers.entry(mem::discriminant(&job)) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(spawn_jobs(
                    self.row,
                    self.result_sender.clone(),
                    self.run_job,
                )?),
            };
            match worker.try_send(job) {
                Ok(()) => {}
                Err(TrySendError::Full(job)) => self.pending.push(job),
                Err(TrySendError::Disconnected(_job)) => {
                    resume_unwind(Box::new("a job worker died"))
                }
            }
        }
        Ok(())
    }

    pub(crate) fn place(
        &mut self,
        loop_effect: LoopEffect<<D as Driver>::Effect, J, D::Message>,
        driver: &mut D,
    ) -> Option<D::Message> {
        match loop_effect {
            LoopEffect::Execute(effect) => driver.execute(effect),
            LoopEffect::Run(job) => {
                stash(&mut self.pending, job);
                None
            }
            LoopEffect::After { delay, message } => {
                self.timers.after(delay, message);
                None
            }
            LoopEffect::Watch { path, changed } => {
                self.file_stream.watch(&path, changed)
            }
            LoopEffect::Unwatch(path) => self.file_stream.unwatch(&path),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::Mutex,
        thread,
        time::Instant,
    };

    use crossbeam_channel::unbounded;
    use kernel::domain::driver::DriverName;

    use crate::{
        driver::tests::{LONG_JOB, Nap, Napper, NapperMessage, RECV_TIMEOUT},
        driver_thread::Congestion,
        jobs::stash,
        outlets::Outlets,
        registry,
        timers::Timers,
        watcher::FileStream,
    };

    #[test]
    fn a_hand_over_to_a_dead_worker_makes_the_driver_died() {
        let (inbox, _reports) = unbounded();
        let congestion = Congestion::default();
        let (result_sender, _result_receiver) = unbounded();
        let mut outlets = Outlets::<Napper, Nap> {
            inbox: &inbox,
            congestion: &congestion,
            row: registry::row(DriverName::Config),
            run_job: |_nap: Nap| -> NapperMessage { panic!("the job panics") },
            result_sender,
            workers: HashMap::new(),
            pending: Vec::new(),
            timers: Timers::default(),
            file_stream: FileStream::Idle,
        };
        let deadline = Instant::now() + RECV_TIMEOUT;

        let died = std::iter::repeat_with(|| {
            stash(&mut outlets.pending, Nap::Long);
            catch_unwind(AssertUnwindSafe(|| outlets.hand_over())).is_err()
        })
        .take_while(|_| Instant::now() < deadline)
        .any(|died| died);

        assert!(died);
    }

    #[test]
    fn a_job_of_one_kind_does_not_wait_behind_a_running_job_of_another() {
        let (inbox, _reports) = unbounded();
        let congestion = Congestion::default();
        let (result_sender, result_receiver) = unbounded();
        let mut outlets = Outlets::<Napper, Nap> {
            inbox: &inbox,
            congestion: &congestion,
            row: registry::row(DriverName::Config),
            run_job: |nap: Nap| {
                if nap == Nap::Long {
                    thread::sleep(LONG_JOB);
                }
                NapperMessage::Woke(nap)
            },
            result_sender,
            workers: HashMap::new(),
            pending: Vec::new(),
            timers: Timers::default(),
            file_stream: FileStream::Idle,
        };
        stash(&mut outlets.pending, Nap::Long);
        stash(&mut outlets.pending, Nap::Brief);

        outlets.hand_over().unwrap();

        let Ok(NapperMessage::Woke(first)) =
            result_receiver.recv_timeout(RECV_TIMEOUT).unwrap()
        else {
            panic!("a job answers with Woke");
        };
        assert_eq!(first, Nap::Brief);
        assert_eq!(outlets.workers.len(), 2);
    }

    #[test]
    fn a_job_behind_a_full_worker_queue_waits_in_pending_and_goes_on_the_next_hand_over()
     {
        static NAP_LOCK: Mutex<()> = Mutex::new(());
        let (inbox, _reports) = unbounded();
        let congestion = Congestion::default();
        let (result_sender, _result_receiver) = unbounded();
        let mut outlets = Outlets::<Napper, Nap> {
            inbox: &inbox,
            congestion: &congestion,
            row: registry::row(DriverName::Config),
            run_job: |nap: Nap| {
                drop(NAP_LOCK.lock());
                NapperMessage::Woke(nap)
            },
            result_sender,
            workers: HashMap::new(),
            pending: Vec::new(),
            timers: Timers::default(),
            file_stream: FileStream::Idle,
        };
        let asleep = NAP_LOCK.lock().unwrap();
        let deadline = Instant::now() + RECV_TIMEOUT;

        let waits = std::iter::repeat_with(|| {
            stash(&mut outlets.pending, Nap::Long);
            outlets.hand_over().unwrap();
            !outlets.pending.is_empty()
        })
        .take_while(|_| Instant::now() < deadline)
        .any(|waits| waits);
        stash(&mut outlets.pending, Nap::Long);

        assert!(waits);
        assert_eq!(outlets.pending, [Nap::Long]);
        drop(asleep);
        let delivered = std::iter::repeat_with(|| {
            outlets.hand_over().unwrap();
            outlets.pending.is_empty()
        })
        .take_while(|_| Instant::now() < deadline + RECV_TIMEOUT)
        .any(|delivered| delivered);
        assert!(delivered);
    }
}
