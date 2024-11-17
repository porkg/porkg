use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::hashing::StableHash;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub package: Metadata,
    pub dependencies: BTreeMap<String, Dependency>,
    #[serde(rename = "build-dependencies")]
    pub build_dependencies: BTreeMap<String, Dependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    #[serde(rename = "compat")]
    pub compatibility: Option<Compatibility>,
    pub targets: BTreeSet<String>,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Compatibility([u64; 3]);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Executable {
    pub exec: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockDefinition {
    pub dependencies: BTreeMap<String, String>,
    #[serde(rename = "build-dependencies")]
    pub build_dependencies: BTreeMap<String, String>,
}

impl StableHash for LockDefinition {
    fn update<H: crate::hashing::StableHasher>(&self, h: &mut H) {
        self.dependencies.update(h);
        self.build_dependencies.update(h);
    }
}
