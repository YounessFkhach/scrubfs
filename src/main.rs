use clap::Parser;
use fuser::MountOption;
use std::path::{Path, PathBuf};
use std::process::Command;

mod cli;
mod config;
mod fs;
mod stripper;

use cli::{Args, Cmd};
use config::Config;

// --- Paths -------------------------------------------------------------------

fn config_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".config/scrubfs")
}

fn config_file() -> PathBuf {
    config_dir().join("scrubfs.conf")
}

fn config_tmp_dir() -> PathBuf {
    config_dir().join("tmp")
}

fn pid_file() -> PathBuf {
    config_dir().join("scrubfs.pid")
}

fn default_mountpoint() -> PathBuf {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    PathBuf::from(format!("/run/media/{}/scrubfs", user))
}

fn drive_mountpoint(config: &Config) -> PathBuf {
    config.mountpoint.clone().unwrap_or_else(default_mountpoint)
}

fn expand_tilde(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(&s[2..]);
        }
    }
    path
}

// --- Helpers -----------------------------------------------------------------

fn is_mounted(path: &Path) -> bool {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else {
        return false;
    };
    let target = path.to_string_lossy();
    mounts
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(target.as_ref()))
}

fn run_unmount(path: &Path) -> bool {
    Command::new("fusermount3")
        .arg("-u")
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn write_pid() {
    let _ = std::fs::write(pid_file(), std::process::id().to_string());
}

fn remove_pid() {
    let _ = std::fs::remove_file(pid_file());
}

fn read_pid() -> Option<i32> {
    std::fs::read_to_string(pid_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn wait_for_signal() {
    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })
    .expect("could not set signal handler");
    rx.recv().ok();
}

fn cleanup_tmp(tmp_dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(tmp_dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn setup_dirs(dir: &Path, tmp_dir: &Path) {
    std::fs::create_dir_all(dir).expect("could not create ~/.config/scrubfs");
    std::fs::create_dir_all(tmp_dir).expect("could not create ~/.config/scrubfs/tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(tmp_dir, std::fs::Permissions::from_mode(0o700));
    }
}

// --- Main --------------------------------------------------------------------

fn main() {
    let args = Args::parse();
    let cfg_dir = config_dir();
    let cfg_file = config_file();
    let tmp_dir = config_tmp_dir();
    setup_dirs(&cfg_dir, &tmp_dir);

    match args.command {
        // No subcommand: start the drive.
        None => {
            let config = Config::load(&cfg_file);
            if config.folders.is_empty() {
                eprintln!("scrubfs: no folders configured.");
                eprintln!("Use 'scrubfs add <source>' to add one.");
                std::process::exit(1);
            }

            let mountpoint = drive_mountpoint(&config);

            if !mountpoint.exists() {
                std::fs::create_dir_all(&mountpoint).unwrap_or_else(|_| {
                    eprintln!("scrubfs: cannot create mountpoint at {}", mountpoint.display());
                    eprintln!("  Create it manually, or set a custom path with:");
                    eprintln!("  scrubfs config mountpoint ~/scrubfs");
                    std::process::exit(1);
                });
            }

            if is_mounted(&mountpoint) {
                eprintln!("scrubfs: drive is already running. Use 'scrubfs stop' first.");
                std::process::exit(1);
            }

            let entries: Vec<(String, PathBuf)> = config
                .folders
                .into_iter()
                .map(|f| (f.name, f.source))
                .collect();

            eprintln!("scrubfs: starting drive at {}", mountpoint.display());
            for (name, source) in &entries {
                eprintln!("  {}/  ->  {}", name, source.display());
            }

            let options = vec![
                MountOption::RO,
                MountOption::FSName("scrubfs".to_string()),
            ];

            let _session = fuser::spawn_mount2(
                fs::MetaFS::new(entries, tmp_dir.clone()),
                &mountpoint,
                &options,
            )
            .expect("mount failed");

            write_pid();
            eprintln!("scrubfs: ready. Press Ctrl+C or run 'scrubfs stop' to exit.");
            wait_for_signal();
            eprintln!("scrubfs: stopping drive...");
            remove_pid();
            cleanup_tmp(&tmp_dir);
        }

        Some(Cmd::Add { source, name }) => {
            let source = expand_tilde(source);
            let source = source.canonicalize().unwrap_or_else(|_| source.clone());

            if !source.is_dir() {
                eprintln!("error: '{}' is not a directory", source.display());
                std::process::exit(1);
            }

            let name = name.unwrap_or_else(|| {
                source
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "folder".to_string())
            });

            let mut config = Config::load(&cfg_file);
            if !config.add(source.clone(), name.clone()) {
                eprintln!("scrubfs: a folder named '{}' already exists", name);
                std::process::exit(1);
            }
            config.save(&cfg_file).expect("could not save config");
            eprintln!("scrubfs: added '{}' -> {}", name, source.display());

            if is_mounted(&drive_mountpoint(&config)) {
                eprintln!("scrubfs: restart the drive for the change to take effect.");
                eprintln!("  scrubfs stop && scrubfs");
            }
        }

        Some(Cmd::Remove { name }) => {
            let mut config = Config::load(&cfg_file);
            if !config.remove(&name) {
                eprintln!("scrubfs: no folder named '{}' found", name);
                std::process::exit(1);
            }
            config.save(&cfg_file).expect("could not save config");
            eprintln!("scrubfs: removed '{}'", name);

            if is_mounted(&drive_mountpoint(&config)) {
                eprintln!("scrubfs: restart the drive for the change to take effect.");
                eprintln!("  scrubfs stop && scrubfs");
            }
        }

        Some(Cmd::List) => {
            let config = Config::load(&cfg_file);
            let mountpoint = drive_mountpoint(&config);
            eprintln!("drive: {}", mountpoint.display());
            if config.folders.is_empty() {
                eprintln!("scrubfs: no folders configured.");
                return;
            }
            let running = is_mounted(&mountpoint);
            for f in &config.folders {
                let status = if running { "active" } else { "stopped" };
                println!("  {}  ->  {}  [{}]", f.name, f.source.display(), status);
            }
        }

        Some(Cmd::Stop) => {
            let config = Config::load(&cfg_file);
            let mountpoint = drive_mountpoint(&config);

            if let Some(pid) = read_pid() {
                let alive = unsafe { libc::kill(pid, 0) == 0 };
                if alive {
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                    eprintln!("scrubfs: drive stopped.");
                    return;
                }
                remove_pid();
            }

            if !is_mounted(&mountpoint) {
                eprintln!("scrubfs: drive is not running.");
                std::process::exit(1);
            }
            if run_unmount(&mountpoint) {
                eprintln!("scrubfs: drive stopped.");
            } else {
                eprintln!("scrubfs: failed to stop drive.");
                std::process::exit(1);
            }
        }

        Some(Cmd::Config { mountpoint }) => {
            let mountpoint = expand_tilde(mountpoint);
            let mut config = Config::load(&cfg_file);
            config.mountpoint = Some(mountpoint.clone());
            config.save(&cfg_file).expect("could not save config");
            eprintln!("scrubfs: mountpoint set to {}", mountpoint.display());
        }
    }
}
