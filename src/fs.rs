use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEmpty,
    ReplyEntry, ReplyOpen, Request,
};
use libc::{EACCES, EIO, EISDIR, ENOENT};

use crate::stripper;

const TTL: Duration = Duration::from_secs(1);
const ROOT_INO: u64 = 1;

// Entry roots get fixed inodes: entry[i] -> 2 + i.
// Real files/dirs under entries get inodes from the counter starting at 2 + n_entries.

enum OpenFile {
    Buffer(Vec<u8>),
    Passthrough(std::fs::File),
}

pub struct MetaFS {
    // (display name, real source path)
    entries: Vec<(String, PathBuf)>,
    tmp_dir: PathBuf,
    mountpoint: PathBuf,
    ino_to_path: HashMap<u64, PathBuf>,
    path_to_ino: HashMap<PathBuf, u64>,
    open_files: HashMap<u64, OpenFile>,
    next_ino: u64,
    next_fh: u64,
    mounted_at: SystemTime,
}

impl MetaFS {
    pub fn new(entries: Vec<(String, PathBuf)>, tmp_dir: PathBuf, mountpoint: PathBuf) -> Self {
        let n = entries.len() as u64;
        let mut ino_to_path = HashMap::new();
        let mut path_to_ino = HashMap::new();
        for (i, (_, source)) in entries.iter().enumerate() {
            let ino = 2 + i as u64;
            ino_to_path.insert(ino, source.clone());
            path_to_ino.insert(source.clone(), ino);
        }
        Self {
            entries,
            tmp_dir,
            mountpoint,
            ino_to_path,
            path_to_ino,
            open_files: HashMap::new(),
            next_ino: 2 + n,
            next_fh: 1,
            mounted_at: SystemTime::now(),
        }
    }

    fn is_entry_root(&self, ino: u64) -> bool {
        ino >= 2 && ino < 2 + self.entries.len() as u64
    }

    fn find_or_alloc_ino(&mut self, path: &Path) -> u64 {
        if let Some(&ino) = self.path_to_ino.get(path) {
            return ino;
        }
        let ino = self.next_ino;
        self.next_ino += 1;
        self.ino_to_path.insert(ino, path.to_owned());
        self.path_to_ino.insert(path.to_owned(), ino);
        ino
    }

    fn alloc_fh(&mut self) -> u64 {
        let fh = self.next_fh;
        self.next_fh += 1;
        fh
    }

    fn root_attr(&self) -> FileAttr {
        FileAttr {
            ino: ROOT_INO,
            size: 0,
            blocks: 0,
            atime: self.mounted_at,
            mtime: self.mounted_at,
            ctime: self.mounted_at,
            crtime: UNIX_EPOCH,
            kind: FileType::Directory,
            perm: 0o555,
            nlink: 2,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: 4096,
            flags: 0,
        }
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
            if s >= 0 { UNIX_EPOCH + Duration::from_secs(s as u64) } else { UNIX_EPOCH }
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
}

impl Filesystem for MetaFS {
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if parent == ROOT_INO {
            let name_str = name.to_string_lossy();
            if let Some((idx, (_, source))) = self
                .entries
                .iter()
                .enumerate()
                .find(|(_, (n, _))| n.as_str() == name_str.as_ref())
            {
                let ino = 2 + idx as u64;
                match std::fs::symlink_metadata(source) {
                    Ok(meta) => reply.entry(&TTL, &Self::make_attr(ino, &meta), 0),
                    Err(_) => reply.error(ENOENT),
                }
            } else {
                reply.error(ENOENT);
            }
            return;
        }

        let parent_path = match self.ino_to_path.get(&parent).cloned() {
            Some(p) => p,
            None => { reply.error(ENOENT); return; }
        };

        let child_path = parent_path.join(name);
        match std::fs::symlink_metadata(&child_path) {
            Ok(meta) => {
                let ino = self.find_or_alloc_ino(&child_path);
                reply.entry(&TTL, &Self::make_attr(ino, &meta), 0);
            }
            Err(_) => reply.error(ENOENT),
        }
    }

    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        if ino == ROOT_INO {
            reply.attr(&TTL, &self.root_attr());
            return;
        }

        let path = match self.ino_to_path.get(&ino).cloned() {
            Some(p) => p,
            None => { reply.error(ENOENT); return; }
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
        if ino == ROOT_INO {
            let mut entries: Vec<(u64, FileType, OsString)> = vec![
                (ROOT_INO, FileType::Directory, ".".into()),
                (ROOT_INO, FileType::Directory, "..".into()),
            ];
            for (i, (name, _)) in self.entries.iter().enumerate() {
                entries.push((2 + i as u64, FileType::Directory, name.into()));
            }
            for (i, (entry_ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
                if reply.add(*entry_ino, (i + 1) as i64, *kind, name) { break; }
            }
            reply.ok();
            return;
        }

        let path = match self.ino_to_path.get(&ino).cloned() {
            Some(p) => p,
            None => { reply.error(ENOENT); return; }
        };

        let parent_ino = if self.is_entry_root(ino) {
            ROOT_INO
        } else {
            path.parent()
                .and_then(|p| self.path_to_ino.get(p).copied())
                .unwrap_or(ROOT_INO)
        };

        // Collect dir entries before mutably borrowing self for inode allocation.
        // Exclude the mountpoint itself to prevent infinite recursion when a source
        // directory contains the mountpoint (e.g. source=~ and mountpoint=~/scrubfs).
        let raw: Vec<(PathBuf, FileType, OsString)> = match std::fs::read_dir(&path) {
            Ok(dir) => dir
                .flatten()
                .filter(|e| e.path() != self.mountpoint)
                .map(|e| {
                    let kind = match e.file_type().ok() {
                        Some(ft) if ft.is_dir() => FileType::Directory,
                        Some(ft) if ft.is_symlink() => FileType::Symlink,
                        _ => FileType::RegularFile,
                    };
                    (e.path(), kind, e.file_name())
                })
                .collect(),
            Err(_) => { reply.error(EIO); return; }
        };

        let mut entries: Vec<(u64, FileType, OsString)> = vec![
            (ino, FileType::Directory, ".".into()),
            (parent_ino, FileType::Directory, "..".into()),
        ];
        for (child_path, kind, name) in raw {
            let child_ino = self.find_or_alloc_ino(&child_path);
            entries.push((child_ino, kind, name));
        }

        for (i, (entry_ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
            if reply.add(*entry_ino, (i + 1) as i64, *kind, name) { break; }
        }
        reply.ok();
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: ReplyOpen) {
        if ino == ROOT_INO || self.is_entry_root(ino) {
            reply.error(EISDIR);
            return;
        }

        let path = match self.ino_to_path.get(&ino).cloned() {
            Some(p) => p,
            None => { reply.error(ENOENT); return; }
        };

        let fh = self.alloc_fh();

        let open_file = if stripper::is_supported(&path) {
            match stripper::strip(&path, &self.tmp_dir) {
                Ok(bytes) => OpenFile::Buffer(bytes),
                Err(e) => {
                    eprintln!("scrubfs: strip failed for {}: {}", path.display(), e);
                    match std::fs::File::open(&path) {
                        Ok(f) => OpenFile::Passthrough(f),
                        Err(_) => { reply.error(EIO); return; }
                    }
                }
            }
        } else {
            match std::fs::File::open(&path) {
                Ok(f) => OpenFile::Passthrough(f),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => { reply.error(ENOENT); return; }
                Err(_) => { reply.error(EACCES); return; }
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
        let path = match self.ino_to_path.get(&ino).cloned() {
            Some(p) => p,
            None => { reply.error(ENOENT); return; }
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
