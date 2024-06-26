// Portions copied from: https://github.com/containers/youki/
// See ../../../notices/youki

use std::{
    ffi::{c_int, c_long},
    num::NonZeroUsize,
};

use crate::Syscall;

use nix::{
    errno::Errno,
    libc::{self, rlim_t, RLIM_INFINITY, SIGCHLD},
    sched::CloneFlags as CloneF,
    sys::{mman, resource},
};

pub use nix::unistd::Pid;
use porkg_private::os::proc::IntoExitCode;
use thiserror::Error;
use tracing::{span, Level, Span};

#[derive(Debug, Clone, Error)]
#[error("failed to clone process: {source}")]
pub struct CloneError {
    #[source]
    source: Errno,
}

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct CloneFlags: u64 {
        /// The parent of the new child  (as returned by getppid(2))
        /// will be the same as that of the calling process.
        const PARENT = CloneF::CLONE_PARENT.bits() as u64;
        /// The cloned child is started in a new mount namespace.
        const NEWNS = CloneF::CLONE_NEWNS.bits() as u64;
        /// Create the process in a new cgroup namespace.
        const NEWCGROUP = CloneF::CLONE_NEWCGROUP.bits() as u64;
        /// Create the process in a new UTS namespace.
        const NEWUTS = CloneF::CLONE_NEWUTS.bits() as u64;
        /// Create the process in a new IPC namespace.
        const NEWIPC = CloneF::CLONE_NEWIPC.bits() as u64;
        /// Create the process in a new user namespace.
        const NEWUSER = CloneF::CLONE_NEWUSER.bits() as u64;
        /// Create the process in a new PID namespace.
        const NEWPID = CloneF::CLONE_NEWPID.bits() as u64;
        /// Create the process in a new network namespace.
        const NEWNET = CloneF::CLONE_NEWNET.bits() as u64;
        #[doc(hidden)]
        const TEST_FALLBACK = 0x100000000;
    }
}

/// Syscalls related to cloning a process.
pub trait CloneSyscall {
    /// Clones the current process and invokes the `callback` inside the clone.
    fn clone<R: IntoExitCode + std::fmt::Debug, F: 'static + FnMut() -> R>(
        callback: F,
        flags: CloneFlags,
    ) -> Result<Pid, CloneError>;
}

impl CloneSyscall for Syscall {
    #[tracing::instrument(skip(callback), err(level = "debug"))]
    fn clone<R: IntoExitCode + std::fmt::Debug, F: 'static + FnMut() -> R>(
        mut callback: F,
        flags: CloneFlags,
    ) -> Result<Pid, CloneError> {
        let current = Span::current().id();
        let mut cb = Box::new(move || {
            let pid = Pid::this().as_raw();
            let new = span!(parent: None, Level::TRACE, "cloned", ?pid);
            new.follows_from(current.clone());
            let _span = new.entered();

            callback()
        });

        let exit_signal = if flags.contains(CloneFlags::PARENT) {
            0
        } else {
            SIGCHLD
        } as u64;
        match clone3(&mut cb, flags, exit_signal) {
            Ok(pid) => Ok(pid),
            // For now, we decide to only fallback on ENOSYS
            Err(nix::Error::ENOSYS) => {
                let flags = flags.difference(CloneFlags::TEST_FALLBACK).bits();
                let pid = clone_fallback(cb, flags, exit_signal)
                    .map_err(|source| CloneError { source })?;

                Ok(pid)
            }
            Err(err) => Err(CloneError { source: err }),
        }
    }
}

