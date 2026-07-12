//! Working-tree materialization backends.
//!
//! A `Materializer` clones one path (file, directory tree, or symlink) from
//! the source repo into a workspace. The default is copy-on-write
//! ([`CowMaterializer`]: APFS `clonefile(2)` on macOS, `FICLONE` on Linux),
//! which is what makes 100 workspaces fit in the disk of one.
//!
//! [`CopyMaterializer`] is the plain-copy fallback for filesystems without
//! reflink support (ext4, network mounts). It is deliberately **not**
//! auto-selected — silently losing the CoW win would falsify the disk story.
//! Selection policy (config / per-FS detection) is Phase B; iOS may also
//! inject a host-side copy callback through the FFI layer (Phase D).

use std::fs;
use std::io;
use std::path::Path;

/// Clones paths from a source tree into a workspace.
pub trait Materializer {
    /// Stable strategy label, surfaced in reports and `ivk doctor`.
    fn strategy(&self) -> &'static str;

    /// Clone `src` to `dst`. `dst` must not exist. Directories are cloned
    /// recursively; symlinks are preserved as symlinks.
    fn clone_path(&self, src: &Path, dst: &Path) -> io::Result<()>;
}

/// The platform copy-on-write primitive.
#[derive(Debug, Clone, Copy, Default)]
pub struct CowMaterializer;

/// Plain recursive copy. Works on any filesystem; shares no blocks.
#[derive(Debug, Clone, Copy, Default)]
pub struct CopyMaterializer;

/// Strategy label of the platform default materializer.
pub fn default_strategy() -> &'static str {
    CowMaterializer.strategy()
}

impl Materializer for CowMaterializer {
    fn strategy(&self) -> &'static str {
        cow::STRATEGY
    }

    fn clone_path(&self, src: &Path, dst: &Path) -> io::Result<()> {
        cow::clone_path(src, dst)
    }
}

impl Materializer for CopyMaterializer {
    fn strategy(&self) -> &'static str {
        "std-copy"
    }

    fn clone_path(&self, src: &Path, dst: &Path) -> io::Result<()> {
        copy_tree(src, dst)
    }
}

fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    let ft = meta.file_type();
    if ft.is_file() {
        fs::copy(src, dst).map(|_| ())
    } else if ft.is_dir() {
        fs::create_dir(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let name = entry.file_name();
            copy_tree(&src.join(&name), &dst.join(&name))?;
        }
        Ok(())
    } else if ft.is_symlink() {
        let target = fs::read_link(src)?;
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, dst);
        #[cfg(not(unix))]
        {
            let _ = target;
            Ok(())
        }
    } else {
        // FIFOs, sockets, devices: not part of a working tree in any
        // practical sense; skip silently (same as the CoW path).
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod cow {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    pub const STRATEGY: &str = "apfs-clonefile";

    extern "C" {
        // int clonefile(const char *src, const char *dst, uint32_t flags);
        fn clonefile(src: *const i8, dst: *const i8, flags: u32) -> i32;
    }

    fn to_cstring(p: &Path) -> io::Result<CString> {
        CString::new(p.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL byte"))
    }

    pub fn clone_path(src: &Path, dst: &Path) -> io::Result<()> {
        let src_c = to_cstring(src)?;
        let dst_c = to_cstring(dst)?;
        // clonefile is recursive on directories: one syscall per top-level
        // entry covers the whole subtree.
        let ret = unsafe { clonefile(src_c.as_ptr(), dst_c.as_ptr(), 0) };
        if ret == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(target_os = "linux")]
mod cow {
    use std::fs;
    use std::io;
    use std::os::unix::fs::symlink as symlink_unix;
    use std::os::unix::io::AsRawFd;
    use std::path::Path;

    pub const STRATEGY: &str = "linux-ficlone";

    /// FICLONE ioctl. Linux's per-file reflink: shares all extents of `src`
    /// with `dst` until either is written to. Equivalent to APFS's
    /// `clonefile(2)` for a single file, but NOT recursive on directories;
    /// we walk the tree ourselves below.
    ///
    /// Value: `_IOW('X', 9, int)` = `0x40049409` on most architectures.
    /// Defined in `<linux/fs.h>`; not currently in the `libc` crate's
    /// public surface, so we hard-code the constant.
    const FICLONE: libc::c_ulong = 0x4004_9409;

    pub fn clone_path(src: &Path, dst: &Path) -> io::Result<()> {
        clone_tree(src, dst)
    }

    fn clone_tree(src: &Path, dst: &Path) -> io::Result<()> {
        let meta = fs::symlink_metadata(src)?;
        let ft = meta.file_type();
        if ft.is_file() {
            clone_file(src, dst).map_err(map_unsupported_fs)
        } else if ft.is_dir() {
            fs::create_dir(dst)?;
            for entry in fs::read_dir(src)? {
                let entry = entry?;
                let name = entry.file_name();
                clone_tree(&src.join(&name), &dst.join(&name))?;
            }
            Ok(())
        } else if ft.is_symlink() {
            let target = fs::read_link(src)?;
            symlink_unix(target, dst)
        } else {
            // FIFOs, sockets, block/char devices: not part of a working tree
            // in any practical sense; skip silently.
            Ok(())
        }
    }

    fn clone_file(src: &Path, dst: &Path) -> io::Result<()> {
        let src_f = fs::File::open(src)?;
        let dst_f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dst)?;
        let ret = unsafe { libc::ioctl(dst_f.as_raw_fd(), FICLONE, src_f.as_raw_fd()) };
        if ret == 0 {
            Ok(())
        } else {
            // Clean up the empty file we just created so the caller doesn't
            // see a half-cloned state.
            let err = io::Error::last_os_error();
            let _ = fs::remove_file(dst);
            Err(err)
        }
    }

    /// Map `EOPNOTSUPP` / `EINVAL` from FICLONE into a human-actionable
    /// message. These are the errors users hit on ext4 or other
    /// non-reflink filesystems.
    fn map_unsupported_fs(e: io::Error) -> io::Error {
        match e.raw_os_error() {
            Some(libc::EOPNOTSUPP) | Some(libc::EINVAL) | Some(libc::ENOTTY) => {
                io::Error::other(format!(
                    "{} — the destination filesystem does not support reflink. \
                     ivk works on macOS APFS and on Linux btrfs / xfs (reflink=1) / \
                     zfs ≥ 2.2 / bcachefs. ext4 is unsupported; \
                     see docs/portability.md for the planned overlayfs fallback.",
                    e
                ))
            }
            _ => e,
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("ivk-core currently supports only macOS and Linux");
