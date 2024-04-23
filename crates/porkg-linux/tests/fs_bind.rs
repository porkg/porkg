use std::path::PathBuf;

use anyhow::{Context, Result};
use porkg_linux::{BindFlags, CloneFlags, FsSyscall, Pid, Syscall};
mod common;

#[test]
fn test_bind() -> Result<()> {
    let pid = Pid::this().as_raw();

    common::setup();
    common::as_root(
        Box::new(move || {
            let dir = PathBuf::from(format!("/tmp/tmp_mount_{pid}"));
            let file = dir.join(format!("test_{pid}"));

            std::fs::create_dir_all(&dir).context("when creating the directory")?;
            Syscall::bind("/tmp", &dir, BindFlags::RECURSIVE).context("when bind mounting")?;

            std::fs::write(file, "test file").context("when writing to bind mount")?;
            std::fs::read(format!("/tmp/test_{pid}")).context("when reading from file")?;

            Ok(())
        }),
        CloneFlags::NEWNS | CloneFlags::NEWUSER | CloneFlags::NEWPID,
    )?;

    Ok(())
}
