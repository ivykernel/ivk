#!/usr/bin/env python3
"""Render a self-contained SVG chart for the LP from one or more bench CSVs.

Usage: chart.py <out.svg> <csv_path> [<csv_path> ...]

Produces a two-line chart:
  X: number of workspaces (log-spaced ticks at N=1, 10, 50, 100)
  Y: disk consumed (log scale to fit both git worktree and ivk on one axis)
  Lines: approach A (git worktree) vs approach F (ivk-hybrid), serial only.

No external dependencies — emits raw SVG.
"""
import csv
import math
import sys
from collections import defaultdict


def load(paths):
    rows = []
    int_cols = ("N", "create_ms", "disk_apparent_kb",
                "disk_actual_kb", "disk_real_kb", "cleanup_ms", "first_edit_ms")
    for p in paths:
        with open(p) as f:
            reader = csv.DictReader(f)
            for r in reader:
                if "parallel" not in r:
                    r["parallel"] = "1"
                for k in int_cols:
                    if k in r:
                        r[k] = int(r[k])
                r["parallel"] = int(r["parallel"])
                rows.append(r)
    return rows


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: chart.py <out.svg> <csv> [<csv>...]", file=sys.stderr)
        return 2
    out_path = sys.argv[1]
    rows = [r for r in load(sys.argv[2:]) if r["parallel"] == 1
            and r["approach"] in ("A", "G")
            and r["size"] in ("M", "MD")]

    # Group: (approach, size) -> [(N, disk_real_kb), ...]
    series = defaultdict(list)
    for r in rows:
        series[(r["approach"], r["size"])].append((r["N"], r["disk_real_kb"]))
    for k in series:
        series[k].sort()

    # Chart geometry
    W, H = 1100, 620
    margin = {"top": 70, "right": 220, "bottom": 70, "left": 90}
    plot_w = W - margin["left"] - margin["right"]
    plot_h = H - margin["top"] - margin["bottom"]

    # Data range
    xs = sorted({n for s in series.values() for n, _ in s})
    if not xs:
        print("no data", file=sys.stderr)
        return 1
    ymax = max(d for s in series.values() for _, d in s)
    ymin = max(1, min(d for s in series.values() for _, d in s))

    # X linear, Y log
    def x_to_px(n):
        # Log spacing so 1/10/50/100 look ok
        lo, hi = math.log10(min(xs)), math.log10(max(xs))
        return margin["left"] + (math.log10(n) - lo) / (hi - lo) * plot_w

    def y_to_px(d):
        lo, hi = math.log10(ymin), math.log10(ymax)
        return margin["top"] + plot_h - (math.log10(d) - lo) / (hi - lo) * plot_h

    # Colors / styling
    palette = {
        ("A", "M"):  ("#c45a5a", "git worktree (M: code)",        "8,4"),
        ("A", "MD"): ("#9a2a2a", "git worktree (MD: code+deps)",  None),
        ("G", "M"):  ("#3a8fbf", "ivk (M: code)",                  "8,4"),
        ("G", "MD"): ("#1c5980", "ivk (MD: code+deps)",            None),
    }

    parts = []
    parts.append(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" '
        f'font-family="-apple-system, BlinkMacSystemFont, Segoe UI, sans-serif" '
        f'font-size="14" fill="#1f2937">'
    )
    parts.append('<rect width="100%" height="100%" fill="#fafafa"/>')

    # Title
    parts.append(
        f'<text x="{W/2}" y="32" text-anchor="middle" '
        f'font-size="22" font-weight="600">'
        f'Disk consumed by N parallel workspaces'
        f'</text>'
    )
    parts.append(
        f'<text x="{W/2}" y="54" text-anchor="middle" '
        f'fill="#6b7280" font-size="13">'
        f'Same repo, same end state. log scale — note the gap.'
        f'</text>'
    )

    # Axes
    ax_color = "#9ca3af"
    parts.append(
        f'<line x1="{margin["left"]}" y1="{margin["top"] + plot_h}" '
        f'x2="{margin["left"] + plot_w}" y2="{margin["top"] + plot_h}" '
        f'stroke="{ax_color}" stroke-width="1"/>'
    )
    parts.append(
        f'<line x1="{margin["left"]}" y1="{margin["top"]}" '
        f'x2="{margin["left"]}" y2="{margin["top"] + plot_h}" '
        f'stroke="{ax_color}" stroke-width="1"/>'
    )

    # X ticks
    for n in xs:
        x = x_to_px(n)
        parts.append(
            f'<line x1="{x}" y1="{margin["top"] + plot_h}" '
            f'x2="{x}" y2="{margin["top"] + plot_h + 6}" '
            f'stroke="{ax_color}"/>'
        )
        parts.append(
            f'<text x="{x}" y="{margin["top"] + plot_h + 22}" '
            f'text-anchor="middle" fill="#374151">{n}</text>'
        )
    parts.append(
        f'<text x="{margin["left"] + plot_w/2}" '
        f'y="{margin["top"] + plot_h + 50}" '
        f'text-anchor="middle" font-size="14" fill="#374151">'
        f'workspaces (N)</text>'
    )

    # Y ticks (powers of 10 + a few mid)
    def fmt_disk_kb(kb):
        if kb < 1024: return f"{kb} KB"
        if kb < 1024 * 1024: return f"{kb//1024} MB"
        return f"{kb/1024/1024:.1f} GB"

    log_lo, log_hi = math.floor(math.log10(ymin)), math.ceil(math.log10(ymax))
    for k in range(int(log_lo), int(log_hi) + 1):
        v = 10 ** k
        if v < ymin or v > ymax:
            continue
        y = y_to_px(v)
        parts.append(
            f'<line x1="{margin["left"] - 6}" y1="{y}" '
            f'x2="{margin["left"] + plot_w}" y2="{y}" '
            f'stroke="#e5e7eb" stroke-width="1"/>'
        )
        parts.append(
            f'<text x="{margin["left"] - 10}" y="{y + 5}" '
            f'text-anchor="end" fill="#374151">{fmt_disk_kb(v)}</text>'
        )

    # Lines
    for key, (color, label, dash) in palette.items():
        pts = series.get(key, [])
        if len(pts) < 2:
            continue
        path = "M " + " L ".join(f"{x_to_px(n)},{y_to_px(d)}" for n, d in pts)
        dash_attr = f' stroke-dasharray="{dash}"' if dash else ''
        parts.append(
            f'<path d="{path}" fill="none" stroke="{color}" '
            f'stroke-width="2.5"{dash_attr}/>'
        )
        # End-point dots
        for n, d in pts:
            parts.append(
                f'<circle cx="{x_to_px(n)}" cy="{y_to_px(d)}" r="4" '
                f'fill="{color}"/>'
            )

    # Annotations on the endpoints at N=100
    if xs:
        x100 = x_to_px(max(xs))
        for key, (color, label, _) in palette.items():
            pts = series.get(key, [])
            if not pts:
                continue
            n_max, d_max = pts[-1]
            y = y_to_px(d_max)
            parts.append(
                f'<text x="{x100 + 14}" y="{y + 4}" fill="{color}" '
                f'font-weight="600">{fmt_disk_kb(d_max)}</text>'
            )

    # Legend
    lx, ly = margin["left"] + plot_w + 40, margin["top"] + 40
    parts.append(
        f'<text x="{lx - 10}" y="{ly - 18}" font-weight="600" font-size="13">'
        f'approach × repo</text>'
    )
    row_h = 22
    for i, (key, (color, label, dash)) in enumerate(palette.items()):
        ty = ly + i * row_h
        dash_attr = f' stroke-dasharray="{dash}"' if dash else ''
        parts.append(
            f'<line x1="{lx - 8}" y1="{ty - 5}" x2="{lx + 16}" y2="{ty - 5}" '
            f'stroke="{color}" stroke-width="2.5"{dash_attr}/>'
        )
        parts.append(
            f'<text x="{lx + 22}" y="{ty}" fill="#1f2937">{label}</text>'
        )

    parts.append('</svg>')

    with open(out_path, "w") as f:
        f.write("\n".join(parts))
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
