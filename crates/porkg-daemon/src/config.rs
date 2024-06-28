use anyhow::Context as _;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    bind: BindConfig,
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
    socket: String,
    tcp: Vec<String>,
}

impl Default for BindConfig {
    fn default() -> Self {
        Self {
            socket: "/var/lib/porkg/porkg.sock".into(),
            tcp: Vec::new(),
        }
    }
}
