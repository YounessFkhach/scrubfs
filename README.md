# scrubfs

Mount and mirror any directory, stripping file metadata transparently on read.

scrubfs exposes a read-only virtual filesystem that mirrors a source directory.
When an application opens a file through the mount, it receives a metadata-free
copy. The original file on disk is never modified.

This removes the need to manually clean files before uploading them — simply
point your browser or application at the scrubfs mount instead of your real
directory.

## Requirements

- Linux with FUSE 3
- [mat2](https://0xacab.org/jvoisin/mat2)

## Installation

**From AUR:**

```bash
yay -S scrubfs
```

**From source:**

```bash
cargo build --release
sudo install -Dm755 target/release/scrubfs /usr/local/bin/scrubfs
```

## Usage

### Persistent mounts (recommended)

Add a source/mountpoint pair to your config and mount it:

```bash
mkdir ~/safe
scrubfs add ~/Downloads ~/safe
```

The pair is saved to `~/.config/scrubfs/scrubfs.conf`. Next time, simply run:

```bash
scrubfs
```

This mounts all configured pairs and waits. Press Ctrl+C to unmount all and exit.

### Other commands

```bash
# One-off mount (not saved to config)
scrubfs mount ~/Downloads ~/safe

# Unmount
scrubfs unmount ~/safe

# Remove a pair from config (also unmounts if currently mounted)
scrubfs remove ~/safe

# Show all configured pairs and their current mount status
scrubfs list
```

### In practice

Once a mount is active, use `~/safe` in your file manager or browser upload
dialog instead of `~/Downloads`. Any file you open through the mount is served
with its metadata stripped. Your original files are never touched.

## Config file

Pairs are stored in `~/.config/scrubfs/scrubfs.conf`:

```toml
[[mounts]]
source = "/home/alice/Downloads"
mountpoint = "/home/alice/safe"

[[mounts]]
source = "/home/alice/Documents"
mountpoint = "/home/alice/safedocs"
```

You can edit this file directly. Changes take effect on the next run.

## Supported formats

Metadata is stripped from the following file types:

| Category  | Formats                                       |
|-----------|-----------------------------------------------|
| Images    | jpg, jpeg, png, gif, tiff, bmp, webp          |
| Documents | pdf, docx, xlsx, pptx, odt, odp, ods, odg, epub |
| Audio     | mp3, flac, ogg, m4a                           |
| Video     | mp4, mkv                                      |
| Archives  | zip                                           |

Files with unsupported formats are served unchanged.

## How it works

scrubfs is a FUSE filesystem written in Rust. Directory listings and file
attributes are passed through directly from the source. When a file is opened,
scrubfs copies it to `~/.config/scrubfs/tmp/`, runs `mat2 --inplace` to strip
its metadata, and buffers the result in memory. All subsequent reads for that
file handle are served from the buffer. The source file is never touched. Temp
files are cleaned up on exit.

## Known limitations

- File sizes reported by `stat` reflect the original file. The actual bytes
  served after stripping may differ slightly. This does not affect file
  transfers or uploads.
- scrubfs is read-only. Write operations are not supported.

## License

MIT
