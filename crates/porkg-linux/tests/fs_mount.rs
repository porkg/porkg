use std::path::PathBuf;

use anyhow::{Context, Result};
use porkg_linux::__itest::{
    CloneFlags, FsSyscall, MountFlags, MountKind, Pid, Syscall, UnmountFlags, NO_PATH,
};
mod common;

#[test]
fn test_mount_proc() -> Result<()> {
    let pid = Pid::this().as_raw();

    common::setup();
    common::as_root(
        Box::new(move || {
            let dir = PathBuf::from(format!("/tmp/tmp_mount_{pid}"));
            std::fs::create_dir_all(&dir).context("when creating the directory")?;
            Syscall::mount(
                NO_PATH,
                &dir,
                Some(MountKind::Proc),
                MountFlags::empty(),
                NO_PATH,
            )
            .context("when mounting")?;

            std::fs::read(dir.join("stat")).context("when reading <tmp>/stat")?;

            Syscall::unmount(&dir, UnmountFlags::empty()).context("when unmounting")?;

            Ok(())
        }),
        CloneFlags::NEWNS | CloneFlags::NEWUSER | CloneFlags::NEWPID,
    )?;

    Ok(())
}
