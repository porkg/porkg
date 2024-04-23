use std::path::PathBuf;

use anyhow::{Context, Result};
use porkg_linux::{CloneFlags, FsSyscall, Pid, Syscall};
mod common;

#[test]
fn test_bind() -> Result<()> {
    let pid = Pid::this().as_raw();

    common::setup();
    common::as_root(
        Box::new(move || {
            let dir = PathBuf::from(format!("/tmp/tmp_mount_{pid}"));
            let file = format!("test_{pid}");

            std::fs::create_dir_all(&dir).context("when creating the directory")?;
            std::fs::write(dir.join(file), "test file").context("when writing to test file")?;

            Syscall::pivot(dir).context("when pivoting to new root")?;

            std::fs::read(format!("/test_{pid}")).context("when reading from file")?;

            Ok(())
        }),
        CloneFlags::NEWNS | CloneFlags::NEWUSER | CloneFlags::NEWPID,
    )?;

    Ok(())
}
