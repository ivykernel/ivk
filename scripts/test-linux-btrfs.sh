#!/usr/bin/env bash
# Validate ivk's Linux backend (native FICLONE) on a real btrfs filesystem.
#
# Runs inside a privileged Docker container (needed to mount loopback btrfs),
# builds ivk for Linux, mounts a btrfs image, runs the workspace integration
# tests with TMPDIR on btrfs so the tests actually exercise reflink.
#
# Also runs the same tests with TMPDIR on the container's default overlayfs
# (which doesn't support reflink) to confirm the friendly EOPNOTSUPP error
# fires.
#
# Usage: bash scripts/test-linux-btrfs.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if ! /usr/bin/which docker >/dev/null 2>&1; then
  echo "ERROR: docker not found (need orbstack / Docker Desktop / colima)" >&2
  exit 2
fi

if ! docker info >/dev/null 2>&1; then
  echo "ERROR: docker daemon not reachable" >&2
  exit 2
fi

IMG="rust:1.83-bookworm"

echo "[linux-btrfs] pulling $IMG ..."
docker pull -q "$IMG" >/dev/null

echo "[linux-btrfs] running build + tests inside container ..."
docker run --rm --privileged \
  -v "$ROOT:/src:ro" \
  -v "$HOME/.cargo/registry:/usr/local/cargo/registry" \
  "$IMG" \
  bash -euxo pipefail -c '
    # Copy source into a writable location (the bind mount is read-only so
    # cargo can write target/ inside the container).
    cp -R /src /work
    cd /work

    # btrfs tools.
    apt-get update -qq
    apt-get install -y -qq btrfs-progs >/dev/null

    # 512 MB loopback btrfs.
    dd if=/dev/zero of=/tmp/btrfs.img bs=1M count=512 2>/dev/null
    mkfs.btrfs -q /tmp/btrfs.img
    mkdir -p /mnt/btrfs
    mount /tmp/btrfs.img /mnt/btrfs
    echo "--- btrfs mount OK ---"
    mount | grep btrfs

    # Build (target/ is on overlayfs; not the bench target).
    cargo build --release --workspace --quiet

    # Smoke: native FICLONE end-to-end.
    echo "--- smoke (TMPDIR on btrfs) ---"
    TMPDIR=/mnt/btrfs cargo test --release \
        -p ivk-core --test integration -- --nocapture

    echo ""
    echo "--- ext4 friendly-error test (TMPDIR on container overlayfs) ---"
    # Default TMPDIR is /tmp which is overlayfs in containers; FICLONE
    # will fail with EOPNOTSUPP and our backend remaps it to a clear
    # message. We just want to see the message in the error path.
    /tmp/ivk-fail-probe() { :; }
    mkdir -p /tmp/src-probe
    echo hello > /tmp/src-probe/f
    cd /tmp/src-probe && git init -q -b main \
      && git -c user.email=t@t -c user.name=t add -A \
      && git -c user.email=t@t -c user.name=t commit -q -m initial
    # Try to materialize onto overlayfs.
    /work/target/release/clonewt /tmp/src-probe /tmp/dst-probe 2>&1 \
      | tee /tmp/err.log || true
    # Confirm the message mentions reflink + portability.md.
    if grep -q "reflink" /tmp/err.log && grep -q "portability.md" /tmp/err.log; then
      echo "--- friendly error confirmed ---"
    else
      echo "--- friendly error NOT confirmed; output above ---" >&2
      exit 1
    fi

    echo "--- ALL LINUX CHECKS PASSED ---"
  '
