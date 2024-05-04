use std::{
    io::{Read as _, Write as _},
    os::unix::net::UnixStream,
};

use anyhow::{bail, Context, Result};
use nix::{
    sys::wait::{waitpid, WaitPidFlag, WaitStatus},
    unistd::ForkResult,
};
use porkg_linux::private::{CloneFlags, CloneSyscall as _, Pid, Syscall};

mod common;

#[test]
fn test_clone_parent() -> Result<()> {
    common::setup();

    // The `container_clone_sibling` will create a sibling process (share
    // the same parent) of the calling process. In Unix, a process can only
    // wait on the immediate children process and can't wait on the sibling
    // process. Therefore, to test the logic, we will have to fork a process
    // first and then let the forked process call `container_clone_sibling`.
    // Then the testing process (the process where test is called), who are
    // the parent to this forked process and the sibling process cloned by
    // the `container_clone_sibling`, can wait on both processes.

    // We need to use a channel so that the forked process can pass the pid
    // of the sibling process to the testing process.
    let (mut child_socket, mut server_socket) = UnixStream::pair()?;

    match unsafe { nix::unistd::fork() }? {
        ForkResult::Parent { child } => {
            let mut sibling_process_pid = [0u8; std::mem::size_of::<i32>()];
            server_socket.read_exact(&mut sibling_process_pid)?;
            let sibling_process_pid = i32::from_ne_bytes(sibling_process_pid);
            let sibling_process_pid = Pid::from_raw(sibling_process_pid);
            match waitpid(sibling_process_pid, Some(WaitPidFlag::__WALL))
                .with_context(|| format!("failed to wait for sibling {sibling_process_pid:?}"))?
            {
                WaitStatus::Exited(p, status) => {
                    assert_eq!(sibling_process_pid, p);
                    assert_eq!(status, 0);
                }
                status => bail!("unexpected status from sibling {status:?}"),
            }
            // After sibling process exits, we can wait on the forked process.
            match waitpid(child, Some(WaitPidFlag::__WALL))
                .with_context(|| format!("failed to wait for child {child:?}"))?
            {
                WaitStatus::Exited(p, status) => {
                    assert_eq!(child, p);
                    assert_eq!(status, 0);
                }
                status => bail!("unexpected status from child {status:?}"),
            }
        }
        ForkResult::Child => {
            // Inside the forked process. We call `container_clone` and pass
            // the pid to the parent process.
            let pid = Syscall::clone(Box::new(|| 0), CloneFlags::PARENT)?;
            let pid = pid.as_raw().to_ne_bytes();
            child_socket.write_all(&pid)?;
            std::process::exit(0);
        }
    };

    Ok(())
}
