use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Config {
    /// Where to mount the scrubfs drive. Defaults to /run/media/$USER/scrubfs.
    pub mountpoint: Option<PathBuf>,
    #[serde(default)]
    pub folders: Vec<FolderEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FolderEntry {
    pub source: PathBuf,
    pub name: String,
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let content = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!("scrubfs: warning: could not read config: {}", e);
                return Self::default();
            }
        };
        match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("scrubfs: warning: config parse error: {}", e);
                eprintln!("  Fix {} to restore your settings.", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(io::Error::other)?;
        std::fs::write(path, content)
    }

    /// Returns false if a folder with the same name already exists.
    pub fn add(&mut self, source: PathBuf, name: String) -> bool {
        if self.folders.iter().any(|f| f.name == name) {
            return false;
        }
        self.folders.push(FolderEntry { source, name });
        true
    }

    /// Returns false if no matching entry was found.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.folders.len();
        self.folders.retain(|f| f.name != name);
        self.folders.len() < before
    }
}
