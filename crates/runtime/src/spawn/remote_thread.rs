use kernel::{cmd::RemoteCmd, domain::driver::DriverName};
use remote::{driver::RemoteDriver, job::RemoteJob, message::RemoteMessage};

use crate::{
    driver::DriverLoop,
    driver_thread::DriverThread,
    error::SpawnError,
    registry,
    spawn_setup::SpawnSetup,
};

pub(crate) fn spawn_remote(
    setup: &SpawnSetup<'_>,
) -> Result<DriverThread<RemoteCmd>, SpawnError> {
    let media_dir = setup.paths.library_dirs.media_dir();
    let reports_path = setup.paths.library_dirs.reports_path();
    let run_job = |job: RemoteJob| job.run(&remote::http::agent());
    DriverLoop::<RemoteDriver, RemoteJob> {
        row: registry::row(DriverName::Remote),
        inbox: setup.inbox.clone(),
        callback_receiver: crossbeam_channel::never(),
        message: Some(RemoteMessage::Started),
        run_job,
    }
    .spawn(move || RemoteDriver::new(media_dir, reports_path))
}