// Unlike the clone call, clone3 is currently using the kernel syscall, mimicking
// the interface of fork. There is not need to explicitly manage the memory, so
// we can safely passing the callback closure as reference.
fn clone3<R: IntoExitCode + std::fmt::Debug, F: FnMut() -> R>(
    cb: &mut Box<F>,
    flags: CloneFlags,
    exit_signal: u64,
) -> Result<Pid, nix::Error> {
    #[repr(C)]
    struct Clone3Args {
        flags: u64,
        pidfd: u64,
        child_tid: u64,
        parent_tid: u64,
        exit_signal: u64,
        stack: u64,
        stack_size: u64,
        tls: u64,
        set_tid: u64,
        set_tid_size: u64,
        cgroup: u64,
    }
    let flags = if flags.intersects(CloneFlags::TEST_FALLBACK) {
        return Err(Errno::ENOSYS);
    } else {
        flags.bits()
    };

    let mut args = Clone3Args {
        flags,
        pidfd: 0,
        child_tid: 0,
        parent_tid: 0,
        exit_signal,
        stack: 0,
        stack_size: 0,
        tls: 0,
        set_tid: 0,
        set_tid_size: 0,
        cgroup: 0,
    };
    let args_ptr = &mut args as *mut Clone3Args;
    let args_size = std::mem::size_of::<Clone3Args>();

    // For now, we can only use clone3 as a kernel syscall. Libc wrapper is not
    // available yet. This can have undefined behavior because libc authors do
    // not like people calling kernel syscall to directly create processes. Libc
    // does perform additional bookkeeping when calling clone or fork. So far,
    // we have not observed any issues with calling clone3 directly, but we
    // should keep an eye on it.
    match unsafe { libc::syscall(libc::SYS_clone3, args_ptr, args_size) } {
        -1 => Err(nix::Error::last()).inspect_err(|error| {
            if *error == Errno::ENOSYS {
                tracing::trace!(?error, "failed to clone(3)")
            } else {
                tracing::debug!(?error, "failed to clone(3)")
            }
        }),
        0 => {
            // Inside the cloned process, we execute the callback and exit with
            // the return code.
            std::process::exit(cb().report());
        }
        ret if ret >= 0 => Ok(Pid::from_raw(ret as i32))
            .inspect(|pid| tracing::trace!(?pid, "cloned using clone(3)")),
        _ => Err(Errno::UnknownErrno)
            .inspect_err(|_| tracing::debug!("clone(3) returned a negative pid")),
    }
}

fn clone_fallback<R: IntoExitCode + std::fmt::Debug, F: 'static + FnMut() -> R>(
    cb: Box<F>,
    flags: u64,
    exit_signal: u64,
) -> Result<Pid, nix::Error> {
    const DEFAULT_STACK_SIZE: usize = 8 * 1024 * 1024; // 8M
    const DEFAULT_PAGE_SIZE: usize = 4 * 1024; // 4K

    // Use sysconf to find the page size. If there is an error, we assume
    // the default 4K page size.
    let page_size = nix::unistd::sysconf(nix::unistd::SysconfVar::PAGE_SIZE)
        .inspect_err(|error| {
            tracing::trace!(
                ?error,
                "failed to get the system page size, assuming {DEFAULT_PAGE_SIZE}"
            )
        })
        .inspect(|size| tracing::trace!(?size, "got the system page size"))
        .unwrap_or(Some(DEFAULT_PAGE_SIZE as c_long))
        .map(|size| size as usize)
        .unwrap_or(DEFAULT_PAGE_SIZE);

    // Find out the default stack max size through getrlimit.
    let (rlim_cur, _) = resource::getrlimit(resource::Resource::RLIMIT_STACK)
        .map(|(a, b)| (interpret_limit(a), interpret_limit(b)))
        .inspect_err(|error| tracing::trace!(?error, "failed to get the system stack limit"))
        .inspect(|size| tracing::trace!(?size, "got the system stack limit"))
        .unwrap_or((None, None));

    // mmap will return ENOMEM if stack size is unlimited when we create the
    // child stack, so we need to set a reasonable default stack size.
    let default_stack_size = rlim_cur.map(|v| v as usize).unwrap_or(DEFAULT_STACK_SIZE);

    // Using the clone syscall requires us to create the stack space for the
    // child process instead of taken cared for us like fork call. We use mmap
    // here to create the stack.  Instead of guessing how much space the child
    // process needs, we allocate through mmap to the system default limit,
    // which is 8MB on most of the linux system today. This is OK since mmap
    // will only reserve the address space upfront, instead of allocating
    // physical memory upfront.  The stack will grow as needed, up to the size
    // reserved, so no wasted memory here. Lastly, the child stack only needs
    // to support the container init process set up code in Youki. When Youki
    // calls exec into the container payload, exec will reset the stack.  Note,
    // do not use MAP_GROWSDOWN since it is not well supported.
    // Ref: https://man7.org/linux/man-pages/man2/mmap.2.html
    tracing::trace!("allocating {default_stack_size} bytes for the new stack");
    let child_stack = unsafe {
        mman::mmap_anonymous(
            None,
            NonZeroUsize::new(default_stack_size).unwrap(),
            mman::ProtFlags::PROT_READ | mman::ProtFlags::PROT_WRITE,
            mman::MapFlags::MAP_PRIVATE | mman::MapFlags::MAP_ANONYMOUS | mman::MapFlags::MAP_STACK,
        )
        .inspect_err(|error| tracing::debug!(?error, "failed to allocate memory for the stack"))
        .inspect(|_| tracing::trace!("allocated the stack memory"))?
    };

    unsafe {
        // Consistent with how pthread_create sets up the stack, we create a
        // guard page of 1 page, to protect the child stack collision. Note, for
        // clone call, the child stack will grow downward, so the bottom of the
        // child stack is in the beginning.
        mman::mprotect(child_stack, page_size, mman::ProtFlags::PROT_NONE)
            .inspect_err(|error| tracing::info!(?error, "failed to create guard page"))
            .inspect(|_| tracing::trace!("created a guard page in the stack"))?
    };

    // Since the child stack for clone grows downward, we need to pass in
    // the top of the stack address.
    let child_stack_top = unsafe { child_stack.as_ptr().add(default_stack_size) };

    // Combine the clone flags with exit signals.
    let combined_flags = (flags | exit_signal) as c_int;

    // We are passing the boxed closure "cb" into the clone function as the a
    // function pointer in C. The box closure in Rust is both a function pointer
    // and a struct. However, when casting the box closure into libc::c_void,
    // the function pointer will be lost. Therefore, to work around the issue,
    // we double box the closure. This is consistent with how std::unix::thread
    // handles the closure.
    // Ref: https://github.com/rust-lang/rust/blob/master/library/std/src/sys/unix/thread.rs
    let data = Box::into_raw(Box::new(cb));

    // The nix::sched::clone wrapper doesn't provide the right interface.  Using
    // the clone syscall is one of the rare cases where we don't want rust to
    // manage the child stack memory. Instead, we want to use c_void directly
    // here.  Therefore, here we are using libc::clone syscall directly for
    // better control.  The child stack will be cleaned when exec is called or
    // the child process terminates. The nix wrapper also does not treat the
    // closure memory correctly. The wrapper implementation fails to pass the
    // right ownership to the new child process.
    // Ref: https://github.com/nix-rust/nix/issues/919
    // Ref: https://github.com/nix-rust/nix/pull/920
    let ret = unsafe {
        libc::clone(
            clone_main::<R, F>,
            child_stack_top,
            combined_flags,
            data as *mut libc::c_void,
        )
    };

    // After the clone returns, the heap memory associated with the Box closure
    // is duplicated in the cloned process. Therefore, we can safely re-box the
    // closure from the raw pointer and let rust to continue managing the
    // memory. We call drop here explicitly to avoid the warning that the
    // closure is not used. This is correct since the closure is called in the
    // cloned process, not the parent process.
    unsafe { drop(Box::from_raw(data)) };
    match ret {
        -1 => Err(nix::Error::last())
            .inspect_err(|error| tracing::info!(?error, "failed to clone(2)")),
        pid if ret > 0 => {
            Ok(Pid::from_raw(pid)).inspect(|pid| tracing::trace!(?pid, "cloned using clone(2)"))
        }
        _ => Err(nix::Error::UnknownErrno)
            .inspect_err(|_| tracing::info!("clone(2) returned a negative pid")),
    }
}

