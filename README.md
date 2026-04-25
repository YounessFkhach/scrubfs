# scrubfs

A single virtual drive that mirrors your folders with metadata stripped on read.

scrubfs mounts as one drive containing all your configured folders as
subdirectories. When an application opens a file through the drive, it receives
a metadata-free copy. Original files on disk are never modified.

This removes the need to manually clean files before uploading. Simply navigate
to the scrubfs drive in your file manager or browser upload dialog instead of
your real directory.

## Requirements

- Linux with FUSE 3
- [mat2](https://0xacab.org/jvoisin/mat2)

## Installation

```bash
cargo build --release
sudo install -Dm755 target/release/scrubfs /usr/local/bin/scrubfs
```

## Quick start

```bash
# Set where the drive will appear (once)
scrubfs config ~/scrubfs

# Add folders to the drive
scrubfs add ~/Downloads
scrubfs add ~/Documents
scrubfs add ~/work/client-docs --name client

# Start the drive
scrubfs
```

The drive appears at `~/scrubfs` (or wherever you configured) with this layout:

```
~/scrubfs/
├── Downloads/     ← mirrors ~/Downloads, metadata stripped on read
├── Documents/     ← mirrors ~/Documents
└── client/        ← mirrors ~/work/client-docs
```

Open this directory in your file manager or use it in a browser upload dialog.
Press Ctrl+C or run `scrubfs stop` to unmount and exit.

## Commands

```bash
scrubfs                              # start the drive
scrubfs stop                         # stop the drive

scrubfs add <source>                 # add a folder (name = directory name)
scrubfs add <source> --name <name>   # add a folder with a custom name
scrubfs remove <name>                # remove a folder from the drive
scrubfs list                         # show configured folders and status

scrubfs config <mountpoint>          # set where the drive is mounted
sudo scrubfs setup                   # create default mountpoint (once, on first install)
```

## Default mountpoint

The default mountpoint is `/run/media/$USER/scrubfs`, which causes most file
managers (Nautilus, Thunar, Dolphin) to display the drive in the sidebar as a
removable device.

`/run/media/$USER/` is root-owned on most systems, so scrubfs cannot create
the directory on its own. Run the setup command once to create it:

```bash
sudo scrubfs setup
```

This creates `/run/media/$USER/scrubfs` and transfers ownership to your user.
After that, `scrubfs` starts the drive without requiring sudo.

If you prefer not to use sudo, set a custom mountpoint instead:

```bash
scrubfs config ~/scrubfs
```

## Config file

Settings are stored in `~/.config/scrubfs/scrubfs.conf`:

```toml
mountpoint = "/home/alice/scrubfs"

[[folders]]
source = "/home/alice/Downloads"
name = "Downloads"

[[folders]]
source = "/home/alice/work/client-docs"
name = "client"
```

You can edit this file directly. Changes take effect on the next `scrubfs` run.

## Supported formats

Metadata is stripped from the following file types:

| Category  | Formats                                             |
|-----------|-----------------------------------------------------|
| Images    | jpg, jpeg, png, gif, tiff, bmp, webp                |
| Documents | pdf, docx, xlsx, pptx, odt, odp, ods, odg, epub    |
| Audio     | mp3, flac, ogg, m4a                                 |
| Video     | mp4, mkv                                            |
| Archives  | zip                                                 |

Files with unsupported formats are served unchanged.

## How it works

scrubfs is a FUSE filesystem written in Rust. The drive root is a virtual
directory whose children are the configured folder names. When a file is opened,
scrubfs copies it to `~/.config/scrubfs/tmp/`, runs `mat2 --inplace` to strip
its metadata, and buffers the result in memory. All reads for that file handle
are served from the buffer. The source file is never touched. Temp files are
cleaned up on exit.

## Known limitations

- File sizes reported by `stat` reflect the original file. Actual bytes served
  after stripping may differ slightly. This does not affect uploads or transfers.
- scrubfs is read-only. Write operations are not supported.
- Changes made with `scrubfs add` or `scrubfs remove` take effect after
  restarting the drive (`scrubfs stop && scrubfs`).

## License

MIT
