#!/usr/bin/env python3
"""Generate a synthetic repository of approximate target size.

Usage: gen_repo.py <S|M|L> <dest_dir>

Profile (file_count, total_bytes_target):
  S:   1_000,    10 MiB
  M:  10_000,   200 MiB
  L: 100_000,  2048 MiB

Distribution:
  90% small  (avg ~2 KB)
  9% medium  (avg ~50 KB)
  1% large   (avg ~1 MB)

Determinism: seeded random, but file contents come from os.urandom for speed.
The benchmark cares about file count + bytes, not exact content.
"""
import os
import random
import sys
import time

PROFILES = {
    "S":  (1_000,    10 * 1024 * 1024),
    "M":  (10_000,  200 * 1024 * 1024),
    "L":  (100_000, 2 * 1024 * 1024 * 1024),
    # MD = realistic medium project: 10k source files (~200 MB) + a synthesized
    # node_modules-like subdir of ~15k smaller files (~400 MB). Total ~600 MB.
    # Generated as the M profile plus an extra "node_modules/" tree.
    "MD": (10_000,  200 * 1024 * 1024),
}

# Some profiles add an extra subtree to simulate installed dependencies.
EXTRA_DEPS = {
    "MD": ("node_modules", 15_000, 400 * 1024 * 1024),
}


def assign_size(rng: random.Random) -> int:
    r = rng.random()
    if r < 0.90:
        return rng.randint(500, 4_000)
    if r < 0.99:
        return rng.randint(20_000, 100_000)
    return rng.randint(500_000, 2_000_000)


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: gen_repo.py <S|M|L> <dest_dir>", file=sys.stderr)
        return 2
    size_key, dest = sys.argv[1], sys.argv[2]
    if size_key not in PROFILES:
        print(f"unknown size: {size_key}", file=sys.stderr)
        return 2

    n_files, total_target = PROFILES[size_key]
    rng = random.Random(42 + hash(size_key) % 10_000)

    # Pre-assign per-file sizes, then scale to hit the target total.
    sizes = [assign_size(rng) for _ in range(n_files)]
    ratio = total_target / sum(sizes)
    sizes = [max(64, int(s * ratio)) for s in sizes]

    # Layout files across a small directory tree (max ~200 files per dir, depth 1-3).
    dir_count = max(1, n_files // 100)
    dirs = []
    for _ in range(dir_count):
        depth = rng.randint(1, 3)
        parts = [f"d{rng.randint(0,9)}{rng.randint(0,9)}" for _ in range(depth)]
        dirs.append("/".join(parts))

    # Pre-generate one large random buffer; slice per file to avoid expensive
    # repeated os.urandom calls (which become the bottleneck at 100k files).
    # +1MB headroom so the per-file slice offset has room to vary.
    BUF_SIZE = max(2 * 1024 * 1024, max(sizes) + 1024 * 1024)
    buf = os.urandom(BUF_SIZE)

    os.makedirs(dest, exist_ok=True)

    t0 = time.perf_counter()
    bytes_written = 0
    for idx, sz in enumerate(sizes):
        d = dirs[idx % len(dirs)]
        dpath = os.path.join(dest, d)
        if not os.path.isdir(dpath):
            os.makedirs(dpath, exist_ok=True)
        fpath = os.path.join(dpath, f"f{idx:06d}.txt")
        # Vary the slice offset by file index so contents differ across files.
        off = (idx * 4096) % (BUF_SIZE - sz)
        with open(fpath, "wb") as f:
            f.write(buf[off:off + sz])
        bytes_written += sz
        if idx and idx % 10_000 == 0:
            elapsed = time.perf_counter() - t0
            print(f"  ... {idx:>7}/{n_files} files, "
                  f"{bytes_written/1e6:.1f} MB, {elapsed:.1f}s", file=sys.stderr)

    elapsed = time.perf_counter() - t0
    print(f"wrote {n_files} files ({bytes_written/1e6:.1f} MB) in {elapsed:.2f}s")

    # If this profile has extra deps, generate them under a subdir.
    if size_key in EXTRA_DEPS:
        subdir_name, dep_n, dep_total = EXTRA_DEPS[size_key]
        dep_root = os.path.join(dest, subdir_name)
        # node_modules has many shallow package dirs containing small files
        # (package.json, README, index.js, etc). Simulate with ~50 files per
        # "package" so each top-level dir feels package-shaped.
        files_per_pkg = 50
        pkg_count = max(1, dep_n // files_per_pkg)
        sizes = [max(256, int(dep_total / dep_n)) for _ in range(dep_n)]
        rng2 = random.Random(rng.random())
        # Mild variance
        sizes = [max(64, int(s * rng2.uniform(0.3, 2.5))) for s in sizes]
        ratio = dep_total / sum(sizes)
        sizes = [max(64, int(s * ratio)) for s in sizes]

        t1 = time.perf_counter()
        dep_written = 0
        for idx, sz in enumerate(sizes):
            pkg = idx // files_per_pkg
            d = os.path.join(dep_root, f"pkg{pkg:04d}")
            if idx % files_per_pkg == 0:
                os.makedirs(d, exist_ok=True)
            fpath = os.path.join(d, f"f{idx % files_per_pkg:03d}.js")
            off = (idx * 4096) % (BUF_SIZE - sz)
            with open(fpath, "wb") as f:
                f.write(buf[off:off + sz])
            dep_written += sz
            if idx and idx % 10_000 == 0:
                el = time.perf_counter() - t1
                print(f"  ... deps {idx:>7}/{dep_n}, "
                      f"{dep_written/1e6:.1f} MB, {el:.1f}s", file=sys.stderr)
        el = time.perf_counter() - t1
        print(f"wrote {dep_n} dep files ({dep_written/1e6:.1f} MB) in {el:.2f}s "
              f"across {pkg_count} packages")

    return 0


if __name__ == "__main__":
    sys.exit(main())
