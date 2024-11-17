use std::collections::BTreeMap;

use porkg_model::hashing::SupportedHash;
use porkg_private::sandbox::{SandboxOptions, SandboxTask};
use tokio::fs;

use crate::Erro;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildTask {
    pub name: String,
    pub hash: SupportedHash,
    pub dependencies: BTreeMap<String, SupportedHash>,
    pub build_dependencies: BTreeMap<String, SupportedHash>,
}

impl BuildTask {
    pub async fn validate(&self, config: &crate::config::StoreConfig) -> Result<(), String> {
        let src_dir = config
            .path
            .join("pkg/by-hash/")
            .join(self.hash.to_string())
            .join("src");

        if !fs::try_exists(&src_dir).await.unwrap_or_default() {
            return Err("source directory not found".to_string());
        }

        let porkg_toml = src_dir.join("porkg.toml");
        if !fs::try_exists(&porkg_toml).await.unwrap_or_default() {
            return Err("porkg.toml not found".to_string());
        }

        for (dep, hash) in &self.dependencies {
            let dep_dir = config.path.join("pkg/by-hash/").join(hash.to_string());

            if !fs::try_exists(&dep_dir).await.unwrap_or_default() {
                return Err(format!("dependency {} not found", dep));
            }
        }

        for (dep, hash) in &self.build_dependencies {
            let dep_dir = config.path.join("pkg/by-hash/").join(hash.to_string());

            if !fs::try_exists(&dep_dir).await.unwrap_or_default() {
                return Err(format!("build dependency {} not found", dep));
            }
        }

        Ok(())
    }
}

impl SandboxTask for BuildTask {
    type ExecuteError = Erro;

    fn create_sandbox_options(&self) -> porkg_private::sandbox::SandboxOptions {
        SandboxOptions::default()
    }

    fn execute(
        &self,
        _fds: impl AsRef<[std::os::unix::prelude::OwnedFd]>,
    ) -> Result<(), Self::ExecuteError> {
        tracing::trace!("running");
        Ok(())
    }
}
