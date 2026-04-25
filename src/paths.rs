use std::path::PathBuf;

use crate::config::Config;

pub fn config_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".config/scrubfs")
}

pub fn config_file() -> PathBuf {
    config_dir().join("scrubfs.conf")
}

pub fn config_tmp_dir() -> PathBuf {
    config_dir().join("tmp")
}

pub fn pid_file() -> PathBuf {
    config_dir().join("scrubfs.pid")
}

pub fn default_mountpoint() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("scrubfs")
}

pub fn drive_mountpoint(config: &Config) -> PathBuf {
    config.mountpoint.clone().unwrap_or_else(default_mountpoint)
}

pub fn expand_tilde(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    path
}
