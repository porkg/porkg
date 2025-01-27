use std::{collections::BTreeMap, path::PathBuf};

use porkg_model::{hashing::SupportedHash, package::Package};
use porkg_private::{
    os::proc::IntoExitCode,
    sandbox::{SandboxOptions, SandboxTask},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::fs;
use tracing::trace;

use crate::config::StoreConfig;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildTask {
    store_path: String,
    name: String,
    hash: SupportedHash,
    dependencies: BTreeMap<String, SupportedHash>,
    build_dependencies: BTreeMap<String, SupportedHash>,
}

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum BuildError {
    #[error("source directory not found")]
    SourceDirectoryNotFound,
    #[error("porkg.toml not found")]
    PorkgTomlNotFound,
    #[error("dependency {name} not found")]
    DependencyNotFound { name: String, hash: SupportedHash },
    #[error("build dependency {name} not found")]
    BuildDependencyNotFound { name: String, hash: SupportedHash },
    #[error("failed to start tokio runtime")]
    TokioFailed,
    #[error("bad porkg manifest")]
    InvalidManfest,
}

impl IntoExitCode for BuildError {
    fn report(&self) -> i32 {
        1
    }
}

impl BuildTask {
    pub async fn try_new(
        config: &StoreConfig,
        name: String,
        hash: SupportedHash,
        dependencies: BTreeMap<String, SupportedHash>,
        build_dependencies: BTreeMap<String, SupportedHash>,
    ) -> Result<Self, BuildError> {
        let src_dir = config
            .path
            .join("pkg/by-hash/")
            .join(hash.to_string())
            .join("src");

        if !fs::try_exists(&src_dir).await.unwrap_or_default() {
            return Err(BuildError::SourceDirectoryNotFound);
        }

        let porkg_toml = src_dir.join("porkg.toml");
        if !fs::try_exists(&porkg_toml).await.unwrap_or_default() {
            return Err(BuildError::PorkgTomlNotFound);
        }

        for (name, hash) in &dependencies {
            let dep_dir = config.path.join("pkg/by-hash/").join(hash.to_string());

            if !fs::try_exists(&dep_dir).await.unwrap_or_default() {
                return Err(BuildError::DependencyNotFound {
                    name: name.clone(),
                    hash: *hash,
                });
            }
        }

        for (name, hash) in &build_dependencies {
            let dep_dir = config.path.join("pkg/by-hash/").join(hash.to_string());

            if !fs::try_exists(&dep_dir).await.unwrap_or_default() {
                return Err(BuildError::BuildDependencyNotFound {
                    name: name.clone(),
                    hash: *hash,
                });
            }
        }

        let store_path = config.path.to_string_lossy().into_owned();
        Ok(Self {
            store_path,
            name,
            hash,
            dependencies,
            build_dependencies,
        })
    }
}

impl SandboxTask for BuildTask {
    type ExecuteError = BuildError;

    fn create_sandbox_options(&self) -> porkg_private::sandbox::SandboxOptions {
        SandboxOptions::default()
    }

    fn execute(
        &self,
        fds: impl AsRef<[std::os::unix::prelude::OwnedFd]>,
    ) -> Result<(), Self::ExecuteError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| BuildError::TokioFailed)?;

        runtime.block_on(self.execute_async(fds.as_ref()))
    }
}

impl BuildTask {
    async fn execute_async(
        &self,
        _fds: &[std::os::unix::prelude::OwnedFd],
    ) -> Result<(), BuildError> {
        let store_dir: PathBuf = PathBuf::from(&self.store_path);
        let source_dir = store_dir
            .join("pkg/by-hash/")
            .join(self.hash.to_string())
            .join("src");

        let porkg_toml = source_dir.join("porkg.toml");
        let porkg_toml = fs::read_to_string(porkg_toml)
            .await
            .map_err(|_| BuildError::PorkgTomlNotFound)?;

        let manifest: Package = toml::from_str(&porkg_toml)
            .inspect_err(|error| tracing::debug!(?error, "invalid package manafest"))
            .map_err(|_| BuildError::InvalidManfest)?;

        trace!("{:?}", manifest);

        Ok(())
    }
}