fn interpret_limit(limit: rlim_t) -> Option<rlim_t> {
    if limit == RLIM_INFINITY {
        None
    } else {
        Some(limit)
    }
}

// The main is a wrapper function passed into clone call below. The "data"
// arg is actually a raw pointer to the Box closure. so here, we re-box the
// pointer back into a box closure so the main takes ownership of the
// memory. Then we can call the closure.
extern "C" fn clone_main<R: IntoExitCode + std::fmt::Debug, F: 'static + FnMut() -> R>(
    data: *mut libc::c_void,
) -> libc::c_int {
    unsafe { Box::from_raw(data as *mut Box<F>)().report() }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read as _, Write as _},
        os::unix::net::UnixStream,
    };

    use crate::private::Syscall;
    use anyhow::{bail, Context as _};
    use nix::{
        sys::wait::{waitpid, WaitPidFlag, WaitStatus},
        unistd::{ForkResult, Pid},
    };

    use super::{CloneFlags, CloneSyscall as _};
    use porkg_test::{fork_test, init_test_logging};

    type Result = anyhow::Result<()>;

    #[fork_test]
    #[test]
    fn clone_basic() -> Result {
        init_test_logging();
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

    #[fork_test]
    #[test]
    fn clone_err() -> Result {
        init_test_logging();
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

    #[fork_test]
    #[test]
    fn clone_fallback() -> Result {
        init_test_logging();
        let pid = Syscall::clone(Box::new(|| 0), CloneFlags::TEST_FALLBACK)?;
        match waitpid(pid, Some(WaitPidFlag::__WALL))
            .with_context(|| format!("failed to wait for {pid:?}"))?
        {
            WaitStatus::Exited(p, status) => {
                assert_eq!(pid, p);
                assert_eq!(status, 0);
                Ok(())
            }
            status => bail!("unexpected status {status:?}"),
        }
    }

    #[fork_test]
    #[test]
    fn clone_parent() -> Result {
        init_test_logging();
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
                match waitpid(sibling_process_pid, Some(WaitPidFlag::__WALL)).with_context(
                    || format!("failed to wait for sibling {sibling_process_pid:?}"),
                )? {
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
}
