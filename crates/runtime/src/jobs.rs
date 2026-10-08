use std::{
    any::Any,
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
    thread,
};

use crossbeam_channel::{Receiver, Sender};

use crate::{error::SpawnError, registry::DriverRow};

const JOB_SLOTS: usize = 4;

pub(crate) fn spawn_jobs<J, M>(
    row: &DriverRow,
    result_sender: Sender<Result<M, Box<dyn Any + Send>>>,
    run: fn(J) -> M,
) -> Result<Sender<J>, SpawnError>
where
    J: Send + 'static,
    M: Send + 'static,
{
    let (job_sender, job_receiver) = crossbeam_channel::bounded(JOB_SLOTS);
    let driver = row.driver_name;
    thread::Builder::new()
        .name(format!("{}-jobs", row.thread_name))
        .spawn(move || serve(&job_receiver, &result_sender, run))
        .map_err(|error| SpawnError::Thread {
            driver_name: driver,
            error,
        })?;
    Ok(job_sender)
}

fn serve<J, M>(
    job_receiver: &Receiver<J>,
    result_sender: &Sender<Result<M, Box<dyn Any + Send>>>,
    run: fn(J) -> M,
) {
    while let Ok(first) = job_receiver.recv() {
        let job = job_receiver.try_iter().last().unwrap_or(first);
        let answer = catch_unwind(AssertUnwindSafe(|| run(job)));
        let panicked = answer.is_err();
        if result_sender.send(answer).is_err() || panicked {
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
        jobs::{JOB_SLOTS, serve},
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
    fn the_worker_runs_the_newest_queued_job_and_ends_when_senders_drop() {
        let (job_sender, job_receiver) = crossbeam_channel::bounded(JOB_SLOTS);
        let (result_sender, result_receiver) = crossbeam_channel::unbounded();
        job_sender.send(Job::First(1)).unwrap();
        job_sender.send(Job::First(2)).unwrap();
        drop(job_sender);
        serve(&job_receiver, &result_sender, label);
        let ran_jobs: Vec<Job> =
            result_receiver.try_iter().map(Result::unwrap).collect();
        assert_eq!(ran_jobs, vec![Job::First(2)]);
    }

    #[test]
    fn a_spawned_worker_answers_through_the_results_channel() {
        let (result_sender, result_receiver) = crossbeam_channel::unbounded();
        let thread = crate::jobs::spawn_jobs(
            registry::row(DriverName::Audio),
            result_sender,
            label,
        )
        .unwrap();
        thread.send(Job::First(7)).unwrap();
        let answer = result_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(answer, Job::First(7));
        drop(thread);
    }

    #[test]
    fn a_panicking_job_is_reported_and_ends_its_worker() {
        let (job_sender, job_receiver) = crossbeam_channel::bounded(JOB_SLOTS);
        let (result_sender, result_receiver) = crossbeam_channel::unbounded();
        job_sender.send(Job::First(1)).unwrap();
        serve(&job_receiver, &result_sender, explode);
        let answers: Vec<_> = result_receiver.try_iter().collect();
        assert_eq!(answers.len(), 1);
        assert!(answers[0].is_err());
    }

    #[test]
    fn a_panicking_job_leaves_another_worker_answering() {
        let (result_sender, result_receiver) = crossbeam_channel::unbounded();
        let (spare_result_sender, spare_result_receiver) =
            crossbeam_channel::unbounded();
        let doomed = crate::jobs::spawn_jobs(
            registry::row(DriverName::Audio),
            result_sender,
            explode,
        )
        .unwrap();
        let other = crate::jobs::spawn_jobs(
            registry::row(DriverName::Config),
            spare_result_sender,
            explode,
        )
        .unwrap();
        doomed.send(Job::First(1)).unwrap();
        other.send(Job::Second(2)).unwrap();
        assert!(
            result_receiver
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .is_err()
        );
        let answer = spare_result_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(answer, Job::Second(2));
    }
}
