use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    #[serde(default)]
    pub mounts: Vec<MountEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MountEntry {
    pub source: PathBuf,
    pub mountpoint: PathBuf,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        std::fs::write(path, content)
    }

    /// Returns false if an entry with the same mountpoint already exists.
    pub fn add(&mut self, source: PathBuf, mountpoint: PathBuf) -> bool {
        if self.mounts.iter().any(|m| m.mountpoint == mountpoint) {
            return false;
        }
        self.mounts.push(MountEntry { source, mountpoint });
        true
    }

    /// Returns false if no matching entry was found.
    pub fn remove(&mut self, mountpoint: &Path) -> bool {
        let before = self.mounts.len();
        self.mounts.retain(|m| m.mountpoint != mountpoint);
        self.mounts.len() < before
    }
}
