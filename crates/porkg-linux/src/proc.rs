use std::{
    collections::HashSet,
    fmt::{self, Write},
    fs::OpenOptions,
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use caps::Capability;
use nix::{
    errno::Errno,
    unistd::{setresgid, setresuid, Gid, Pid, Uid},
};
use porkg_private::debug::PrintableBuffer;
use thiserror::Error;

use crate::private::Syscall;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdMapping {
    host_start: u32,
    child_start: u32,
    length: u32,
}

trait AsRaw {
    fn as_raw(&self) -> u32;
}

impl AsRaw for Gid {
    fn as_raw(&self) -> u32 {
        Gid::as_raw(*self)
    }
}

impl AsRaw for Uid {
    fn as_raw(&self) -> u32 {
        Uid::as_raw(*self)
    }
}

impl IdMapping {
    pub fn new(child_start: u32, host_start: u32, length: u32) -> Self {
        Self {
            host_start,
            child_start,
            length,
        }
    }

    pub fn current_user_to_root() -> Self {
        Self {
            host_start: Uid::current().as_raw(),
            child_start: 0,
            length: 1,
        }
    }

    pub fn current_group_to_root() -> Self {
        Self {
            host_start: Gid::current().as_raw(),
            child_start: 0,
            length: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdMappingTools {
    uid_map: Option<PathBuf>,
    gid_map: Option<PathBuf>,
}

#[derive(Debug, Error)]
enum WriteMappingsErrorKind {
    #[error("shadowutils not installed or not found")]
    NoTools,
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error("invalid mapping")]
    BadMapping,
    #[error("shadowutils failed to write mappings")]
    ShadowUtils,
}

#[derive(Debug, Error)]
#[error("failed to write the user mappings: {source}")]
pub struct WriteMappingsError {
    #[source]
    #[from]
    source: WriteMappingsErrorKind,
}

#[derive(Debug, Error)]
#[error("failed to set user and group ids: {source}")]
pub struct SetIdsError {
    #[source]
    #[from]
    source: Errno,
}

pub trait ProcSyscall {
    fn find_tools() -> IdMappingTools;
    fn write_mappings(
        pid: Option<Pid>,
        users: (impl IntoIterator<Item = IdMapping> + fmt::Debug),
        groups: (impl IntoIterator<Item = IdMapping> + fmt::Debug),
        tools: IdMappingTools,
    ) -> Result<(), WriteMappingsError>;
    fn set_ids(uid: Uid, gid: Gid) -> Result<(), SetIdsError>;
}

impl ProcSyscall for Syscall {
    #[tracing::instrument]
    fn find_tools() -> IdMappingTools {
        IdMappingTools {
            uid_map: which::which_global("newuidmap")
                .inspect_err(|error| tracing::warn!(?error, "unable to find newuidmap"))
                .inspect(|path| tracing::debug!(?path, "found newuidmap"))
                .ok(),
            gid_map: which::which_global("newgidmap")
                .inspect_err(|error| tracing::warn!(?error, "unable to find newgidmap"))
                .inspect(|path| tracing::debug!(?path, "found newgidmap"))
                .ok(),
        }
    }

    #[tracing::instrument(skip_all)]
    fn write_mappings(
        pid: Option<Pid>,
        users: (impl IntoIterator<Item = IdMapping> + fmt::Debug),
        groups: (impl IntoIterator<Item = IdMapping> + fmt::Debug),
        tools: IdMappingTools,
    ) -> Result<(), WriteMappingsError> {
        let users: HashSet<_> = users.into_iter().collect();
        let groups: HashSet<_> = groups.into_iter().collect();
        let pid = pid.unwrap_or_else(Pid::this);

        tracing::trace!(?pid, ?users, ?groups);

        if can_direct(Uid::current(), Capability::CAP_SETUID, &users) {
            map_direct(pid, "uid_map", &users)?;
        } else if let Some(tool) = tools.uid_map {
            map_shadowutils(pid, &tool, &users)?;
        } else {
            tracing::error!("setuidmap required to write mappings");
            return Err(WriteMappingsErrorKind::NoTools.into());
        }

        if can_direct(Gid::current(), Capability::CAP_SETGID, &groups) {
            let mut file = OpenOptions::new()
                .create(true)
                .append(false)
                .write(true)
                .open(format!("/proc/{pid}/setgroups", pid = pid.as_raw()))
                .map_err(WriteMappingsErrorKind::from)?;
            file.write_all(b"deny")
                .map_err(WriteMappingsErrorKind::from)?;
            map_direct(pid, "gid_map", &groups)?;
        } else if let Some(tool) = tools.gid_map {
            map_shadowutils(pid, &tool, &groups)?;
        } else {
            tracing::error!("setgidmap required to write mappings");
            return Err(WriteMappingsErrorKind::NoTools.into());
        }

        Ok(())
    }

    #[tracing::instrument]
    fn set_ids(uid: Uid, gid: Gid) -> Result<(), SetIdsError> {
        setresuid(uid, uid, uid)?;
        setresgid(gid, gid, gid)?;
        Ok(())
    }
}

fn can_direct<T: AsRaw + std::fmt::Debug + Copy>(
    current: T,
    cap: Capability,
    mappings: &HashSet<IdMapping>,
) -> bool {
    let raw = current.as_raw();

    if raw == 0 {
        tracing::trace!(?current, "SETUID/SETGID is implied");
        return true;
    }

    let mut iter = mappings.iter();
    if let Some(mapping) = iter.next() {
        if iter.next().is_none() && mapping.host_start == raw && mapping.length == 1 {
            tracing::trace!(?current, "matches mapping and mapping is length 1");
            return true;
        }
    }

    if caps::has_cap(None, caps::CapSet::Permitted, cap).unwrap_or_default() {
        tracing::trace!(?cap, "has capability");
        return true;
    }

    false
}

fn map_direct(
    pid: Pid,
    map_file: &str,
    mappings: &HashSet<IdMapping>,
) -> Result<(), WriteMappingsError> {
    let mut val = String::with_capacity(mappings.len() * (4 + 1 + 4 + 1 + 4));

    for mapping in mappings {
        if !val.is_empty() {
            val.push('\n');
        }

        let host = mapping.host_start;
        let child = mapping.child_start;
        let length = mapping.length;
        write!(val, "{child} {host} {length}")
            .inspect_err(|error| tracing::error!(?pid, ?error, "failed to format mappings"))
            .map_err(|_| WriteMappingsErrorKind::BadMapping)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(false)
        .write(true)
        .open(format!("/proc/{pid}/{map_file}"))
        .inspect_err(|error| tracing::error!(?pid, ?error, "failed to open mapping file"))
        .map_err(WriteMappingsErrorKind::from)?;
    file.write_all(val.as_bytes())
        .inspect_err(|error| tracing::error!(?pid, ?error, "failed to write mappings"))
        .inspect(|_| tracing::trace!(?pid, "wrote mappings"))
        .map_err(WriteMappingsErrorKind::from)?;

    Ok(())
}

fn map_shadowutils(
    pid: Pid,
    tool_path: &Path,
    mappings: &HashSet<IdMapping>,
) -> Result<(), WriteMappingsError> {
    let args: Vec<String> = [pid.as_raw().to_string()]
        .into_iter()
        .chain(mappings.iter().flat_map(|m| {
            [
                m.child_start.to_string(),
                m.host_start.to_string(),
                m.length.to_string(),
            ]
        }))
        .collect();

    let status = Command::new(tool_path).args(args).output().map_err(|err| {
        tracing::error!(?err, ?tool_path, "failed to execute shadowutils");
        WriteMappingsErrorKind::ShadowUtils
    })?;

    if status.status.success() {
        Ok(())
    } else {
        tracing::error!(
            stdout = ?PrintableBuffer(&status.stdout[..]),
            stderr = ?PrintableBuffer(&status.stderr[..]),
            status = ?status.status,
            ?tool_path,
            "shadowutils failed"
        );
        Err(WriteMappingsErrorKind::ShadowUtils.into())
    }
}
