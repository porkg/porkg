use std::path::PathBuf;

use anyhow::{Context, Result};
use porkg_linux::__itest::{BindFlags, CloneFlags, FsSyscall, Pid, Syscall};
mod common;

#[test]
fn test_bind_ro() -> Result<()> {
    let pid = Pid::this().as_raw();

    common::setup();
    common::as_root(
        Box::new(move || {
            let dir = PathBuf::from(format!("/tmp/tmp_mount_{pid}"));
            let file = dir.join(format!("test_{pid}"));

            std::fs::create_dir_all(&dir).context("when creating the directory")?;
            Syscall::bind("/tmp", &dir, BindFlags::RECURSIVE | BindFlags::READ_ONLY)
                .context("when read-only bind mounting")?;

            std::fs::write(file, "test file").expect_err("should be read only");

            Ok(())
        }),
        CloneFlags::NEWNS | CloneFlags::NEWUSER | CloneFlags::NEWPID,
    )?;

    Ok(())
}
