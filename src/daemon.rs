use std::path::Path;
use std::process::Command;

use crate::paths::pid_file;

pub fn is_mounted(path: &Path) -> bool {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else {
        return false;
    };
    let target = path.to_string_lossy();
    mounts
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(target.as_ref()))
}

pub fn run_unmount(path: &Path) -> bool {
    Command::new("fusermount3")
        .arg("-u")
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn write_pid() {
    let _ = std::fs::write(pid_file(), std::process::id().to_string());
}

pub fn remove_pid() {
    let _ = std::fs::remove_file(pid_file());
}

pub fn read_pid() -> Option<i32> {
    std::fs::read_to_string(pid_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

pub fn wait_for_signal() {
    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })
    .expect("could not set signal handler");
    rx.recv().ok();
}

pub fn cleanup_tmp(tmp_dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(tmp_dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub fn setup_dirs(dir: &Path, tmp_dir: &Path) {
    std::fs::create_dir_all(dir).expect("could not create ~/.config/scrubfs");
    std::fs::create_dir_all(tmp_dir).expect("could not create ~/.config/scrubfs/tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(tmp_dir, std::fs::Permissions::from_mode(0o700));
    }
}
