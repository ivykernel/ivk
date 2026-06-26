#!/usr/bin/env bash
# Materialize a single workspace using the requested primitive.
#
# Usage: create-one.sh <APPROACH> <REPO> <DST>
#
# Designed to be the unit invoked by both serial and parallel runners.
# Keep it small and side-effect-free beyond writing $DST.
set -euo pipefail

APPROACH="${1:?approach required}"
REPO="${2:?repo required}"
DST="${3:?dst required}"

case "$APPROACH" in
  A)  # git worktree (baseline)
      /usr/bin/git -C "$REPO" worktree add -q --detach "$DST" HEAD
      ;;
  B)  # naive deep copy
      /bin/cp -R "$REPO" "$DST"
      ;;
  C)  # clonefile (full clone, including .git)
      /bin/cp -cR "$REPO" "$DST"
      ;;
  D)  # rsync hardlink tree
      /usr/bin/rsync -a --link-dest="$REPO/" "$REPO/" "$DST/"
      ;;
  E)  # ivk-pure: clonefile working tree only, no .git/ at all
      /bin/mkdir -p "$DST"
      for entry in "$REPO"/*; do
        /bin/cp -cR "$entry" "$DST/"
      done
      ;;
  F)  # ivk-hybrid (shell): git worktree pointer + clonefile working tree
      # The git worktree admin dir + .git pointer file is created without checkout,
      # then we clonefile working-tree files into it. Each top-level entry forks
      # a cp -cR; that fork overhead is the structural cost approach G eliminates.
      /usr/bin/git -C "$REPO" worktree add -q --no-checkout --detach "$DST" HEAD
      for entry in "$REPO"/*; do
        /bin/cp -cR "$entry" "$DST/"
      done
      ;;
  G)  # ivk-hybrid (Rust): same architecture as F but no per-entry fork.
      # Calls clonefile(2) directly from a single binary, then `read-tree HEAD`
      # so the workspace's index matches HEAD (clean `git status`).
      _SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
      _CLONEWT="$_SCRIPT_DIR/../../target/release/clonewt"
      if [[ ! -x "$_CLONEWT" ]]; then
        echo "approach G needs the clonewt binary at $_CLONEWT" >&2
        echo "build it with: cargo build --release --workspace" >&2
        exit 2
      fi
      "$_CLONEWT" "$REPO" "$DST"
      ;;
  *)
      echo "unknown approach: $APPROACH" >&2
      exit 2
      ;;
esac
