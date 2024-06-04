use std::{
    collections::HashSet,
    fmt::Write,
    fs::OpenOptions,
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use caps::Capability;
use nix::unistd::{Gid, Pid, Uid};
use thiserror::Error;

use crate::private::Syscall;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdMapping {
    host_start: u32,
    child_start: u32,
    length: u32,
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
}

#[derive(Debug, Error)]
#[error("failed to write the user mappings: {source}")]
pub struct WriteMappingsError {
    #[source]
    #[from]
    source: WriteMappingsErrorKind,
}

pub trait ProcSyscall {
    fn find_tools() -> IdMappingTools;
    fn write_mappings(
        pid: Option<Pid>,
        users: impl IntoIterator<Item = IdMapping>,
        groups: impl IntoIterator<Item = IdMapping>,
        tools: IdMappingTools,
    ) -> Result<(), WriteMappingsError>;
}

impl ProcSyscall for Syscall {
    fn find_tools() -> IdMappingTools {
        IdMappingTools {
            uid_map: which::which_global("newuidmap").ok(),
            gid_map: which::which_global("newgidmap").ok(),
        }
    }

    fn write_mappings(
        pid: Option<Pid>,
        users: impl IntoIterator<Item = IdMapping>,
        groups: impl IntoIterator<Item = IdMapping>,
        tools: IdMappingTools,
    ) -> Result<(), WriteMappingsError> {
        let users: HashSet<_> = users.into_iter().collect();
        let groups: HashSet<_> = groups.into_iter().collect();

        let pid = if let Some(pid) = pid {
            pid.to_string()
        } else {
            Pid::this().as_raw().to_string()
        };

        if can_direct(Uid::current().as_raw(), Capability::CAP_SETUID, &users) {
            map_direct(&pid, "uid_map", &users).map_err(WriteMappingsErrorKind::from)?;
        } else if let Some(tool) = tools.uid_map {
            map_shadowutils(&tool, &pid, &users).map_err(WriteMappingsErrorKind::from)?;
        } else {
            return Err(WriteMappingsErrorKind::NoTools.into());
        }

        if can_direct(Gid::current().as_raw(), Capability::CAP_SETGID, &groups) {
            let mut file = OpenOptions::new()
                .create(true)
                .append(false)
                .write(true)
                .open(format!("/proc/{pid}/setgroups"))
                .map_err(WriteMappingsErrorKind::from)?;
            file.write_all(b"deny")
                .map_err(WriteMappingsErrorKind::from)?;
            map_direct(&pid, "gid_map", &users).map_err(WriteMappingsErrorKind::from)?;
        } else if let Some(tool) = tools.gid_map {
            map_shadowutils(&tool, &pid, &users).map_err(WriteMappingsErrorKind::from)?;
        } else {
            return Err(WriteMappingsErrorKind::NoTools.into());
        }

        Ok(())
    }
}

fn can_direct(current: u32, cap: Capability, mappings: &HashSet<IdMapping>) -> bool {
    if current == 0 {
        return true;
    }

    let mut iter = mappings.iter();
    if let Some(mapping) = iter.next() {
        if iter.next().is_none() && mapping.host_start == current && mapping.length == 1 {
            return true;
        }
    }

    if caps::has_cap(None, caps::CapSet::Permitted, cap).unwrap_or_default() {
        return true;
    }

    false
}

fn map_direct(pid: &str, path: &str, mappings: &HashSet<IdMapping>) -> std::io::Result<()> {
    let mut val = String::new();

    for mapping in mappings {
        if !val.is_empty() {
            val.push('\n');
        }

        let host = mapping.host_start;
        let child = mapping.child_start;
        let length = mapping.length;
        write!(val, "{child} {host} {length}")
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(false)
        .write(true)
        .open(format!("/proc/{pid}/{path}"))?;
    file.write_all(val.as_bytes())?;

    Ok(())
}

fn map_shadowutils(path: &Path, pid: &str, mappings: &HashSet<IdMapping>) -> std::io::Result<()> {
    let args: Vec<String> = mappings
        .iter()
        .flat_map(|m| {
            [
                m.child_start.to_string(),
                m.host_start.to_string(),
                m.length.to_string(),
            ]
        })
        .collect();

    let status = Command::new(path)
        .arg(pid.to_string())
        .args(args)
        .output()
        .map_err(|err| {
            tracing::error!(?err, ?path, "failed to execute newuidmap/newgidmap");
            std::io::Error::from(std::io::ErrorKind::InvalidData)
        })?;

    if status.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::from(std::io::ErrorKind::InvalidData))
    }
}
