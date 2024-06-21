use std::{
    io::{Read as _, Write as _},
    os::{
        fd::{AsRawFd as _, FromRawFd as _},
        unix::net::UnixStream,
    },
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result};
use porkg_linux::__test::{CloneFlags, CloneSyscall, Pid, ProcSyscall, Syscall};
mod common;

#[test]
fn test_clone_fd() -> Result<()> {
    let (mut par, child) = UnixStream::pair()?;

    common::setup();
    let child_pid = Syscall::clone(
        move || {
            let mut child = child.try_clone()?;
            let mut tmp = [0u8; 1];
            let (par2, mut child2) = UnixStream::pair()?;
            child.write_all(&par2.as_raw_fd().to_ne_bytes())?;
            child.read_exact(&mut tmp)?;
            drop(par2);
            child2.write_all(b"1")?;
            Ok::<_, anyhow::Error>(())
        },
        CloneFlags::empty(),
    )?;

    let mut fd_buf = [0u8; 4];
    par.read_exact(&mut fd_buf)
        .context("getting FD from child")?;
    let child2 = Syscall::clone_fd(child_pid, i32::from_ne_bytes(fd_buf))?;
    par.write_all(b"1").context("allowing child to proceed")?;

    let mut read_buf = [0u8; 1];
    let mut child2 = UnixStream::from(child2);
    child2
        .read_exact(&mut read_buf)
        .context("reading test data from child")?;

    assert_eq!(&read_buf, b"1");

    Ok(())
}
