use std::{
    fmt,
    mem,
    panic,
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, Sender};

use crate::{driver::LoopEffect, error::Error, registry::DriverRow};

pub(crate) struct Jobs<E, J, M> {
    pub(crate) split: fn(E) -> LoopEffect<E, J, M>,
    pub(crate) run: fn(J) -> M,
}

impl<E, J, M> fmt::Debug for Jobs<E, J, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Jobs").finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(crate) struct JobThread<J> {
    pub(crate) jobs: Sender<J>,
    handle: JoinHandle<()>,
}

impl<J> JobThread<J> {
    pub(crate) fn stop(self) {
        drop(self.jobs);
        if let Err(payload) = self.handle.join() {
            panic::resume_unwind(payload);
        }
    }
}

const JOB_SLOTS: usize = 4;

pub(crate) fn spawn_jobs<J, M>(
    row: &DriverRow,
    results: Sender<M>,
    run: fn(J) -> M,
) -> Result<JobThread<J>, Error>
where
    J: Ord + Send + 'static,
    M: Send + 'static,
{
    let (jobs, inbox) = crossbeam_channel::bounded(JOB_SLOTS);
    let driver = row.driver;
    let handle = thread::Builder::new()
        .name(format!("{}-jobs", row.thread_name))
        .spawn(move || serve(&inbox, &results, run))
        .map_err(|source| Error::Spawn { driver, source })?;
    Ok(JobThread { jobs, handle })
}

fn serve<J: Ord, M>(inbox: &Receiver<J>, results: &Sender<M>, run: fn(J) -> M) {
    let mut pending: Vec<J> = Vec::new();
    loop {
        if pending.is_empty() {
            match inbox.recv() {
                Ok(job) => stash(&mut pending, job),
                Err(_) => return,
            }
        }
        for job in inbox.try_iter() {
            stash(&mut pending, job);
        }
        pending.sort();
        let job = pending.remove(0);
        if results.send(run(job)).is_err() {
            return;
        }
    }
}

pub(crate) fn stash<J>(pending: &mut Vec<J>, job: J) {
    pending.retain(|held| mem::discriminant(held) != mem::discriminant(&job));
    pending.push(job);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use kernel::domain::DriverName;

    use crate::{
        jobs::{JOB_SLOTS, serve, stash},
        registry,
    };

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Job {
        First(u8),
        Second(u8),
    }

    fn label(job: Job) -> Job {
        job
    }

    #[test]
    fn a_newer_job_of_the_same_kind_replaces_the_older() {
        let mut pending = Vec::new();
        stash(&mut pending, Job::Second(1));
        stash(&mut pending, Job::First(1));
        stash(&mut pending, Job::Second(2));
        pending.sort();
        assert_eq!(pending, vec![Job::First(1), Job::Second(2)]);
    }

    #[test]
    fn the_worker_runs_the_earlier_kind_first_and_ends_when_senders_drop() {
        let (jobs, inbox) = crossbeam_channel::bounded(JOB_SLOTS);
        let (results, heard) = crossbeam_channel::unbounded();
        jobs.send(Job::Second(1)).unwrap();
        jobs.send(Job::First(1)).unwrap();
        jobs.send(Job::First(2)).unwrap();
        drop(jobs);
        serve(&inbox, &results, label);
        let ran: Vec<Job> = heard.try_iter().collect();
        assert_eq!(ran, vec![Job::First(2), Job::Second(1)]);
    }

    #[test]
    fn a_spawned_worker_answers_through_the_results_channel() {
        let (results, heard) = crossbeam_channel::unbounded();
        let thread =
            crate::jobs::spawn_jobs(registry::row(DriverName::Audio), results, label)
                .unwrap();
        thread.jobs.send(Job::First(7)).unwrap();
        let answer = heard.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(answer, Job::First(7));
        thread.stop();
    }
}
