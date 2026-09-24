use crossbeam_channel::unbounded;
use kernel::AudioCmd;
use runtime::{Runtime, run};

use crate::support::{QuitShell, boot_paths, recording_hardware, stock_startup};

#[test]
fn boot_sends_the_startup_stop_and_list_devices_to_the_stub_audio_inbox() {
    let directory = tempfile::tempdir().unwrap();
    let startup = stock_startup();
    let (hardware, commands) = recording_hardware();
    let runtime =
        Runtime::boot(startup, boot_paths(directory.path()), hardware).unwrap();
    let (keys, input) = unbounded();
    keys.send(()).unwrap();
    let mut shell = QuitShell;

    let ended = run(runtime, &mut shell, &input);

    assert!(matches!(ended, Ok(())));
    assert_eq!(commands.recv().unwrap(), AudioCmd::Stop);
    assert_eq!(commands.recv().unwrap(), AudioCmd::ListDevices);
}
