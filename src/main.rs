use clap::Parser;
use fuser::MountOption;
use std::path::{Path, PathBuf};
use std::process::Command;

mod fs;
mod stripper;

/// Virtual filesystem that mirrors a directory but strips file metadata on read.
/// Requires mat2 to be installed: https://0xacab.org/jvoisin/mat2
#[derive(Parser)]
#[command(name = "scrubfs")]
struct Args {
    /// Directory to mirror
    source: PathBuf,

    /// Where to mount the virtual filesystem
    mountpoint: PathBuf,
}

fn is_mounted(path: &Path) -> bool {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else {
        return false;
    };
    let target = path.to_string_lossy();
    mounts
        .lines()
        .any(|line| line.split_whitespace().nth(1) == Some(target.as_ref()))
}

fn unmount(path: &Path) {
    let status = Command::new("fusermount3").arg("-u").arg(path).status();
    match status {
        Ok(s) if s.success() => {}
        _ => eprintln!("scrubfs: warning: failed to unmount {}", path.display()),
    }
}

fn main() {
    let args = Args::parse();

    if !args.source.is_dir() {
        eprintln!("error: source '{}' is not a directory", args.source.display());
        std::process::exit(1);
    }
    if !args.mountpoint.is_dir() {
        eprintln!("error: mountpoint '{}' is not a directory", args.mountpoint.display());
        std::process::exit(1);
    }

    let mountpoint = args
        .mountpoint
        .canonicalize()
        .expect("could not resolve mountpoint path");

    if is_mounted(&mountpoint) {
        eprintln!("scrubfs: {} is already mounted, unmounting first...", mountpoint.display());
        unmount(&mountpoint);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    let options = vec![
        MountOption::RO,
        MountOption::FSName("scrubfs".to_string()),
    ];

    eprintln!("scrubfs: {} -> {}", args.source.display(), mountpoint.display());

    let _session = fuser::spawn_mount2(
        fs::MetaFS::new(args.source),
        &mountpoint,
        &options,
    )
    .expect("mount failed");

    eprintln!("scrubfs: ready. Press Ctrl+C to unmount and exit.");

    let (tx, rx) = std::sync::mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = tx.send(());
    })
    .expect("could not set signal handler");

    rx.recv().ok();
    eprintln!("scrubfs: unmounting {}...", mountpoint.display());
    // _session dropped here — fuser unmounts the filesystem automatically
}
