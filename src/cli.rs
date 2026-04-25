use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// A single virtual drive that mirrors your folders with metadata stripped on read.
///
/// Run without arguments to start the scrubfs drive. All configured folders
/// will appear as subdirectories inside /run/media/$USER/scrubfs/.
#[derive(Parser)]
#[command(name = "scrubfs", version)]
pub struct Args {
    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Add a folder to the drive
    Add {
        /// Source directory to mirror
        source: PathBuf,
        /// Name of the folder inside the drive (defaults to the directory name)
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Remove a folder from the drive by name
    Remove {
        /// Folder name to remove
        name: String,
    },

    /// List configured folders
    List,

    /// Stop the scrubfs drive
    Stop,

    /// Set the drive mountpoint
    Config {
        /// Path where the drive will be mounted
        mountpoint: PathBuf,
    },
}
