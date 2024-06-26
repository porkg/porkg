mod clone;
mod fs;
mod proc;
pub mod sandbox;

use private::{Syscall, NO_PATH};

pub mod private {
    use std::path::Path;

    pub struct Syscall;
    pub const NO_PATH: Option<&Path> = None::<&Path>;
}

#[cfg(test)]
mod test {
    use std::{
        io::{ErrorKind, Read as _, Write as _},
        os::unix::net::UnixStream,
    };

    use anyhow::{bail, Context as _};
    use nix::{
        sys::wait::{waitpid, WaitPidFlag, WaitStatus},
        unistd::{setresgid, setresuid, Gid, Uid},
    };

    use crate::{
        clone::{CloneFlags, CloneSyscall as _},
        private::Syscall,
    };

    pub fn as_root<F: 'static + FnMut() -> anyhow::Result<()>>(
        mut callback: F,
        flags: CloneFlags,
    ) -> anyhow::Result<()> {
        let my_uid = Uid::current().as_raw();
        let my_gid = Gid::current().as_raw();
        let (mut outer, mut inner) = UnixStream::pair().context("when creating socket")?;

        let pid = Syscall::clone(
            Box::new(move || {
                std::fs::write("/proc/self/uid_map", format!("0 {my_uid} 1"))
                    .context("when mapping the uid")?;
                std::fs::write("/proc/self/setgroups", "deny").context("when denying setgroups")?;
                std::fs::write("/proc/self/gid_map", format!("0 {my_gid} 1"))
                    .context("when mapping the gid")?;
                setresuid(Uid::from_raw(0), Uid::from_raw(0), Uid::from_raw(0))
                    .context("when switching to the root user")?;
                setresgid(Gid::from_raw(0), Gid::from_raw(0), Gid::from_raw(0))
                    .context("when switching to the root group")?;
                let result = callback();
                outer.write_all(&[0]).ok();
                result
            }),
            flags,
        )?;

        match waitpid(pid, Some(WaitPidFlag::__WALL))
            .with_context(|| format!("failed to wait for test implementation {pid:?}"))?
        {
            WaitStatus::Exited(_, status) => {
                assert_eq!(status, 0, "the child process tests passed");
            }
            other => bail!("unexpected wait result {other:?}"),
        }

        let mut inner_read = [0u8; 1];
        inner.set_nonblocking(true)?;

        match inner.read(&mut inner_read) {
            Ok(1) => Ok(()),
            Err(e) if e.kind() != ErrorKind::WouldBlock => Err(e)?,
            _ => bail!("child process paniced"),
        }
    }
}
