# Filesystem Portability for `ivk`

`ivk`'s "cheap workspace" claim rests on a single OS primitive: **block-level copy-on-write** at file granularity. The primitive has a different name on every platform and is not universally available. This document records what works where and what we plan to do about it.

---

## Required primitive

For any workspace materialization strategy to deliver the headline disk savings, the OS must support either:

1. **File reflinks / clonefile** — two files share data blocks until written; writes are copy-on-write per block.
2. **Mountable overlay** — workspace is a thin upper layer over a read-only lower layer; reads pass through, writes go to the upper layer.

Hardlinks are not sufficient: an edit to one hardlinked file mutates the other. They are listed only as an unsafe last-resort fallback.

---

## Per-platform support

| Platform | FS | Primitive | CLI | Status |
|---|---|---|---|---|
| macOS | APFS | `clonefile(2)` | `cp -c`, `cp -cR` | ✅ default on every modern Mac (10.13+) |
| Linux | btrfs | `FICLONE` ioctl | `cp --reflink=auto` | ✅ universal on btrfs |
| Linux | xfs (reflink=1) | `FICLONE` ioctl | `cp --reflink=auto` | ✅ when mounted with `reflink=1` (default on modern distros) |
| Linux | zfs | block clone | `cp --reflink=auto` (zfs ≥ 2.2) | ✅ on recent zfs |
| Linux | bcachefs | reflink | `cp --reflink=auto` | ✅ |
| Linux | ext4 | none | — | ❌ no reflink support; fall back to overlayfs or hardlink |
| Linux | overlayfs | layered mount | mount syscall | ✅ kernel-level overlay; needs root or rootless containers |
| Linux | fuse-overlayfs | layered mount (userspace) | `fuse-overlayfs` | ✅ rootless alternative |
| Windows | ReFS | block cloning | `Copy-Item -Force` with `BlockClone` (PowerShell 7+) | ⚠️ Server / Enterprise SKUs |
| Windows | NTFS | none | — | ❌ no reflink; would need junction + overlay |

---

## What `ivk` should actually do at runtime

1. **Detect strategy at `ivk init`** and persist the choice in `.ivk/config.toml`.
2. **Fall back gracefully** with explicit warnings — never silently degrade to hardlinks (data hazard) or full copies (defeats the pitch).
3. **Expose the strategy** in `ivk doctor --agent --json` so callers know what regime they're in.

Detection logic (proven by the included `scripts/bench/clone.sh`):

```text
if Darwin:                         use APFS clonefile (cp -c / clonefile(2))
elif Linux and FS supports reflink: use reflink (cp --reflink=always)
elif Linux and overlayfs available: use overlayfs (mount each workspace)
elif Linux ext4 and rootless:       use fuse-overlayfs
else:                               warn and either refuse or full-copy
```

The MVP can ship with macOS as the only fully-supported platform and clearly mark Linux as Phase 2.

---

## Why ext4 is the operational pain point

ext4 is the default on most server Linux distributions. It does not support reflinks and will not, by design (its on-disk format predates per-file CoW). For Linux server deployments, `ivk` has two realistic options:

### Option A: require btrfs / xfs (with reflink)

Tell users to provision their dev volume as btrfs or xfs+reflink. This is normal for many CI runners (BuildKit and modern container engines already require this). It is a hard ask for shared corporate infra.

### Option B: use overlayfs

Each workspace is a `mount -t overlay` with the base snapshot as `lowerdir` and a workspace-private `upperdir`. Reads from the workspace pass through to the lower layer; writes accumulate in the upper layer. Disk usage matches the reflink case (only modified blocks consume new space). Performance is comparable.

Constraints:
- Needs the `overlay` kernel module (universal on modern kernels).
- Each mount needs root, *or* a user namespace (rootless containers). For an end-user CLI this is awkward.
- `fuse-overlayfs` solves the privilege problem but introduces userspace I/O cost.

### Option C: accept full copies on ext4, hide it with shared dependency cache

If working-tree CoW is unavailable, the next-best move is to minimize what each workspace duplicates. With shared `node_modules` / `target/` / `.venv/`, even full-copy working trees stay small. This is the path the spike's dependency-cache numbers (MD profile, results below) speak to.

The honest read of the data: working-tree CoW saves a lot in absolute terms, but **dependency-store sharing saves more** for typical projects. ext4 users still benefit from the latter even without the former.

---

## What this means for the MVP

1. macOS only for Phase 0–2. Document this. Don't pretend to ship Linux until it's tested.
2. Ship `clone.sh` as a documented portable wrapper inside the bench scripts so contributors can repro on Linux+btrfs from day 1.
3. Plan Linux support as Phase 3 with two tracks: reflink (btrfs/xfs) and overlayfs. ext4 either requires dependency-cache-only mode or remains unsupported.
4. Windows: out of scope. ReFS is too niche; NTFS does not have the primitive.
