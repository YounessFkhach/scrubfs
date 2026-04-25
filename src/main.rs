use clap::Parser;
use fuser::{BackgroundSession, MountOption};
use std::path::{Path, PathBuf};
use std::process::Command;

mod cli;
mod config;
mod fs;
mod stripper;

use cli::{Args, Cmd};
use config::Config;

// --- Path helpers ------------------------------------------------------------

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

fn expand_tilde(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(&s[2..]);
        }
    }
    path
}

// --- Mount / unmount helpers -------------------------------------------------

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

fn spawn_mount(source: PathBuf, mountpoint: PathBuf, tmp_dir: &Path) -> BackgroundSession {
    let source = expand_tilde(source);
    let mountpoint = expand_tilde(mountpoint);

    let mountpoint = mountpoint
        .canonicalize()
        .unwrap_or(mountpoint);

    if is_mounted(&mountpoint) {
        eprintln!("scrubfs: {} is already mounted, unmounting first...", mountpoint.display());
        run_unmount(&mountpoint);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    let options = vec![
        MountOption::RO,
        MountOption::FSName("scrubfs".to_string()),
    ];

    eprintln!("scrubfs: {} -> {}", source.display(), mountpoint.display());

    fuser::spawn_mount2(
        fs::MetaFS::new(source, tmp_dir.to_owned()),
        &mountpoint,
        &options,
    )
    .expect("mount failed")
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

// --- Setup -------------------------------------------------------------------

fn setup_config_dir(dir: &Path, tmp_dir: &Path) {
    std::fs::create_dir_all(dir).expect("could not create ~/.config/scrubfs");
    std::fs::create_dir_all(tmp_dir).expect("could not create ~/.config/scrubfs/tmp");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tmp_dir, std::fs::Permissions::from_mode(0o700))
            .expect("could not set permissions on tmp dir");
    }
}

// --- Main --------------------------------------------------------------------

fn main() {
    let args = Args::parse();
    let cfg_dir = config_dir();
    let cfg_file = config_file();
    let tmp_dir = config_tmp_dir();
    setup_config_dir(&cfg_dir, &tmp_dir);

    match args.command {
        // No subcommand: mount all pairs from config.
        None => {
            let config = Config::load(&cfg_file);
            if config.mounts.is_empty() {
                eprintln!("scrubfs: no mounts configured.");
                eprintln!("Use 'scrubfs add <source> <mountpoint>' to add one.");
                std::process::exit(1);
            }
            let sessions: Vec<BackgroundSession> = config
                .mounts
                .into_iter()
                .map(|m| spawn_mount(m.source, m.mountpoint, &tmp_dir))
                .collect();
            eprintln!("scrubfs: ready. Press Ctrl+C to unmount all and exit.");
            wait_for_signal();
            eprintln!("scrubfs: unmounting all...");
            drop(sessions);
            cleanup_tmp(&tmp_dir);
        }

        Some(Cmd::Mount { source, mountpoint }) => {
            if !source.is_dir() {
                eprintln!("error: source '{}' is not a directory", source.display());
                std::process::exit(1);
            }
            if !mountpoint.is_dir() {
                eprintln!("error: mountpoint '{}' is not a directory", mountpoint.display());
                std::process::exit(1);
            }
            let _session = spawn_mount(source, mountpoint, &tmp_dir);
            eprintln!("scrubfs: ready. Press Ctrl+C to unmount and exit.");
            wait_for_signal();
            eprintln!("scrubfs: unmounting...");
            cleanup_tmp(&tmp_dir);
        }

        Some(Cmd::Unmount { mountpoint }) => {
            let mountpoint = mountpoint.canonicalize().unwrap_or(mountpoint);
            if !is_mounted(&mountpoint) {
                eprintln!("scrubfs: {} is not mounted", mountpoint.display());
                std::process::exit(1);
            }
            if run_unmount(&mountpoint) {
                eprintln!("scrubfs: unmounted {}", mountpoint.display());
            } else {
                eprintln!("scrubfs: failed to unmount {}", mountpoint.display());
                std::process::exit(1);
            }
        }

        Some(Cmd::Add { source, mountpoint }) => {
            let source_expanded = expand_tilde(source);
            let source = source_expanded
                .canonicalize()
                .unwrap_or_else(|_| source_expanded.clone());
            let mountpoint = expand_tilde(mountpoint);

            if !source.is_dir() {
                eprintln!("error: source '{}' is not a directory", source.display());
                std::process::exit(1);
            }
            if !mountpoint.is_dir() {
                eprintln!("error: mountpoint '{}' is not a directory", mountpoint.display());
                std::process::exit(1);
            }

            let mut config = Config::load(&cfg_file);
            if !config.add(source.clone(), mountpoint.clone()) {
                eprintln!(
                    "scrubfs: {} is already in the config",
                    mountpoint.display()
                );
                std::process::exit(1);
            }
            config.save(&cfg_file).expect("could not save config");
            eprintln!(
                "scrubfs: added {} -> {}",
                source.display(),
                mountpoint.display()
            );

            let _session = spawn_mount(source, mountpoint, &tmp_dir);
            eprintln!("scrubfs: ready. Press Ctrl+C to unmount and exit.");
            wait_for_signal();
            eprintln!("scrubfs: unmounting...");
            cleanup_tmp(&tmp_dir);
        }

        Some(Cmd::Remove { mountpoint }) => {
            let mountpoint = expand_tilde(mountpoint);
            let mountpoint = mountpoint.canonicalize().unwrap_or(mountpoint);

            let mut config = Config::load(&cfg_file);
            if !config.remove(&mountpoint) {
                eprintln!(
                    "scrubfs: {} is not in the config",
                    mountpoint.display()
                );
                std::process::exit(1);
            }
            config.save(&cfg_file).expect("could not save config");
            eprintln!("scrubfs: removed {}", mountpoint.display());

            if is_mounted(&mountpoint) {
                if run_unmount(&mountpoint) {
                    eprintln!("scrubfs: unmounted {}", mountpoint.display());
                } else {
                    eprintln!("scrubfs: warning: could not unmount {}", mountpoint.display());
                }
            }
        }

        Some(Cmd::List) => {
            let config = Config::load(&cfg_file);
            if config.mounts.is_empty() {
                eprintln!("scrubfs: no mounts configured.");
                return;
            }
            for m in &config.mounts {
                let status = if is_mounted(&m.mountpoint) {
                    "mounted"
                } else {
                    "not mounted"
                };
                println!(
                    "{} -> {}  [{}]",
                    m.source.display(),
                    m.mountpoint.display(),
                    status
                );
            }
        }
    }
}
