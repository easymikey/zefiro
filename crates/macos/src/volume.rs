#![forbid(unsafe_code)]

use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use crossbeam_channel::bounded;
use kernel::{Bounded, Percent};

const READ_TIMEOUT: Duration = Duration::from_millis(750);

fn osascript(script: &str) -> Command {
    let mut command = Command::new("osascript");
    command
        .args(["-e", script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

pub(crate) fn read_volume() -> Option<Percent> {
    let mut child = osascript("output volume of (get volume settings)")
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (reply, response) = bounded(1);
    thread::spawn(move || {
        let mut output = String::new();
        let read = stdout.read_to_string(&mut output).is_ok();
        let _ = reply.send(read.then_some(output));
    });
    let Ok(output) = response.recv_timeout(READ_TIMEOUT) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    output?.trim().parse::<u8>().ok().map(Percent::clamped)
}

pub(crate) fn write_volume(volume: Percent) {
    let _ = osascript(&format!("set volume output volume {}", volume.value())).status();
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::volume::read_volume;

    #[test]
    #[ignore = "hardware: spawns osascript against System Events and times it"]
    fn read_volume_returns_quickly_in_the_common_case() {
        let start = Instant::now();
        let volume = read_volume();
        assert!(start.elapsed() < Duration::from_millis(500));
        assert!(volume.is_some());
    }
}
