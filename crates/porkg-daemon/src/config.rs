use std::path::PathBuf;

use anyhow::Context as _;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub bind: BindConfig,
    #[serde(default)]
    pub store: StoreConfig,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let conf = config::Config::builder()
            .add_source(
                config::Environment::with_prefix("PORKG")
                    .try_parsing(true)
                    .separator("__")
                    .list_separator(":"),
            )
            .build()
            .context("while preparing to load config")?;
        conf.try_deserialize().context("while loading config")
    }
}

#[derive(Debug, Deserialize)]
pub struct BindConfig {
    #[serde(default = "default_socket_path", with = "porkg_private::ser::pathbuf")]
    pub socket: PathBuf,
    #[serde(default)]
    pub tcp: Vec<String>,
}

fn default_socket_path() -> PathBuf {
    // Automatically set the socket path if we are running under systemd
    if let Ok(dir) = std::env::var("RUNTIME_DIRECTORY") {
        let mut path = PathBuf::from(dir);
        path.push("porkg.sock");
        return path;
    }

    "/var/lib/porkg/porkg.sock".into()
}

impl Default for BindConfig {
    fn default() -> Self {
        Self {
            socket: default_socket_path(),
            tcp: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct StoreConfig {
    #[serde(default = "default_store_path", with = "porkg_private::ser::pathbuf")]
    pub path: PathBuf,
}

fn default_store_path() -> PathBuf {
    "/var/lib/porkg/store".into()
}
