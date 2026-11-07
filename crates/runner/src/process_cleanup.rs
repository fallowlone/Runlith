use std::io;
use std::process::{Child, ExitStatus};
use std::time::Duration;

#[cfg(any(target_os = "macos", target_os = "linux"))]
use rustix::process::{kill_process_group, waitid, Pid, Signal, WaitId, WaitIdOptions};

/// Polls an owned group leader, terminating descendants before reaping it.
pub fn try_wait_process_tree(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        match observe(child) {
            Ok(None) => return Ok(None),
            Ok(Some(_)) => kill_process_tree(child),
            Err(rustix::io::Errno::CHILD) => {}
            Err(error) => return Err(error.into()),
        }
    }
    child.try_wait()
}

/// Terminates and reaps a child spawned in its own process group.
pub fn kill_process_tree(child: &mut Child) {
    terminate_process_tree(child, Duration::ZERO);
}

/// Gives the owned process group a bounded TERM grace before KILL and reap.
pub fn terminate_process_tree(child: &mut Child, grace: Duration) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        if observe(child).is_err() {
            let _ = child.try_wait();
            return;
        }
        let group = Pid::from_child(child);
        if !grace.is_zero() {
            let _ = kill_process_group(group, Signal::TERM);
            let deadline = std::time::Instant::now() + grace;
            while matches!(observe(child), Ok(None)) {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                std::thread::sleep(remaining.min(Duration::from_millis(10)));
            }
        }
        // NOWAIT keeps the leader's PID reserved until every group signal is sent.
        let _ = kill_process_group(group, Signal::KILL);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = grace;
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn observe(child: &Child) -> rustix::io::Result<Option<rustix::process::WaitIdStatus>> {
    loop {
        match waitid(
            WaitId::Pid(Pid::from_child(child)),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        ) {
            Err(rustix::io::Errno::INTR) => continue,
            result => return result,
        }
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    #[test]
    fn exit_observation_retains_pid_until_descendants_are_terminated() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 30 & exit 23"])
            .process_group(0)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while observe(&child).unwrap().is_none() {
            if std::time::Instant::now() >= deadline {
                kill_process_tree(&mut child);
                panic!("fixture leader did not exit");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            observe(&child).unwrap().is_some(),
            "observation must not reap"
        );
        let status = try_wait_process_tree(&mut child).unwrap().unwrap();
        assert_eq!(status.code(), Some(23));
        assert_eq!(observe(&child).unwrap_err(), rustix::io::Errno::CHILD);
        assert_eq!(try_wait_process_tree(&mut child).unwrap(), Some(status));
        kill_process_tree(&mut child);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            tx.send(stdout.read_to_end(&mut bytes)).unwrap();
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap(), 0);
    }
}
