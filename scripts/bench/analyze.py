#!/usr/bin/env python3
"""Render a markdown summary from one or more bench CSVs.

Usage: analyze.py <csv_path> [<csv_path> ...]

Handles both schemas:
  v1 (no parallel col):  approach,size,N,create_ms,disk_apparent_kb,disk_actual_kb,disk_real_kb,cleanup_ms,first_edit_ms
  v2 (with parallel):    approach,size,N,parallel,create_ms,disk_apparent_kb,disk_actual_kb,disk_real_kb,cleanup_ms,first_edit_ms

When a v1 row is read, `parallel` defaults to 1.
"""
import csv
import sys
from collections import defaultdict


APPROACH_LABELS = {
    "A": "git worktree",
    "B": "cp -R (naive)",
    "C": "cp -cR (clonefile, full)",
    "D": "rsync --link-dest (hardlink)",
    "E": "ivk-pure (clonefile WT, no .git)",
    "F": "ivk-hybrid (worktree + clonefile WT)",
}


def fmt_ms(ms: int) -> str:
    ms = int(ms)
    if ms < 1000:
        return f"{ms} ms"
    if ms < 60_000:
        return f"{ms/1000:.2f} s"
    return f"{ms/60_000:.2f} min"


def fmt_kb(kb: int) -> str:
    kb = int(kb)
    if kb < 1024:
        return f"{kb} KB"
    if kb < 1024 * 1024:
        return f"{kb/1024:.1f} MB"
    return f"{kb/1024/1024:.2f} GB"


def load_rows(paths):
    rows = []
    int_cols = ("N", "parallel", "create_ms", "disk_apparent_kb",
                "disk_actual_kb", "disk_real_kb", "cleanup_ms", "first_edit_ms")
    for path in paths:
        with open(path) as f:
            reader = csv.DictReader(f)
            for r in reader:
                if "parallel" not in r:
                    r["parallel"] = "1"
                for k in int_cols:
                    r[k] = int(r[k])
                rows.append(r)
    return rows


def per_n_table(rows_for_size, size, P):
    Ns = sorted({r["N"] for r in rows_for_size if r["parallel"] == P})
    approaches = sorted({r["approach"] for r in rows_for_size if r["parallel"] == P})
    out = []
    for N in Ns:
        base = next((r for r in rows_for_size
                     if r["approach"] == "A" and r["N"] == N and r["parallel"] == P), None)
        out.append(f"### {size}, N={N}, P={P}\n")
        out.append("| approach | create | disk (real) | cleanup | first edit | create vs A | disk vs A |")
        out.append("|---|---:|---:|---:|---:|---:|---:|")
        for ap in approaches:
            r = next((r for r in rows_for_size
                      if r["approach"] == ap and r["N"] == N and r["parallel"] == P), None)
            if not r:
                continue
            label = APPROACH_LABELS.get(ap, ap)
            if base and ap != "A":
                cr = r["create_ms"] / max(1, base["create_ms"])
                dr = r["disk_real_kb"] / max(1, base["disk_real_kb"])
                cr_str = f"{cr:.2f}×"
                dr_str = f"{dr:.3f}×"
            else:
                cr_str = "1.00×"
                dr_str = "1.00×"
            out.append(
                f"| {ap} {label} | {fmt_ms(r['create_ms'])} | "
                f"{fmt_kb(r['disk_real_kb'])} | {fmt_ms(r['cleanup_ms'])} | "
                f"{fmt_ms(r['first_edit_ms'])} | {cr_str} | {dr_str} |"
            )
        out.append("")
    return out


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: analyze.py <csv_path> [<csv_path> ...]", file=sys.stderr)
        return 2

    rows = load_rows(sys.argv[1:])
    if not rows:
        print("no rows", file=sys.stderr)
        return 1

    sizes = sorted({r["size"] for r in rows})
    parallels = sorted({r["parallel"] for r in rows})

    out = ["# Benchmark results\n"]

    for size in sizes:
        size_rows = [r for r in rows if r["size"] == size]
        out.append(f"## Size {size}\n")
        for P in parallels:
            if not any(r["parallel"] == P for r in size_rows):
                continue
            out.extend(per_n_table(size_rows, size, P))

    print("\n".join(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
