// Portions copied from: https://github.com/containers/youki/
// See ../../../notices/youki

use nix::{
    errno::Errno,
    fcntl::OFlag,
    mount::{MntFlags, MsFlags},
    sys::stat::{makedev, Mode, SFlag},
};
use procfs::process::MountOptFields;
use std::{
    ffi::OsStr,
    os::unix::prelude::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd},
    path::{Path, PathBuf},
};
use thiserror::Error;

use crate::{Syscall, NO_PATH};

#[inline(always)]
pub(crate) fn make_owned_fd<F: IntoRawFd, E>(
    f: impl FnOnce() -> Result<F, E>,
) -> Result<OwnedFd, E> {
    f().map(|fd| unsafe { OwnedFd::from_raw_fd(fd.into_raw_fd()) })
}

const PROC: &[u8] = b"proc";
const SYSFS: &[u8] = b"sysfs";
const TMPFS: &[u8] = b"tmpfs";
const DEVPTS: &[u8] = b"devpts";
const OVERLAY: &[u8] = b"overlay";
const FUSE: &[u8] = b"fuse";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountKind {
    Proc,
    SysFs,
    TmpFs,
    DevPts,
    Overlay,
    Fuse,
}

impl AsRef<OsStr> for MountKind {
    #[inline]
    fn as_ref(&self) -> &OsStr {
        let cstr = match self {
            MountKind::Proc => PROC,
            MountKind::SysFs => SYSFS,
            MountKind::TmpFs => TMPFS,
            MountKind::DevPts => DEVPTS,
            MountKind::Overlay => OVERLAY,
            MountKind::Fuse => FUSE,
        };
        unsafe { OsStr::from_encoded_bytes_unchecked(cstr) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Null,
    Zero,
    Full,
    Random,
    URandom,
    Tty,
    Ptmx,
}

impl From<DeviceKind> for SFlag {
    #[inline]
    fn from(value: DeviceKind) -> Self {
        match value {
            DeviceKind::Null
            | DeviceKind::Zero
            | DeviceKind::Full
            | DeviceKind::Random
            | DeviceKind::URandom
            | DeviceKind::Tty
            | DeviceKind::Ptmx => SFlag::S_IFCHR,
        }
    }
}

impl From<DeviceKind> for u64 {
    #[inline]
    fn from(value: DeviceKind) -> Self {
        match value {
            DeviceKind::Null => makedev(1, 3),
            DeviceKind::Zero => makedev(1, 5),
            DeviceKind::Full => makedev(1, 7),
            DeviceKind::Random => makedev(1, 8),
            DeviceKind::URandom => makedev(1, 9),
            DeviceKind::Tty => makedev(5, 0),
            DeviceKind::Ptmx => makedev(5, 2),
        }
    }
}

#[derive(Debug, Clone, Error)]
#[error("failed to mount {path:?}: {source}")]
pub struct MountError {
    path: PathBuf,
    #[source]
    source: Errno,
}

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct MountFlags: u64 {
        /// Mount read-only
        const READ_ONLY = MsFlags::MS_RDONLY.bits();
        /// Do not update access times.
        const NO_ATIME = MsFlags::MS_NOATIME.bits();
        /// Do not update access times for directories.
        const NO_DIR_ATIME = MsFlags::MS_NODIRATIME.bits();
        /// Make this mount point private.
        const PRIVATE = MsFlags::MS_PRIVATE.bits();
        /// If this is a shared mount point that is a member of a peer group
        /// that contains other members, convert it to a slave mount.
        const SLAVE = MsFlags::MS_SLAVE.bits();
        /// Make  this mount point shared.
        const SHARED = MsFlags::MS_SHARED.bits();
        /// When a file on this filesystem is accessed,  update  the  file's
        /// last  access  time (atime) only if the current value of atime is
        /// less than or equal to the file's last modification time  (mtime) or
        /// last  status change time (ctime).
        const RELATIVE_ATIME = MsFlags::MS_RELATIME.bits();
        /// Always  update  the  last access time (atime) when files on this
        /// filesystem are accessed.
        const STRICT_ATIME = MsFlags::MS_STRICTATIME.bits();
        /// Reduce on-disk updates of inode timestamps (atime, mtime, ctime) by
        /// maintaining these changes only in memory.
        const LAZY_TIME = MsFlags::MS_LAZYTIME.bits();
    }
}

#[derive(Debug, Clone, Error)]
#[error("failed to bind mount {path:?}: {source}")]
pub struct BindError {
    path: PathBuf,
    #[source]
    source: Errno,
}

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct BindFlags: u64 {
        /// Create a recursive bind mount.
        const RECURSIVE = MsFlags::MS_REC.bits();
        /// Bind read-only.
        const READ_ONLY = MsFlags::MS_RDONLY.bits();
    }
}

