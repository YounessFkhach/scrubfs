use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::unix::fs::{DirEntryExt, FileExt, MetadataExt};
use std::path::PathBuf;
use std::time::{Duration, UNIX_EPOCH};

use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, Request,
};
use libc::{EACCES, EIO, ENOENT};

use crate::stripper;

const TTL: Duration = Duration::from_secs(1);

enum OpenFile {
    /// Stripped content held in memory for the lifetime of the file handle.
    Buffer(Vec<u8>),
    /// Unsupported type — reads go straight to the real file.
    Passthrough(std::fs::File),
}

pub struct MetaFS {
    source: PathBuf,
    /// FUSE inode -> real path. Inode 1 is always the mounted root.
    inodes: HashMap<u64, PathBuf>,
    /// File handle -> open file state.
    open_files: HashMap<u64, OpenFile>,
    next_fh: u64,
}

impl MetaFS {
    pub fn new(source: PathBuf) -> Self {
        let mut inodes = HashMap::new();
        inodes.insert(1u64, source.clone());
        Self {
            source,
            inodes,
            open_files: HashMap::new(),
            next_fh: 1,
        }
    }

    fn alloc_fh(&mut self) -> u64 {
        let fh = self.next_fh;
        self.next_fh += 1;
        fh
    }

    fn make_attr(ino: u64, meta: &std::fs::Metadata) -> FileAttr {
        let kind = if meta.is_dir() {
            FileType::Directory
        } else if meta.file_type().is_symlink() {
            FileType::Symlink
        } else {
            FileType::RegularFile
        };

        let atime = meta.accessed().unwrap_or(UNIX_EPOCH);
        let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
        let ctime = {
            let s = meta.ctime();
            if s >= 0 {
                UNIX_EPOCH + Duration::from_secs(s as u64)
            } else {
                UNIX_EPOCH
            }
        };

        FileAttr {
            ino,
            size: meta.len(),
            blocks: meta.blocks(),
            atime,
            mtime,
            ctime,
            crtime: UNIX_EPOCH,
            kind,
            perm: (meta.mode() & 0o777) as u16,
            nlink: meta.nlink() as u32,
            uid: meta.uid(),
            gid: meta.gid(),
            rdev: meta.rdev() as u32,
            blksize: 4096,
            flags: 0,
        }
    }

    /// Translate a real inode to a FUSE inode.
    /// Real inode numbers are used directly except for the root.
    fn fuse_ino(real_ino: u64, is_source_root: bool) -> u64 {
        if is_source_root { 1 } else { real_ino }
    }
}

impl Filesystem for MetaFS {
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let parent_path = match self.inodes.get(&parent).cloned() {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let child_path = parent_path.join(name);

        match std::fs::symlink_metadata(&child_path) {
            Ok(meta) => {
                let is_root = child_path == self.source;
                let fuse_ino = Self::fuse_ino(meta.ino(), is_root);
                self.inodes.insert(fuse_ino, child_path);
                reply.entry(&TTL, &Self::make_attr(fuse_ino, &meta), 0);
            }
            Err(_) => reply.error(ENOENT),
        }
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        let path = match self.inodes.get(&ino).cloned() {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        match std::fs::symlink_metadata(&path) {
            Ok(meta) => reply.attr(&TTL, &Self::make_attr(ino, &meta)),
            Err(_) => reply.error(ENOENT),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let path = match self.inodes.get(&ino).cloned() {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let mut entries: Vec<(u64, FileType, std::ffi::OsString)> = vec![
            (ino, FileType::Directory, ".".into()),
        ];

        let parent_ino = if ino == 1 {
            1
        } else {
            path.parent()
                .and_then(|p| {
                    let meta = std::fs::symlink_metadata(p).ok()?;
                    let is_root = p == self.source.as_path();
                    Some(Self::fuse_ino(meta.ino(), is_root))
                })
                .unwrap_or(1)
        };
        entries.push((parent_ino, FileType::Directory, "..".into()));

        if let Ok(dir) = std::fs::read_dir(&path) {
            for entry in dir.flatten() {
                // Use inode and file type from the dirent directly — no stat syscall needed.
                let child_ino = Self::fuse_ino(entry.ino(), entry.path() == self.source);
                let kind = match entry.file_type().ok() {
                    Some(ft) if ft.is_dir() => FileType::Directory,
                    Some(ft) if ft.is_symlink() => FileType::Symlink,
                    _ => FileType::RegularFile,
                };
                self.inodes.insert(child_ino, entry.path());
                entries.push((child_ino, kind, entry.file_name()));
            }
        }

        for (i, (entry_ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
            if reply.add(*entry_ino, (i + 1) as i64, *kind, name) {
                break;
            }
        }

        reply.ok();
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: ReplyOpen) {
        let path = match self.inodes.get(&ino).cloned() {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let fh = self.alloc_fh();

        let open_file = if stripper::is_supported(&path) {
            match stripper::strip(&path) {
                Ok(bytes) => OpenFile::Buffer(bytes),
                Err(e) => {
                    eprintln!("scrubfs: strip failed for {}: {}", path.display(), e);
                    match std::fs::File::open(&path) {
                        Ok(f) => OpenFile::Passthrough(f),
                        Err(_) => {
                            reply.error(EIO);
                            return;
                        }
                    }
                }
            }
        } else {
            match std::fs::File::open(&path) {
                Ok(f) => OpenFile::Passthrough(f),
                Err(_) => {
                    reply.error(EACCES);
                    return;
                }
            }
        };

        self.open_files.insert(fh, open_file);
        reply.opened(fh, 0);
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        match self.open_files.get(&fh) {
            Some(OpenFile::Buffer(bytes)) => {
                let start = offset as usize;
                if start >= bytes.len() {
                    reply.data(&[]);
                } else {
                    let end = (start + size as usize).min(bytes.len());
                    reply.data(&bytes[start..end]);
                }
            }
            Some(OpenFile::Passthrough(file)) => {
                let mut buf = vec![0u8; size as usize];
                match file.read_at(&mut buf, offset as u64) {
                    Ok(n) => reply.data(&buf[..n]),
                    Err(_) => reply.error(EIO),
                }
            }
            None => reply.error(ENOENT),
        }
    }

    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
        let path = match self.inodes.get(&ino).cloned() {
            Some(p) => p,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        match std::fs::read_link(&path) {
            Ok(target) => {
                use std::os::unix::ffi::OsStrExt;
                reply.data(target.as_os_str().as_bytes());
            }
            Err(_) => reply.error(EIO),
        }
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        self.open_files.remove(&fh);
        reply.ok();
    }
}
