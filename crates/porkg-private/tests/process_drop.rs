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
fn test_proc_drop() -> Result<()> {
    common::setup();
    match unsafe { fork() }.context("creating child process")? {
        nix::unistd::ForkResult::Parent { child } => {
            let pid = child;
            let child: ChildProcess = child.into();
            drop(child);

            assert_eq!(waitpid(pid, Some(WaitPidFlag::WNOHANG)), Err(Errno::ECHILD));
        }
        nix::unistd::ForkResult::Child => std::thread::park(),
    }

    Ok(())
}
