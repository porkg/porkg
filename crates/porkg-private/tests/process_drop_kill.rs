use std::{
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use anyhow::{Context as _, Result};
use nix::{
    errno::Errno,
    sys::wait::{waitpid, WaitPidFlag},
    unistd::fork,
};
use porkg_private::os::proc::ChildProcess;
use pretty_assertions::assert_eq;

mod common;

#[test]
fn test_proc_drop_kill() -> Result<()> {
    common::setup();
    match unsafe { fork() }.context("creating child process")? {
        nix::unistd::ForkResult::Parent { child } => {
            let pid = child;
            std::thread::sleep(Duration::from_secs(1));
            let child: ChildProcess = child.into();
            drop(child);

            assert_eq!(waitpid(pid, Some(WaitPidFlag::WNOHANG)), Err(Errno::ECHILD));
        }
        nix::unistd::ForkResult::Child => {
            let term = Arc::new(AtomicBool::new(false));
            signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term)).unwrap();
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    Ok(())
}
