use std::{
    any::Any,
    fmt,
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
    thread,
};

use crossbeam_channel::{Receiver, Sender};

use crate::{error::Error, registry::DriverRow};

pub(crate) struct Jobs<J, M> {
    pub(crate) run: fn(J) -> M,
}

impl<J, M> fmt::Debug for Jobs<J, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Jobs").finish_non_exhaustive()
    }
}

const JOB_SLOTS: usize = 4;

pub(crate) fn spawn_jobs<J, M>(
    row: &DriverRow,
    results: Sender<Result<M, Box<dyn Any + Send>>>,
    run: fn(J) -> M,
) -> Result<Sender<J>, Error>
where
    J: Send + 'static,
    M: Send + 'static,
{
    let (jobs, inbox) = crossbeam_channel::bounded(JOB_SLOTS);
    let driver = row.driver;
    thread::Builder::new()
        .name(format!("{}-jobs", row.thread_name))
        .spawn(move || serve(&inbox, &results, run))
        .map_err(|source| Error::Spawn { driver, source })?;
    Ok(jobs)
}

fn serve<J, M>(
    inbox: &Receiver<J>,
    results: &Sender<Result<M, Box<dyn Any + Send>>>,
    run: fn(J) -> M,
) {
    while let Ok(first) = inbox.recv() {
        let job = inbox.try_iter().last().unwrap_or(first);
        let answer = catch_unwind(AssertUnwindSafe(|| run(job)));
        let panicked = answer.is_err();
        if results.send(answer).is_err() || panicked {
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

    use kernel::domain::driver::DriverName;

    use crate::{
        jobs::{JOB_SLOTS, serve, stash},
        registry,
    };

    #[derive(Debug, PartialEq, Eq)]
    enum Job {
        First(u8),
        Second(u8),
    }

    fn label(job: Job) -> Job {
        job
    }

    fn explode(job: Job) -> Job {
        match job {
            Job::First(_) => panic!("the job panics"),
            Job::Second(_) => job,
        }
    }

    #[test]
    fn a_newer_job_of_the_same_kind_replaces_the_older() {
        let mut pending = Vec::new();
        stash(&mut pending, Job::Second(1));
        stash(&mut pending, Job::First(1));
        stash(&mut pending, Job::Second(2));
        assert_eq!(pending, vec![Job::First(1), Job::Second(2)]);
    }

    #[test]
    fn the_worker_runs_the_newest_queued_job_and_ends_when_senders_drop() {
        let (jobs, inbox) = crossbeam_channel::bounded(JOB_SLOTS);
        let (results, heard) = crossbeam_channel::unbounded();
        jobs.send(Job::First(1)).unwrap();
        jobs.send(Job::First(2)).unwrap();
        drop(jobs);
        serve(&inbox, &results, label);
        let ran: Vec<Job> = heard.try_iter().map(Result::unwrap).collect();
        assert_eq!(ran, vec![Job::First(2)]);
    }

    #[test]
    fn a_spawned_worker_answers_through_the_results_channel() {
        let (results, heard) = crossbeam_channel::unbounded();
        let thread =
            crate::jobs::spawn_jobs(registry::row(DriverName::Audio), results, label)
                .unwrap();
        thread.send(Job::First(7)).unwrap();
        let answer = heard.recv_timeout(Duration::from_secs(2)).unwrap().unwrap();
        assert_eq!(answer, Job::First(7));
        drop(thread);
    }

    #[test]
    fn a_panicking_job_is_reported_and_ends_its_worker() {
        let (jobs, inbox) = crossbeam_channel::bounded(JOB_SLOTS);
        let (results, heard) = crossbeam_channel::unbounded();
        jobs.send(Job::First(1)).unwrap();
        serve(&inbox, &results, explode);
        let answers: Vec<_> = heard.try_iter().collect();
        assert_eq!(answers.len(), 1);
        assert!(answers[0].is_err());
    }

    #[test]
    fn a_panicking_job_leaves_another_worker_answering() {
        let (results, heard) = crossbeam_channel::unbounded();
        let (spare, spare_heard) = crossbeam_channel::unbounded();
        let doomed =
            crate::jobs::spawn_jobs(registry::row(DriverName::Audio), results, explode)
                .unwrap();
        let other =
            crate::jobs::spawn_jobs(registry::row(DriverName::Config), spare, explode)
                .unwrap();
        doomed.send(Job::First(1)).unwrap();
        other.send(Job::Second(2)).unwrap();
        assert!(heard.recv_timeout(Duration::from_secs(2)).unwrap().is_err());
        let answer = spare_heard
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(answer, Job::Second(2));
    }
}
