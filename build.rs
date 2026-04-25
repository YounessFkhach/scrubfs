use clap::CommandFactory;
use std::path::PathBuf;

// Pull in only the CLI definition, not the rest of the binary.
#[path = "src/cli.rs"]
mod cli;
use cli::Args;

fn main() -> std::io::Result<()> {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let man_dir = manifest_dir.join("man");
    std::fs::create_dir_all(&man_dir)?;

    // Generates scrubfs.1 plus scrubfs-<subcommand>.1 for each subcommand.
    clap_mangen::generate_to(Args::command(), &man_dir)?;

    println!("cargo:rerun-if-changed=src/cli.rs");

    Ok(())
}
