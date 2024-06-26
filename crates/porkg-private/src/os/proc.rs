use std::{
    cell::RefCell,
    ops::Add,
    sync::atomic::AtomicU64,
    time::{Duration, Instant},
};

use nix::{
    sys::{
        signal::Signal,
        wait::{waitpid, WaitPidFlag},
    },
    unistd::Pid,
};

/// A value that can be converted into an exit code.
pub trait IntoExitCode {
    /// Converts the current value into an exit code.
    fn report(&self) -> i32;
}

impl<T, E: IntoExitCode> IntoExitCode for Result<T, E> {
    fn report(&self) -> i32 {
        match self {
            Ok(_) => 0,
            Err(v) => v.report(),
        }
    }
}

impl IntoExitCode for anyhow::Error {
    fn report(&self) -> i32 {
        tracing::error!(?self, "process failed");
        -1
    }
}

impl IntoExitCode for i32 {
    fn report(&self) -> i32 {
        *self
    }
}

static CHILD_DROP_WAIT_MILLIS: AtomicU64 = AtomicU64::new(4500);
static CHILD_KILL_WAIT_MILLIS: AtomicU64 = AtomicU64::new(500);

/// Kills a child process (first with SIGINT, then with SIGKILL if it takes more than 5 seconds) when this value is
/// dropped.
pub struct ChildProcess(RefCell<Option<Pid>>);

impl std::fmt::Debug for ChildProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.borrow().fmt(f)
    }
}

impl From<Pid> for ChildProcess {
    fn from(value: Pid) -> Self {
        Self::new(value)
    }
}

impl From<i32> for ChildProcess {
    fn from(value: i32) -> Self {
        Self::new(Pid::from_raw(value))
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        if let Err(error) = self.try_drop_impl() {
            tracing::warn!(?error, "failed to drop child process");
        }
    }
}

impl ChildProcess {
    /// Forgets the child process and returns the pid.
    pub fn forget(self) -> Pid {
        self.take().unwrap()
    }

    /// Gets the pid without taking ownership of it.
    pub fn inner(&self) -> Pid {
        self.0.borrow().unwrap()
    }

    /// Creats a new child process.
    pub fn new(pid: Pid) -> Self {
        Self(RefCell::new(Some(pid)))
    }

    /// Attempts to take the inner process.
    pub fn take(&self) -> Option<Pid> {
        self.0.borrow_mut().take()
    }

    fn poll(pid: Pid) -> std::io::Result<()> {
        let flags = WaitPidFlag::WNOHANG;

        #[cfg(target_os = "linux")]
        let flags = flags | WaitPidFlag::__WALL;

        match waitpid(pid, Some(flags)) {
            Ok(result) => {
                tracing::trace!(?pid, ?result, "process polled");
                match result {
                    nix::sys::wait::WaitStatus::Exited(_, _) => Ok(()),
                    nix::sys::wait::WaitStatus::Signaled(_, _, _) => Ok(()),
                    nix::sys::wait::WaitStatus::Stopped(_, _) => Ok(()),
                    nix::sys::wait::WaitStatus::PtraceEvent(_, _, _)
                    | nix::sys::wait::WaitStatus::Continued(_)
                    | nix::sys::wait::WaitStatus::StillAlive
                    | nix::sys::wait::WaitStatus::PtraceSyscall(_) => {
                        Err(std::io::ErrorKind::WouldBlock.into())
                    }
                }
            }
            Err(e) => match e {
                nix::Error::ECHILD => Ok(()),
                other => Err(std::io::Error::from_raw_os_error(other as i32)),
            },
        }
    }

    fn kill(pid: Pid, signal: Signal) -> nix::Result<bool> {
        match nix::sys::signal::kill(pid, signal) {
            Ok(_) => match Self::poll(pid) {
                Ok(_) => Ok(true),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
                Err(error) => Err(error
                    .raw_os_error()
                    .map(nix::Error::from_raw)
                    .unwrap_or(nix::Error::EFAULT)),
            },
            Err(nix::Error::ESRCH) => Ok(true),
            Err(e) => Err(e),
        }
    }

    fn try_drop_impl(&mut self) -> nix::Result<()> {
        let pid = *if let Some(pid) = self.0.get_mut() {
            pid
        } else {
            return Ok(());
        };

        if Self::kill(pid, Signal::SIGTERM)? {
            return Ok(());
        }

        tracing::trace!(?pid, "waiting for process to exit");
        let end = Instant::now().add(Duration::from_millis(
            CHILD_DROP_WAIT_MILLIS.load(std::sync::atomic::Ordering::Relaxed),
        ));

        loop {
            match Self::poll(pid) {
                Ok(_) => return Ok(()),
                Err(err) => match err.kind() {
                    std::io::ErrorKind::WouldBlock if end > Instant::now() => {
                        std::thread::sleep(Duration::from_millis(15))
                    }
                    std::io::ErrorKind::WouldBlock => break,
                    _ => {
                        return Err(err
                            .raw_os_error()
                            .map(nix::Error::from_raw)
                            .unwrap_or(nix::Error::EFAULT))
                    }
                },
            }
        }

        tracing::warn!(?pid, "process has taken too long to exit, sending SIGKILL");
        Self::kill(pid, Signal::SIGKILL)?;

        let end = Instant::now().add(Duration::from_millis(
            CHILD_KILL_WAIT_MILLIS.load(std::sync::atomic::Ordering::Relaxed),
        ));
        loop {
            match Self::poll(pid) {
                Ok(_) => return Ok(()),
                Err(err) => match err.kind() {
                    std::io::ErrorKind::WouldBlock if end > Instant::now() => {
                        std::thread::sleep(Duration::from_millis(15))
                    }
                    std::io::ErrorKind::WouldBlock => break,
                    _ => {
                        return Err(err
                            .raw_os_error()
                            .map(nix::Error::from_raw)
                            .unwrap_or(nix::Error::EFAULT))
                    }
                },
            }
        }

        tracing::warn!(?pid, "process has not responded to SIGKILL");
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::{
        sync::{atomic::AtomicBool, Arc},
        time::Duration,
    };

    use anyhow::Context as _;
    use nix::{
        errno::Errno,
        sys::wait::{waitpid, WaitPidFlag},
        unistd::fork,
    };
    use porkg_test::{fork_test, init_test_logging};

    use crate::os::proc::ChildProcess;

    type Result = anyhow::Result<()>;

    #[fork_test]
    #[test]
    fn proc_drop() -> Result {
        init_test_logging();
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

    #[fork_test]
    #[test]
    fn proc_drop_kill() -> Result {
        init_test_logging();
        match unsafe { fork() }.context("creating child process")? {
            nix::unistd::ForkResult::Parent { child } => {
                super::CHILD_DROP_WAIT_MILLIS.store(100, std::sync::atomic::Ordering::SeqCst);
                super::CHILD_KILL_WAIT_MILLIS.store(100, std::sync::atomic::Ordering::SeqCst);

                let pid = child;
                std::thread::sleep(Duration::from_secs(1));
                let child: ChildProcess = child.into();
                drop(child);

                assert_eq!(waitpid(pid, Some(WaitPidFlag::WNOHANG)), Err(Errno::ECHILD));
            }
            nix::unistd::ForkResult::Child => {
                let term = Arc::new(AtomicBool::new(false));
                signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))
                    .unwrap();
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        }
        Ok(())
    }
}