#[derive(Debug, Clone, Error)]
#[error("failed to unmount {path:?}: {source}")]
pub struct UnmountError {
    path: PathBuf,
    #[source]
    source: Errno,
}

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct UnmountFlags: u64 {
        /// Attempt to unmount even if still in use, aborting pending requests.
        const FORCE = MntFlags::MNT_FORCE.bits() as u64;
        /// Lazy unmount.  Disconnect the file system immediately, but don't
        /// actually unmount it until it ceases to be busy.
        const DETACH = MntFlags::MNT_DETACH.bits() as u64;
        /// Mark the mount point as expired.
        const EXPIRE = MntFlags::MNT_EXPIRE.bits() as u64;
        /// Don't dereference `target` if it is a symlink.
        const NOFOLLOW = MntFlags::UMOUNT_NOFOLLOW.bits() as u64;
    }
}

#[derive(Debug, Clone, Error)]
#[error("failed to pivot to new root at {path:?}: {source}")]
pub struct PivotError {
    path: PathBuf,
    #[source]
    source: Errno,
}

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct PivotFlags: u64 {
        ///
        const CREATE_MOUNT = 0b0001;
    }
}

pub trait FsSyscall {
    fn mount<P1: AsRef<Path>, P2: AsRef<OsStr>, P3: AsRef<OsStr>>(
        source: Option<P1>,
        target: impl AsRef<Path>,
        kind: Option<P2>,
        flags: MountFlags,
        options: Option<P3>,
    ) -> Result<(), MountError>;

    fn bind(
        source: impl AsRef<Path>,
        target: impl AsRef<Path>,
        flags: BindFlags,
    ) -> Result<(), BindError>;

    fn unmount(path: impl AsRef<Path>, flags: UnmountFlags) -> Result<(), UnmountError>;

    fn pivot(new_root: impl AsRef<Path>) -> Result<(), PivotError>;
}

