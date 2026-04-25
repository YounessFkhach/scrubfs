use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Mount and mirror any directory, stripping file metadata transparently on read.
/// Requires mat2 to be installed: https://0xacab.org/jvoisin/mat2
///
/// Run without arguments to mount all pairs saved in ~/.config/scrubfs/scrubfs.conf.
#[derive(Parser)]
#[command(name = "scrubfs", version)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Mount SOURCE at MOUNTPOINT (one-off, not saved to config)
    Mount {
        /// Directory to mirror
        source: PathBuf,
        /// Where to expose the stripped filesystem
        mountpoint: PathBuf,
    },

    /// Unmount a scrubfs filesystem
    Unmount {
        /// Mountpoint to unmount
        mountpoint: PathBuf,
    },

    /// Add a mount pair to the config and mount it
    Add {
        /// Directory to mirror
        source: PathBuf,
        /// Where to expose the stripped filesystem
        mountpoint: PathBuf,
    },

    /// Remove a mount pair from the config
    Remove {
        /// Mountpoint to remove from config
        mountpoint: PathBuf,
    },

    /// List configured mount pairs and their current status
    List,
}
