use anyhow::{bail, Context, Result};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use porkg_linux::__itest::{CloneFlags, CloneSyscall as _, Syscall};

mod common;

#[test]
fn test_clone_err() -> Result<()> {
    common::setup();

    let pid = Syscall::clone(Box::new(|| -1), CloneFlags::empty())?;
    match waitpid(pid, Some(WaitPidFlag::__WALL))
        .with_context(|| format!("failed to wait for {pid:?}"))?
    {
        WaitStatus::Exited(p, status) => {
            assert_eq!(pid, p);
            assert_eq!(status, 255);
            Ok(())
        }
        status => bail!("unexpected status {status:?}"),
    }
}