impl FsSyscall for Syscall {
    #[tracing::instrument(skip_all, fields(
        source = ?source.as_ref().map(AsRef::as_ref),
        target = ?target.as_ref(),
        kind = ?kind.as_ref().map(AsRef::as_ref),
        ?flags,
        options = ?options.as_ref().map(AsRef::as_ref),
    ), err(level = "debug"))]
    fn mount<P1: AsRef<Path>, P2: AsRef<OsStr>, P3: AsRef<OsStr>>(
        source: Option<P1>,
        target: impl AsRef<Path>,
        kind: Option<P2>,
        flags: MountFlags,
        options: Option<P3>,
    ) -> Result<(), MountError> {
        let source = source.as_ref().map(AsRef::as_ref);
        let target = target.as_ref();
        let kind = kind.as_ref().map(AsRef::as_ref);
        let options = options.as_ref().map(AsRef::as_ref);
        let flags = MsFlags::from_bits_truncate(flags.bits());

        nix::mount::mount(source, target, kind, flags, options)
            .inspect_err(|_| tracing::debug!("failed to mount"))
            .inspect(|_| tracing::trace!("created mount"))
            .map_err(|source| MountError {
                path: target.to_path_buf(),
                source,
            })
    }

    #[tracing::instrument(skip_all, fields(
        path = ?path.as_ref(),
        ?flags,
    ), err(level = "debug"))]
    fn unmount(path: impl AsRef<Path>, flags: UnmountFlags) -> Result<(), UnmountError> {
        let path = path.as_ref();
        let flags = MntFlags::from_bits_truncate(flags.bits() as i32);

        nix::mount::umount2(path, flags)
            .inspect_err(|_| tracing::debug!("failed to unmount"))
            .inspect(|_| tracing::trace!("unmounted"))
            .map_err(|source| UnmountError {
                path: path.to_path_buf(),
                source,
            })
    }

    #[tracing::instrument(skip_all, fields(
        path = ?new_root.as_ref(),
    ), err(level = "debug"))]
    fn pivot(new_root: impl AsRef<Path>) -> Result<(), PivotError> {
        let new_root = new_root.as_ref();

        match has_existing_shared_mount(new_root) {
            Some(true) => {
                tracing::trace!("shared mount exists at the path");
                nix::mount::mount(
                    NO_PATH,
                    new_root,
                    NO_PATH,
                    MsFlags::MS_PRIVATE | MsFlags::MS_REC,
                    NO_PATH,
                )
                .inspect_err(|_| tracing::debug!("failed to make the existing mount private"))
                .inspect(|_| tracing::trace!("made the existing mount private"))
                .map_err(|source| PivotError {
                    path: new_root.to_path_buf(),
                    source,
                })?;
            }
            None => {
                tracing::trace!("no mount exists at the path");
                nix::mount::mount(
                    Some(new_root),
                    new_root,
                    NO_PATH,
                    MsFlags::MS_PRIVATE | MsFlags::MS_BIND | MsFlags::MS_REC,
                    NO_PATH,
                )
                .inspect_err(|_| tracing::debug!("bound the path to itself"))
                .inspect(|_| tracing::trace!("bound the path to itself"))
                .map_err(|source| PivotError {
                    path: new_root.to_path_buf(),
                    source,
                })?;
            }
            _ => tracing::trace!("private mount exists at the path"),
        }

        let newroot = make_owned_fd(|| {
            nix::fcntl::open(
                new_root,
                OFlag::O_DIRECTORY | OFlag::O_RDONLY,
                Mode::empty(),
            )
        })
        .inspect_err(|_| tracing::debug!("failed to open new root directory"))
        .inspect(|_| tracing::trace!("opened new root directory"))
        .map_err(|source| PivotError {
            path: new_root.to_path_buf(),
            source,
        })?;

        // make the given path as the root directory for the container
        // see https://man7.org/linux/man-pages/man2/pivot_root.2.html, specially the notes
        // pivot root usually changes the root directory to first argument, and then mounts the original root
        // directory at second argument. Giving same path for both stacks mapping of the original root directory
        // above the new directory at the same path, then the call to umount unmounts the original root directory from
        // this path. This is done, as otherwise, we will need to create a separate temporary directory under the new root path
        // so we can move the original root there, and then unmount that. This way saves the creation of the temporary
        // directory to put original root directory.
        nix::unistd::pivot_root(new_root, new_root)
            .inspect_err(|_| tracing::debug!("failed to pivot to new path"))
            .inspect(|_| tracing::trace!("pivoted to new path"))
            .map_err(|source| PivotError {
                path: new_root.to_path_buf(),
                source,
            })?;

        // Make the original root directory rslave to avoid propagating unmount event to the host mount namespace.
        // We should use MS_SLAVE not MS_PRIVATE according to https://github.com/opencontainers/runc/pull/1500.
        nix::mount::mount(
            NO_PATH,
            "/",
            NO_PATH,
            MsFlags::MS_SLAVE | MsFlags::MS_REC,
            NO_PATH,
        )
        .inspect_err(|_| tracing::debug!("failed to re-mount root"))
        .inspect(|_| tracing::trace!("re-mounted the root directory"))
        .map_err(|source| PivotError {
            path: new_root.to_path_buf(),
            source,
        })?;

        // Unmount the original root directory which was stacked on top of new root directory
        // MNT_DETACH makes the mount point unavailable to new accesses, but waits till the original mount point
        // to be free of activity to actually unmount
        // see https://man7.org/linux/man-pages/man2/umount2.2.html for more information
        nix::mount::umount2("/", MntFlags::MNT_DETACH)
            .inspect_err(|_| tracing::debug!("failed to unmount original root"))
            .inspect(|_| tracing::trace!("unmounted original root"))
            .map_err(|source| PivotError {
                path: new_root.to_path_buf(),
                source,
            })?;

        // Change directory to the new root
        nix::unistd::fchdir(newroot.as_raw_fd())
            .inspect_err(|_| tracing::debug!("failed to chdir to the new root"))
            .inspect(|_| tracing::trace!("changed current directory to new root"))
            .map_err(|source| PivotError {
                path: new_root.to_path_buf(),
                source,
            })?;

        Ok(())
    }

    #[tracing::instrument(skip_all, fields(
        source = ?source.as_ref(),
        target = ?target.as_ref(),
        flags = ?flags,
    ), err(level = "debug"))]
    fn bind(
        source: impl AsRef<Path>,
        target: impl AsRef<Path>,
        flags: BindFlags,
    ) -> Result<(), BindError> {
        let source = source.as_ref();
        let target = target.as_ref();
        let mut mount_flags = MsFlags::MS_BIND;

        if flags.contains(BindFlags::RECURSIVE) {
            mount_flags |= MsFlags::MS_REC;
        }

        nix::mount::mount(Some(source), target, NO_PATH, mount_flags, NO_PATH)
            .inspect_err(|error| tracing::debug!(?error, "failed to bind mount"))
            .inspect(|_| tracing::trace!("created bind mount"))
            .map_err(|source| BindError {
                path: target.to_path_buf(),
                source,
            })?;

        if flags.contains(BindFlags::READ_ONLY) {
            nix::mount::mount(
                NO_PATH,
                target,
                NO_PATH,
                MsFlags::MS_REMOUNT | MsFlags::MS_BIND | MsFlags::MS_RDONLY,
                NO_PATH,
            )
            .inspect_err(|error| tracing::debug!(?error, "failed to make mount read-only"))
            .inspect(|_| tracing::trace!("made bind mount read-only"))
            .map_err(|source| BindError {
                path: target.to_path_buf(),
                source,
            })?;
        }

        Ok(())
    }
}

pub fn has_existing_shared_mount(path: &Path) -> Option<bool> {
    // Don't bail on errors, it's not the end of the world if we make a superfluous bind mount.
    let myself = procfs::process::Process::myself().ok()?;
    let mountinfo = myself.mountinfo().ok()?;
    let parent = mountinfo.into_iter().find(|mi| path == mi.mount_point)?;

    let has_shared = parent
        .opt_fields
        .iter()
        .any(|field| matches!(field, MountOptFields::Shared(_)));

    Some(has_shared)
}

// Covered by integration tests because clone is being used (see ./clone.rs)

// ../tests/fs_bind.rs
// ../tests/fs_bind_ro.rs
// ../tests/fs_mount.rs
// ../tests/fs_pivot.rs
