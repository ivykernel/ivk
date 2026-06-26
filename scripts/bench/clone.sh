#!/usr/bin/env bash
# Portable wrapper around the platform's reflink/clonefile primitive.
#
# Usage: clone.sh <src> <dst>
#
# Strategy by platform:
#   Darwin (APFS):    cp -cR                          (APFS clonefile(2), always works on modern Macs)
#   Linux + reflink:  cp --reflink=auto -R            (btrfs / xfs-with-reflink / zfs / new bcachefs)
#   Linux fallback:   cp -al                          (hardlink tree — UNSAFE on edits unless caller
#                                                      is prepared to break-on-write, e.g. via overlayfs)
#   Other:            error                           (refuse silently rather than silently degrade)
#
# Detection prefers an explicit IVK_CLONE_STRATEGY env var so callers can force
# a strategy in CI / debugging. Otherwise uname + a one-shot probe.
set -euo pipefail

SRC="${1:?usage: clone.sh <src> <dst>}"
DST="${2:?dst required}"

OS_KIND="$(uname -s)"
STRATEGY="${IVK_CLONE_STRATEGY:-auto}"

probe_linux_reflink() {
  # Try cp --reflink=always on a 1-byte file. If it succeeds, the FS supports reflinks.
  local tmpdir tmp1 tmp2
  tmpdir="$(/usr/bin/mktemp -d)"
  tmp1="$tmpdir/a"
  tmp2="$tmpdir/b"
  /bin/echo x > "$tmp1"
  if /bin/cp --reflink=always "$tmp1" "$tmp2" 2>/dev/null; then
    /bin/rm -rf "$tmpdir"
    return 0
  fi
  /bin/rm -rf "$tmpdir"
  return 1
}

if [[ "$STRATEGY" == "auto" ]]; then
  case "$OS_KIND" in
    Darwin) STRATEGY="apfs-clonefile" ;;
    Linux)
      if probe_linux_reflink; then
        STRATEGY="linux-reflink"
      else
        STRATEGY="linux-hardlink"
      fi
      ;;
    *)
      echo "clone.sh: no clone strategy for $OS_KIND" >&2
      exit 3
      ;;
  esac
fi

case "$STRATEGY" in
  apfs-clonefile) /bin/cp -cR "$SRC" "$DST" ;;
  linux-reflink)  /bin/cp --reflink=always -R "$SRC" "$DST" ;;
  linux-hardlink)
    # Last-resort fallback: hardlink tree. Caller must ensure that workspace
    # writes use copy-on-write semantics (e.g. via overlayfs upperdir) or
    # accept that an edit to a workspace file mutates the source.
    /bin/cp -al "$SRC" "$DST"
    ;;
  *)
    echo "clone.sh: unknown strategy '$STRATEGY'" >&2
    exit 3
    ;;
esac
