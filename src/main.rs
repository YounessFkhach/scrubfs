use clap::Parser;
use fuser::MountOption;
use std::path::PathBuf;

mod cli;
mod config;
mod daemon;
mod fs;
mod paths;
mod stripper;

use cli::{Args, Cmd};
use config::Config;
use daemon::{cleanup_tmp, is_mounted, read_pid, remove_pid, run_unmount, setup_dirs,
             wait_for_signal, write_pid};
use paths::{config_dir, config_file, config_tmp_dir, drive_mountpoint, expand_tilde};

fn main() {
    let args = Args::parse();
    let cfg_dir = config_dir();
    let cfg_file = config_file();
    let tmp_dir = config_tmp_dir();
    setup_dirs(&cfg_dir, &tmp_dir);

    match args.command {
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
                    eprintln!("  scrubfs config ~/scrubfs");
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
                    eprintln!("scrubfs: stop signal sent.");
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

        Some(Cmd::Setup) => {
            if unsafe { libc::getuid() } != 0 {
                eprintln!("scrubfs: 'setup' must be run with sudo.");
                std::process::exit(1);
            }

            let sudo_user = std::env::var("SUDO_USER").unwrap_or_else(|_| {
                eprintln!("scrubfs: SUDO_USER not set — run as: sudo scrubfs setup");
                std::process::exit(1);
            });
            let sudo_uid: libc::uid_t = std::env::var("SUDO_UID")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| {
                    eprintln!("scrubfs: SUDO_UID not set");
                    std::process::exit(1);
                });
            let sudo_gid: libc::gid_t = std::env::var("SUDO_GID")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| {
                    eprintln!("scrubfs: SUDO_GID not set");
                    std::process::exit(1);
                });

            let media_user = PathBuf::from(format!("/run/media/{}", sudo_user));
            let mountpoint = media_user.join("scrubfs");

            std::fs::create_dir_all(&mountpoint).unwrap_or_else(|e| {
                eprintln!("scrubfs: failed to create {}: {}", mountpoint.display(), e);
                std::process::exit(1);
            });

            for dir in [&media_user, &mountpoint] {
                let cpath = std::ffi::CString::new(dir.to_string_lossy().as_bytes()).unwrap();
                if unsafe { libc::chown(cpath.as_ptr(), sudo_uid, sudo_gid) } != 0 {
                    eprintln!("scrubfs: chown failed for {}", dir.display());
                    std::process::exit(1);
                }
            }

            eprintln!("scrubfs: {} is ready.", mountpoint.display());
            eprintln!("Run 'scrubfs' to start the drive.");
        }
    }
}
