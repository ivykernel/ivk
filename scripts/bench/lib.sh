#!/usr/bin/env bash
# Shared helpers for ivk benchmark spike scripts.

# Use absolute paths to avoid user shell aliases (e.g. cp -> cpi, rm -> rm -i).
CP=/bin/cp
RM=/bin/rm
MV=/bin/mv
LN=/bin/ln

# Milliseconds since epoch. Perl is faster to launch than python3 on macOS.
now_ms() {
  /usr/bin/perl -MTime::HiRes=time -E 'say int(time()*1000)'
}

# OS detection.
OS_KIND="$(uname -s)"
case "$OS_KIND" in
  Darwin) ;;
  Linux)  ;;
  *) echo "WARN: untested OS: $OS_KIND" >&2 ;;
esac

# du flags: macOS BSD du and GNU du have different semantics.
#   macOS:  du -sk           -> actual blocks allocated (KB)
#           du -skA          -> apparent size (sum of file sizes, KB)
#   Linux:  du -sk           -> actual blocks (KB)
#           du -sk --apparent-size  -> apparent
du_actual_kb() {
  /usr/bin/du -sk "$1" | awk '{print $1}'
}
du_apparent_kb() {
  if [[ "$OS_KIND" == "Darwin" ]]; then
    /usr/bin/du -skA "$1" | awk '{print $1}'
  else
    /usr/bin/du -sk --apparent-size "$1" | awk '{print $1}'
  fi
}

# Best-effort fsync + cache hint. Real cache-drop requires root and varies by OS;
# we just sync so that pending writes don't get attributed to the next phase.
fs_sync() {
  /bin/sync || true
}

# Free KB on the volume containing the given path.
# On APFS, this is the ground truth for "how much disk did we actually consume",
# because clonefile-shared blocks are not reflected in `du -sk`.
df_free_kb() {
  /bin/df -k "$1" | /usr/bin/awk 'NR==2 {print $4}'
}

# Resolve repo root (the ivk/ directory) given a script in scripts/bench/.
script_dir() { cd "$(dirname "$1")" && pwd; }
repo_root_from_script() {
  cd "$(dirname "$1")/../.." && pwd
}
